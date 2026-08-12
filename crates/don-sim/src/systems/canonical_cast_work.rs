// SPDX-License-Identifier: GPL-3.0-or-later
//! Atomic production seam for the fresh retail Fishermen DEPLOY order.
//!
//! The fresh v16 SVX has one `CastOrder`: active Unit `(who=0,o=16)`, type 317
//! (`Fishermen`), no-target sentinel, `paid=1`, and spell 658 (`Deploy4`).  Its saved
//! `UnitData::spell_time` is 12.  In `Unit::do_cast` this is an already-open untargeted
//! deploy frame.  Fishermen are not siege, so the `has_general`/contained-Guy timer cone is
//! skipped; the only write before the `get_job_time` comparison is `spell_time++`.
//!
//! This adapter owns that exact waiting branch.  Spell/type virtual results and the
//! installed job-time result are revision-bound authority.  Payment, opening animation,
//! siege/general coupling, completion/spell effects, targets, and every other Cast shape
//! fail closed before publication.

use crate::objects::OWNER_SLOTS;
use crate::order::{Order, OrderIndex};
use crate::systems::economy_order_payload_authority::{
    CastOrderPayload, EconomyOrderPayload, StableTargetIdentity,
};
use crate::world::{Handle, World, OBJ_FLAG_ACTIVE};

pub const FRESH_CAST_ACTOR_TYPE: i32 = 317;
pub const FRESH_CAST_SPELL: i32 = 0x292;
pub const FRESH_CAST_JOB_TIME: i16 = 40;
pub const FRESH_CAST_SPELL_TIME: i16 = 12;
pub const FRESH_CAST_PAYLOAD_BYTES: usize = 27;
pub const UNIT_DO_CAST_VA: u32 = 0x005e_bfe0;
pub const UNIT_DO_CAST_BYTES: usize = 4_191;
pub const UNIT_DO_CAST_SHA256: &str =
    "e6ddf5fe099b840ec1241f9b8d7aba81884646f2cf8ab45992ccdfce4602da7a";
pub const FRESH_CAST_PAYLOAD_SHA256: &str =
    "81cdba3f17e0185b23b1179e44b0b40fd841e2d0b819baa965a44d577bf62ab9";
pub const FRESH_CAST_UNIT_SHA256: &str =
    "1f2afb9d1cb4697bb0546a16adbb6f6d75d8d03f7e0d3764e163e9396065e612";
pub const FRESH_CAST_GUY_SHA256: &str =
    "a1192bce1ac9672a768c59e685cb4162ec8a49ec12efe3abe5c3fa5274991687";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CastWorkOrder {
    pub node_metric: u8,
    pub flags: u8,
    pub target_o: i32,
    pub target_who: i32,
    pub target_uid: u16,
    pub x: i32,
    pub y: i32,
    pub paid: i32,
    pub spell: i32,
}

impl CastWorkOrder {
    pub const FRESH: Self = Self {
        node_metric: 0,
        flags: 0,
        target_o: -1,
        target_who: -1,
        target_uid: u16::MAX,
        x: -1,
        y: -1,
        paid: 1,
        spell: FRESH_CAST_SPELL,
    };

    /// Exact `CastOrder::walk_data` image, beginning with inherited `UnitOrder::flags`.
    pub fn retail_payload_image(self) -> [u8; FRESH_CAST_PAYLOAD_BYTES] {
        let mut out = [0; FRESH_CAST_PAYLOAD_BYTES];
        out[0] = self.flags;
        out[1..5].copy_from_slice(&self.target_o.to_le_bytes());
        out[5..9].copy_from_slice(&self.target_who.to_le_bytes());
        out[9..11].copy_from_slice(&self.target_uid.to_le_bytes());
        out[11..15].copy_from_slice(&self.x.to_le_bytes());
        out[15..19].copy_from_slice(&self.y.to_le_bytes());
        out[19..23].copy_from_slice(&self.paid.to_le_bytes());
        out[23..27].copy_from_slice(&self.spell.to_le_bytes());
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CastActorImage {
    pub who: u8,
    pub o: i16,
    pub uid: u16,
    pub type_index: i32,
    pub active: bool,
    pub spell_time: i16,
}

/// Installed executable/content observations used on this exact path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CastTypeFacts {
    /// Result of the spell object's virtual `+0x48` real-SpellType predicate.
    pub real_spell: bool,
    /// Exact `SpellTypeData::spell_flags` value. Bits `0x0e` select the targeted domain.
    pub spell_flags: u32,
    /// Results of the spell object's virtual `+0x50/+0x54` predicates.
    pub pack_predicate: bool,
    pub deploy_predicate: bool,
    /// Result of actor `UnitTypeData` virtual `+0x10c` (`is_siege`).
    pub actor_is_siege: bool,
    /// Exact returned value of `SpellTypeData::get_job_time(actor_o, actor_who)` after all
    /// content, leader, attribute, and general modifiers.
    pub job_time: i16,
}

impl CastTypeFacts {
    pub const FRESH_FISHERMEN_DEPLOY: Self = Self {
        real_spell: true,
        spell_flags: 0,
        pack_predicate: false,
        deploy_predicate: true,
        actor_is_siege: false,
        job_time: FRESH_CAST_JOB_TIME,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CastWorkSnapshot {
    pub authority_revision: u64,
    pub actor: CastActorImage,
    pub order: CastWorkOrder,
    pub type_facts: CastTypeFacts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastWorkBranch {
    FishermenDeployWaiting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CastWorkPlan {
    pub before: CastWorkSnapshot,
    pub after: CastWorkSnapshot,
    pub branch: CastWorkBranch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnownedCastArm {
    Payment,
    Targeted,
    UntargetedOpening,
    Pack,
    NonDeploy,
    SiegeGeneralAndContainedTimers,
    CompletionAndSpellEffects,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastWorkPlanError {
    FreshPayloadMismatch,
    ActorTypeMismatch(i32),
    InactiveActor,
    InvalidJobTime(i16),
    Unowned(UnownedCastArm),
}

/// Plan the exact saved Fishermen/Deploy4 waiting frame.
pub fn plan_fresh_cast_tick(before: CastWorkSnapshot) -> Result<CastWorkPlan, CastWorkPlanError> {
    if before.order != CastWorkOrder::FRESH {
        return Err(CastWorkPlanError::FreshPayloadMismatch);
    }
    if before.actor.type_index != FRESH_CAST_ACTOR_TYPE {
        return Err(CastWorkPlanError::ActorTypeMismatch(
            before.actor.type_index,
        ));
    }
    if !before.actor.active {
        return Err(CastWorkPlanError::InactiveActor);
    }
    if !before.type_facts.real_spell {
        return Err(CastWorkPlanError::Unowned(UnownedCastArm::Payment));
    }
    if before.type_facts.spell_flags & 0x0e != 0 {
        return Err(CastWorkPlanError::Unowned(UnownedCastArm::Targeted));
    }
    if before.actor.spell_time == 0 {
        return Err(CastWorkPlanError::Unowned(
            UnownedCastArm::UntargetedOpening,
        ));
    }
    if before.type_facts.pack_predicate {
        return Err(CastWorkPlanError::Unowned(UnownedCastArm::Pack));
    }
    if !before.type_facts.deploy_predicate {
        return Err(CastWorkPlanError::Unowned(UnownedCastArm::NonDeploy));
    }
    if before.type_facts.actor_is_siege {
        return Err(CastWorkPlanError::Unowned(
            UnownedCastArm::SiegeGeneralAndContainedTimers,
        ));
    }
    if before.type_facts.job_time <= 0 {
        return Err(CastWorkPlanError::InvalidJobTime(
            before.type_facts.job_time,
        ));
    }

    let mut after = before;
    after.actor.spell_time = after.actor.spell_time.wrapping_add(1);
    if after.actor.spell_time >= before.type_facts.job_time {
        return Err(CastWorkPlanError::Unowned(
            UnownedCastArm::CompletionAndSpellEffects,
        ));
    }
    Ok(CastWorkPlan {
        before,
        after,
        branch: CastWorkBranch::FishermenDeployWaiting,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CastActorRuntimeFacts {
    pub actor: Handle,
    pub who: u8,
    pub o: i16,
    pub uid: u16,
    pub type_index: i32,
    pub type_facts: CastTypeFacts,
}

/// External content/executable projection. It is intentionally absent from DoNSave: load
/// restores canonical actor/order bytes and remains fail closed until reinstallation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CastWorkAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub actors: Vec<CastActorRuntimeFacts>,
}

impl CastWorkAuthority {
    fn actor(&self, actor: Handle) -> Option<CastActorRuntimeFacts> {
        self.actors
            .iter()
            .copied()
            .find(|facts| facts.actor == actor)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CastWorkRuntimeError {
    RowOutOfRange,
    MissingAuthority,
    MissingCompositionDigest,
    DuplicateActorAuthority(Handle),
    ActorIdentityMismatch,
    UnitTypeProjectionMismatch,
    MissingCastOrder,
    MissingTypedPayload,
    OrderFieldOutOfRange,
    Plan(CastWorkPlanError),
    StaleState,
}

impl From<CastWorkPlanError> for CastWorkRuntimeError {
    fn from(error: CastWorkPlanError) -> Self {
        Self::Plan(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedCastWorkActivation {
    pub row: usize,
    pub actor_facts: CastActorRuntimeFacts,
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub plan: CastWorkPlan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CastWorkActivationReceipt {
    pub actor: Handle,
    pub authority_revision: u64,
    pub branch: CastWorkBranch,
    pub spell: i32,
    pub spell_time_before: i16,
    pub spell_time_after: i16,
    pub job_time: i16,
    pub order_unchanged: bool,
    pub rng_draws: u8,
    pub external_effects: u8,
}

fn work_order(order: &Order) -> Result<CastWorkOrder, CastWorkRuntimeError> {
    if order.kind != OrderIndex::CastSpell {
        return Err(CastWorkRuntimeError::MissingCastOrder);
    }
    let Some(EconomyOrderPayload::CastSpell(CastOrderPayload { paid, spell })) = order.economy
    else {
        return Err(CastWorkRuntimeError::MissingTypedPayload);
    };
    Ok(CastWorkOrder {
        node_metric: order.node_metric,
        flags: order.flags,
        target_o: i32::from(order.target_o),
        target_who: i32::from(order.target_who),
        target_uid: order.target_uid,
        x: order.x,
        y: order.y,
        paid,
        spell,
    })
}

fn validate_authority(authority: &CastWorkAuthority) -> Result<(), CastWorkRuntimeError> {
    if authority.composition_digest == [0; 32] {
        return Err(CastWorkRuntimeError::MissingCompositionDigest);
    }
    for (index, facts) in authority.actors.iter().enumerate() {
        if authority.actors[..index]
            .iter()
            .any(|old| old.actor == facts.actor)
        {
            return Err(CastWorkRuntimeError::DuplicateActorAuthority(facts.actor));
        }
    }
    Ok(())
}

fn canonical_snapshot(
    world: &World,
    unit_types: &[i32],
    authority: &CastWorkAuthority,
    row: usize,
) -> Result<(CastActorRuntimeFacts, CastWorkSnapshot), CastWorkRuntimeError> {
    validate_authority(authority)?;
    if row >= world.live_count() as usize {
        return Err(CastWorkRuntimeError::RowOutOfRange);
    }
    let actor = world
        .handle_at_row(row)
        .ok_or(CastWorkRuntimeError::RowOutOfRange)?;
    let facts = authority
        .actor(actor)
        .ok_or(CastWorkRuntimeError::MissingAuthority)?;
    if facts.who as usize >= OWNER_SLOTS
        || (facts.who, facts.o, facts.uid)
            != (
                world.units.get_who(row),
                world.units.o()[row],
                world.units.get_uid(row),
            )
    {
        return Err(CastWorkRuntimeError::ActorIdentityMismatch);
    }
    let type_index = unit_types
        .get(row)
        .copied()
        .ok_or(CastWorkRuntimeError::UnitTypeProjectionMismatch)?;
    if facts.type_index != type_index || world.unit_type_id(row) != Some(type_index) {
        return Err(CastWorkRuntimeError::UnitTypeProjectionMismatch);
    }
    let order = world
        .orders(row)
        .current()
        .ok_or(CastWorkRuntimeError::MissingCastOrder)?;
    let order = work_order(order)?;
    if order.target_o != StableTargetIdentity::NONE.o
        || order.target_who != StableTargetIdentity::NONE.who
    {
        return Err(CastWorkRuntimeError::OrderFieldOutOfRange);
    }
    Ok((
        facts,
        CastWorkSnapshot {
            authority_revision: authority.revision,
            actor: CastActorImage {
                who: world.units.get_who(row),
                o: world.units.o()[row],
                uid: world.units.get_uid(row),
                type_index,
                active: world.units.get_flags(row) & OBJ_FLAG_ACTIVE != 0,
                spell_time: world.units.spell_time()[row],
            },
            order,
            type_facts: facts.type_facts,
        },
    ))
}

/// Read and plan one production `Unit::work` CAST activation from canonical owners.
pub fn prepare_cast_work_activation(
    world: &World,
    unit_types: &[i32],
    authority: &CastWorkAuthority,
    row: usize,
) -> Result<PreparedCastWorkActivation, CastWorkRuntimeError> {
    let (actor_facts, before) = canonical_snapshot(world, unit_types, authority, row)?;
    let plan = plan_fresh_cast_tick(before)?;
    Ok(PreparedCastWorkActivation {
        row,
        actor_facts,
        authority_revision: authority.revision,
        authority_digest: authority.composition_digest,
        plan,
    })
}

/// Re-read the complete plan and publish the single exact Unit-column write. Every possible
/// failure precedes publication; the order, RNG, Guys, and external world remain untouched.
pub fn commit_cast_work_activation(
    world: &mut World,
    unit_types: &[i32],
    authority: &CastWorkAuthority,
    prepared: PreparedCastWorkActivation,
) -> Result<CastWorkActivationReceipt, CastWorkRuntimeError> {
    let current = prepare_cast_work_activation(world, unit_types, authority, prepared.row)?;
    if current != prepared {
        return Err(CastWorkRuntimeError::StaleState);
    }
    world.units.spell_time_mut()[prepared.row] = prepared.plan.after.actor.spell_time;
    Ok(CastWorkActivationReceipt {
        actor: prepared.actor_facts.actor,
        authority_revision: prepared.authority_revision,
        branch: prepared.plan.branch,
        spell: prepared.plan.before.order.spell,
        spell_time_before: prepared.plan.before.actor.spell_time,
        spell_time_after: prepared.plan.after.actor.spell_time,
        job_time: prepared.plan.before.type_facts.job_time,
        order_unchanged: prepared.plan.before.order == prepared.plan.after.order,
        rng_draws: 0,
        external_effects: 0,
    })
}
