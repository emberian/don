// SPDX-License-Identifier: GPL-3.0-or-later
//! Atomic production seam for the fresh retail Peasant `BUILD_AT` order.
//!
//! The fresh v16 SVX contains four `BUILD_AT` nodes. This module owns the substantive,
//! non-completing witness at Unit `(who=2,o=3)`, type 50 (`Peasants`), targeting the
//! started Village `(who=2,o=2007,uid=16)`, type 414. The target begins at 25,100/60,000
//! construction progress. Retail contributes 100 work, latches the helper, increments the
//! site's recharge counter, and retains the order. The selected same-animation path writes
//! the lead Guy's already-zero `hold_attack` byte and consumes no RNG.
//!
//! Every virtual/content result that is not a canonical `World`, `Order`, or `BuildData`
//! field is revision-bound installed authority. Target retirement, reswarming, Farms,
//! facing changes, decoys, under-fire/Korean modifiers, start/activation, completion, and
//! animation/RNG tails all fail closed before the first canonical write.

use crate::objects::{Band, OWNER_SLOTS};
use crate::order::{Order, OrderIndex, ORDER_GROUP};
use crate::systems::construction::{
    self, BuildOrderTarget, BuildOutcome, BuilderContribution, BuilderFinish, BuilderGate,
    BuilderGateReceipt, ChecksumEffects, ConstructionEffects, EffectReceipt, ObjectKey,
    SiteCheckReceipt, SiteLifecycleReceipt,
};
use crate::systems::construction_builder::{self, PreflightInput, PreflightPlan};
use crate::systems::production::{self, BuildData, ProdRules, BUILD_BAND_BASE};
use crate::world::{Handle, World, OBJ_FLAG_ACTIVE};

pub const FRESH_BUILD_AT_ACTOR_TYPE: i32 = 50;
pub const FRESH_BUILD_AT_TARGET_TYPE: i32 = 414;
pub const FRESH_BUILD_AT_ACTOR_WHO: u8 = 2;
pub const FRESH_BUILD_AT_ACTOR_O: i16 = 3;
pub const FRESH_BUILD_AT_ACTOR_UID: u16 = 9;
pub const FRESH_BUILD_AT_TARGET_WHO: u8 = 2;
pub const FRESH_BUILD_AT_TARGET_O: i16 = 2007;
pub const FRESH_BUILD_AT_TARGET_UID: u16 = 16;
pub const FRESH_BUILD_AT_FLAGS: u8 = ORDER_GROUP;
pub const FRESH_BUILD_AT_PAYLOAD_BYTES: usize = 11;

pub const FRESH_BUILD_FLAGS: u8 =
    production::flag::VALID | production::flag::STARTED | production::flag::CAPTURED;
pub const FRESH_BUILD_MASKS: u16 = production::mask::WORKED_LAST_FRAME;
pub const FRESH_BUILD_RECHARGING: i16 = 251;
pub const FRESH_BUILD_HELPERS: u8 = 0;
pub const FRESH_BUILD_JOB_COUNTER: u32 = 25_100;
pub const FRESH_BUILD_CONSTRUCT_TIME: u32 = 60_000;
pub const FRESH_BUILD_RATE: i32 = 100;
pub const FRESH_BUILD_AI_SPEED: i32 = 1;
pub const FRESH_BUILD_ANIMATION: u8 = 0x21;
pub const FRESH_BUILD_ANIMATION_CLASS: u8 = 0x21;
pub const FRESH_BUILD_ANIMATION_TIME: i32 = 12;
pub const FRESH_BUILD_ANIMATION_END: i32 = 14;
pub const FRESH_BUILD_AT_FRAME: i32 = 1_199;
pub const FRESH_BUILD_AT_ACTOR_X: i32 = 4_440;
pub const FRESH_BUILD_AT_ACTOR_Y: i32 = 24_648;
pub const FRESH_BUILD_AT_TARGET_X: i32 = 4_704;
pub const FRESH_BUILD_AT_TARGET_Y: i32 = 23_904;
pub const FRESH_BUILD_AT_ACTOR_ANGLE: i32 = 0x0dfa_0000;
pub const FRESH_BUILD_AT_ACTOR_MASKS: u32 = 0x0004_040a;

pub const UNIT_DO_BUILD_VA: u32 = 0x005e_ebf0;
pub const UNIT_DO_BUILD_BYTES: usize = 1_711;
pub const UNIT_DO_BUILD_SHA256: &str =
    "0ddd82c78a65cd13956f0d33b2ebdfaaa5942000339102b40f33b9a6d102587e";
pub const WALL_DO_CONSTRUCT_VA: u32 = 0x0064_34d0;
pub const WALL_DO_CONSTRUCT_BYTES: usize = 1_245;
pub const WALL_DO_CONSTRUCT_SHA256: &str =
    "b92b6180e910b076a4de2458f94ed2ccb530e6d0db292ae70eb201561fd59589";
pub const FRESH_BUILD_AT_PAYLOAD_SHA256: &str =
    "026217de3bb8dbbaafc4dbde5f641bedd55c3b4144ff9f0147ad270c46dcc895";
pub const FRESH_BUILD_AT_NODE_SHA256: &str =
    "2c727e3424927616773913b0cdc595882237ac2460a90546ba661c40a875ded2";
pub const FRESH_BUILD_AT_UNIT_SHA256: &str =
    "9c9c7ffc492a0b3da9cd1a549392f4306267cc73ea1fac883390e2ab7d38402c";
pub const FRESH_BUILD_AT_GUY_SHA256: &str =
    "9591baee4dd8bb895ff9f6e00305e5f83a55114a7e3184439d68de6f32fcc61f";
pub const FRESH_BUILD_AT_BUILD_SHA256: &str =
    "e919959a17ca603d5d901b75c55d4caa94df92d762caa96574bfe987c66684c3";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildAtWorkOrder {
    pub node_metric: u8,
    pub flags: u8,
    pub target_o: i32,
    pub target_who: i32,
    pub target_uid: u16,
}

impl BuildAtWorkOrder {
    pub const FRESH: Self = Self {
        node_metric: 0,
        flags: FRESH_BUILD_AT_FLAGS,
        target_o: FRESH_BUILD_AT_TARGET_O as i32,
        target_who: FRESH_BUILD_AT_TARGET_WHO as i32,
        target_uid: FRESH_BUILD_AT_TARGET_UID,
    };

    /// Exact `TargetOrder::walk_data` image, beginning with inherited `UnitOrder::flags`.
    pub fn retail_payload_image(self) -> [u8; FRESH_BUILD_AT_PAYLOAD_BYTES] {
        let mut out = [0; FRESH_BUILD_AT_PAYLOAD_BYTES];
        out[0] = self.flags;
        out[1..5].copy_from_slice(&self.target_o.to_le_bytes());
        out[5..9].copy_from_slice(&self.target_who.to_le_bytes());
        out[9..11].copy_from_slice(&self.target_uid.to_le_bytes());
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildAtActorImage {
    pub who: u8,
    pub o: i16,
    pub uid: u16,
    pub type_index: i32,
    pub active: bool,
    pub x: i32,
    pub y: i32,
    pub angle: i32,
    pub unit_masks: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildAtSiteImage {
    pub who: u8,
    pub o: i16,
    pub uid: u16,
    pub type_index: i32,
    pub flags: u8,
    pub x: i32,
    pub y: i32,
    pub build_masks: u16,
    pub recharging: i16,
    pub helpers: u8,
    pub job_counter: u32,
    pub job_counter_2: u32,
    pub construct_time: u32,
}

/// Installed virtual/content observations reached on the selected executable path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildAtBranchFacts {
    pub target_is_valid_wall: bool,
    pub adjacent: bool,
    pub builder_tile_is_covered: bool,
    pub target_is_farm: bool,
    pub korean_bonus: bool,
    pub ai_speed: i32,
    pub construct_rate: i32,
    pub construct_time_result: u32,
    pub guys_length: i32,
    pub guys_capacity: i32,
    pub guys_increment: i32,
    pub guys_flags: u8,
    pub slot_zero_present: bool,
    pub lead_animation: u8,
    pub lead_animation_class: u8,
    pub lead_animation_time: i32,
    pub lead_animation_end: i32,
    pub lead_hold_attack: u8,
}

impl BuildAtBranchFacts {
    pub const FRESH: Self = Self {
        target_is_valid_wall: true,
        adjacent: true,
        builder_tile_is_covered: false,
        target_is_farm: false,
        korean_bonus: false,
        ai_speed: FRESH_BUILD_AI_SPEED,
        construct_rate: FRESH_BUILD_RATE,
        construct_time_result: FRESH_BUILD_CONSTRUCT_TIME,
        guys_length: 1,
        guys_capacity: 1,
        guys_increment: 1,
        guys_flags: 0,
        slot_zero_present: true,
        lead_animation: FRESH_BUILD_ANIMATION,
        lead_animation_class: FRESH_BUILD_ANIMATION_CLASS,
        lead_animation_time: FRESH_BUILD_ANIMATION_TIME,
        lead_animation_end: FRESH_BUILD_ANIMATION_END,
        lead_hold_attack: 0,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildAtWorkSnapshot {
    pub authority_revision: u64,
    pub actor: BuildAtActorImage,
    pub order: BuildAtWorkOrder,
    pub site: BuildAtSiteImage,
    pub branch_facts: BuildAtBranchFacts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildAtWorkBranch {
    PeasantVillageNonCompleting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildAtWorkPlan {
    pub before: BuildAtWorkSnapshot,
    pub after: BuildAtWorkSnapshot,
    pub branch: BuildAtWorkBranch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnownedBuildAtArm {
    InvalidOrReusedTarget,
    UnstartedOrActiveTarget,
    Reswarm,
    Farm,
    Facing,
    Decoy,
    UnderFireOrKoreanRate,
    GuyAnimation,
    CompletionAndActivation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildAtWorkPlanError {
    FreshOrderMismatch,
    ActorIdentityMismatch,
    ActorTypeMismatch(i32),
    InactiveActor,
    TargetIdentityMismatch,
    TargetTypeMismatch(i32),
    FreshSiteMismatch,
    InvalidConstructFacts,
    Unowned(UnownedBuildAtArm),
}

/// Plan exactly the saved Peasant/Village contribution.
pub fn plan_fresh_build_at_tick(
    before: BuildAtWorkSnapshot,
) -> Result<BuildAtWorkPlan, BuildAtWorkPlanError> {
    if before.order != BuildAtWorkOrder::FRESH {
        return Err(BuildAtWorkPlanError::FreshOrderMismatch);
    }
    if (before.actor.who, before.actor.o, before.actor.uid)
        != (
            FRESH_BUILD_AT_ACTOR_WHO,
            FRESH_BUILD_AT_ACTOR_O,
            FRESH_BUILD_AT_ACTOR_UID,
        )
    {
        return Err(BuildAtWorkPlanError::ActorIdentityMismatch);
    }
    if before.actor.type_index != FRESH_BUILD_AT_ACTOR_TYPE {
        return Err(BuildAtWorkPlanError::ActorTypeMismatch(
            before.actor.type_index,
        ));
    }
    if !before.actor.active {
        return Err(BuildAtWorkPlanError::InactiveActor);
    }
    if (before.site.who, before.site.o, before.site.uid)
        != (
            FRESH_BUILD_AT_TARGET_WHO,
            FRESH_BUILD_AT_TARGET_O,
            FRESH_BUILD_AT_TARGET_UID,
        )
    {
        return Err(BuildAtWorkPlanError::TargetIdentityMismatch);
    }
    if before.site.type_index != FRESH_BUILD_AT_TARGET_TYPE {
        return Err(BuildAtWorkPlanError::TargetTypeMismatch(
            before.site.type_index,
        ));
    }
    if !before.branch_facts.target_is_valid_wall {
        return Err(BuildAtWorkPlanError::Unowned(
            UnownedBuildAtArm::InvalidOrReusedTarget,
        ));
    }
    if before.site.flags != FRESH_BUILD_FLAGS {
        return Err(BuildAtWorkPlanError::Unowned(
            UnownedBuildAtArm::UnstartedOrActiveTarget,
        ));
    }
    if !before.branch_facts.adjacent || before.branch_facts.builder_tile_is_covered {
        return Err(BuildAtWorkPlanError::Unowned(UnownedBuildAtArm::Reswarm));
    }
    if before.branch_facts.target_is_farm {
        return Err(BuildAtWorkPlanError::Unowned(UnownedBuildAtArm::Farm));
    }
    let expected_angle = crate::trig::find_angle(
        before.site.x.wrapping_sub(before.actor.x),
        before.site.y.wrapping_sub(before.actor.y),
    );
    if before.actor.angle != expected_angle {
        return Err(BuildAtWorkPlanError::Unowned(UnownedBuildAtArm::Facing));
    }
    if before.actor.unit_masks & 1 != 0 {
        return Err(BuildAtWorkPlanError::Unowned(UnownedBuildAtArm::Decoy));
    }
    if before.site.build_masks != FRESH_BUILD_MASKS
        || before.site.recharging != FRESH_BUILD_RECHARGING
        || before.site.helpers != FRESH_BUILD_HELPERS
        || before.site.job_counter != FRESH_BUILD_JOB_COUNTER
        || before.site.job_counter_2 != FRESH_BUILD_JOB_COUNTER
        || before.site.construct_time != FRESH_BUILD_CONSTRUCT_TIME
    {
        return Err(BuildAtWorkPlanError::FreshSiteMismatch);
    }
    if before.branch_facts.korean_bonus
        || before.branch_facts.ai_speed != FRESH_BUILD_AI_SPEED
        || before.branch_facts.construct_rate != FRESH_BUILD_RATE
        || before.branch_facts.construct_time_result != FRESH_BUILD_CONSTRUCT_TIME
    {
        return Err(BuildAtWorkPlanError::Unowned(
            UnownedBuildAtArm::UnderFireOrKoreanRate,
        ));
    }
    if before.branch_facts != BuildAtBranchFacts::FRESH {
        return Err(BuildAtWorkPlanError::Unowned(
            UnownedBuildAtArm::GuyAnimation,
        ));
    }

    let mut after = before;
    after.site.recharging = after.site.recharging.wrapping_add(1);
    after.site.build_masks |= production::mask::HELPER_COUNTED;
    let step = production::do_construct(
        before.branch_facts.construct_rate,
        before.branch_facts.ai_speed,
        false,
        before.site.job_counter,
        before.site.job_counter_2,
        before.site.helpers,
        before.branch_facts.construct_time_result,
    );
    if step.completed {
        return Err(BuildAtWorkPlanError::Unowned(
            UnownedBuildAtArm::CompletionAndActivation,
        ));
    }
    after.site.helpers = step.helpers;
    after.site.job_counter = step.job_counter;
    after.site.job_counter_2 = step.job_counter_2;
    Ok(BuildAtWorkPlan {
        before,
        after,
        branch: BuildAtWorkBranch::PeasantVillageNonCompleting,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildAtActorRuntimeFacts {
    pub actor: Handle,
    pub who: u8,
    pub o: i16,
    pub uid: u16,
    pub type_index: i32,
    pub branch: BuildAtBranchFacts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildAtSiteRuntimeFacts {
    pub who: u8,
    pub o: i16,
    pub uid: u16,
    pub type_index: i32,
}

/// Reinstalled executable/content projection. It is deliberately absent from DoNSave.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BuildAtWorkAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub actors: Vec<BuildAtActorRuntimeFacts>,
    pub sites: Vec<BuildAtSiteRuntimeFacts>,
}

impl BuildAtWorkAuthority {
    pub fn is_installed(&self) -> bool {
        self.composition_digest != [0; 32]
    }

    fn actor(&self, actor: Handle) -> Option<BuildAtActorRuntimeFacts> {
        self.actors
            .iter()
            .copied()
            .find(|facts| facts.actor == actor)
    }

    fn site(&self, who: u8, o: i16, uid: u16) -> Option<BuildAtSiteRuntimeFacts> {
        self.sites
            .iter()
            .copied()
            .find(|facts| (facts.who, facts.o, facts.uid) == (who, o, uid))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildAtWorkRuntimeError {
    RowOutOfRange,
    MissingAuthority,
    MissingCompositionDigest,
    DuplicateActorAuthority(Handle),
    DuplicateSiteAuthority { who: u8, o: i16, uid: u16 },
    ActorIdentityMismatch,
    UnitTypeProjectionMismatch,
    InvalidGuyArrayFacts,
    MissingBuildAtOrder,
    UnexpectedOrderPayload,
    TargetOutOfRange,
    MissingBuildTarget,
    BuildIdentityMismatch,
    SiteAuthorityMismatch,
    ContentProjectionMismatch,
    ConstructionCoreMismatch,
    Plan(BuildAtWorkPlanError),
    StaleState,
}

impl From<BuildAtWorkPlanError> for BuildAtWorkRuntimeError {
    fn from(error: BuildAtWorkPlanError) -> Self {
        Self::Plan(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UnreachedBuildEffect {
    Unreachable,
}

struct SavedBuildEffects {
    builder: ObjectKey,
    target: ObjectKey,
}

impl ConstructionEffects for SavedBuildEffects {
    type Error = UnreachedBuildEffect;

    fn builder_gate(
        &mut self,
        builder: ObjectKey,
        target: ObjectKey,
    ) -> Result<BuilderGateReceipt, Self::Error> {
        if (builder, target) != (self.builder, self.target) {
            return Err(UnreachedBuildEffect::Unreachable);
        }
        Ok(BuilderGateReceipt {
            gate: BuilderGate::Ready,
            effect: EffectReceipt {
                rng_draws: 0,
                checksums: ChecksumEffects::NONE,
            },
        })
    }

    fn blocked_site(
        &mut self,
        _site_key: ObjectKey,
        _site: &BuildData,
    ) -> Result<SiteCheckReceipt, Self::Error> {
        Err(UnreachedBuildEffect::Unreachable)
    }

    fn start_site(
        &mut self,
        _site_key: ObjectKey,
        _site: &mut BuildData,
        _notify: i32,
    ) -> Result<SiteLifecycleReceipt, Self::Error> {
        Err(UnreachedBuildEffect::Unreachable)
    }

    fn disband_site(
        &mut self,
        _site_key: ObjectKey,
        _site: &mut BuildData,
        _mode: i32,
    ) -> Result<SiteLifecycleReceipt, Self::Error> {
        Err(UnreachedBuildEffect::Unreachable)
    }

    fn activate_site(
        &mut self,
        _site_key: ObjectKey,
        _site: &mut BuildData,
        _arg0: i32,
        _arg1: i32,
        _arg2: i32,
    ) -> Result<SiteLifecycleReceipt, Self::Error> {
        Err(UnreachedBuildEffect::Unreachable)
    }

    fn finish_builder(
        &mut self,
        _builder: ObjectKey,
        _target: ObjectKey,
        _reason: BuilderFinish,
    ) -> Result<EffectReceipt, Self::Error> {
        Err(UnreachedBuildEffect::Unreachable)
    }

    fn interrupt_builder(
        &mut self,
        _builder: ObjectKey,
        _target: ObjectKey,
        _reason: BuilderFinish,
    ) -> Result<EffectReceipt, Self::Error> {
        Err(UnreachedBuildEffect::Unreachable)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedBuildAtWorkActivation {
    pub row: usize,
    pub build_row: usize,
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub actor_facts: BuildAtActorRuntimeFacts,
    pub site_facts: BuildAtSiteRuntimeFacts,
    pub plan: BuildAtWorkPlan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildAtWorkActivationReceipt {
    pub actor: Handle,
    pub site_who: u8,
    pub site_o: i16,
    pub site_uid: u16,
    pub authority_revision: u64,
    pub branch: BuildAtWorkBranch,
    pub progress_before: u32,
    pub progress_after: u32,
    pub recharge_before: i16,
    pub recharge_after: i16,
    pub helper_latch_set: bool,
    pub order_unchanged: bool,
    pub guy_hold_attack_zero_write: bool,
    pub rng_draws: u8,
    pub external_effects: u8,
}

fn work_order(order: &Order) -> Result<BuildAtWorkOrder, BuildAtWorkRuntimeError> {
    if order.kind != OrderIndex::BuildAt {
        return Err(BuildAtWorkRuntimeError::MissingBuildAtOrder);
    }
    if order.move_state.is_some()
        || order.follow.is_some()
        || order.special_anim.is_some()
        || order.form_order.is_some()
        || order.air_patrol.is_some()
        || order.strafe.is_some()
        || order.economy.is_some()
    {
        return Err(BuildAtWorkRuntimeError::UnexpectedOrderPayload);
    }
    Ok(BuildAtWorkOrder {
        node_metric: order.node_metric,
        flags: order.flags,
        target_o: i32::from(order.target_o),
        target_who: i32::from(order.target_who),
        target_uid: order.target_uid,
    })
}

fn validate_authority(authority: &BuildAtWorkAuthority) -> Result<(), BuildAtWorkRuntimeError> {
    if authority.composition_digest == [0; 32] {
        return Err(BuildAtWorkRuntimeError::MissingCompositionDigest);
    }
    for (index, facts) in authority.actors.iter().enumerate() {
        if authority.actors[..index]
            .iter()
            .any(|old| old.actor == facts.actor)
        {
            return Err(BuildAtWorkRuntimeError::DuplicateActorAuthority(
                facts.actor,
            ));
        }
    }
    for (index, facts) in authority.sites.iter().enumerate() {
        if authority.sites[..index]
            .iter()
            .any(|old| (old.who, old.o, old.uid) == (facts.who, facts.o, facts.uid))
        {
            return Err(BuildAtWorkRuntimeError::DuplicateSiteAuthority {
                who: facts.who,
                o: facts.o,
                uid: facts.uid,
            });
        }
    }
    Ok(())
}

fn build_row(
    world: &World,
    builds: &[BuildData],
    who: i32,
    o: i32,
) -> Result<usize, BuildAtWorkRuntimeError> {
    if !(0..OWNER_SLOTS as i32).contains(&who)
        || !(BUILD_BAND_BASE as i32..=i16::MAX as i32).contains(&o)
    {
        return Err(BuildAtWorkRuntimeError::TargetOutOfRange);
    }
    let offset = (o - BUILD_BAND_BASE as i32) as usize;
    let row = *world
        .objects
        .slot(who as usize)
        .band(Band::Build)
        .get(offset)
        .ok_or(BuildAtWorkRuntimeError::MissingBuildTarget)? as usize;
    let build = builds
        .get(row)
        .ok_or(BuildAtWorkRuntimeError::MissingBuildTarget)?;
    if (i32::from(build.who), i32::from(build.object_id())) != (who, o) {
        return Err(BuildAtWorkRuntimeError::BuildIdentityMismatch);
    }
    Ok(row)
}

/// Read and plan one exact production `Unit::work` BUILD_AT activation.
pub fn prepare_build_at_work_activation(
    world: &World,
    builds: &[BuildData],
    unit_types: &[i32],
    build_types: &[Option<i32>],
    ai_speed: i32,
    korean_bonus: bool,
    prod_rules: &ProdRules,
    authority: &BuildAtWorkAuthority,
    row: usize,
) -> Result<PreparedBuildAtWorkActivation, BuildAtWorkRuntimeError> {
    validate_authority(authority)?;
    if row >= world.live_count() as usize {
        return Err(BuildAtWorkRuntimeError::RowOutOfRange);
    }
    let actor = world
        .handle_at_row(row)
        .ok_or(BuildAtWorkRuntimeError::RowOutOfRange)?;
    let actor_facts = authority
        .actor(actor)
        .ok_or(BuildAtWorkRuntimeError::MissingAuthority)?;
    if (actor_facts.who, actor_facts.o, actor_facts.uid)
        != (
            world.units.get_who(row),
            world.units.o()[row],
            world.units.get_uid(row),
        )
    {
        return Err(BuildAtWorkRuntimeError::ActorIdentityMismatch);
    }
    let type_index = unit_types
        .get(row)
        .copied()
        .ok_or(BuildAtWorkRuntimeError::UnitTypeProjectionMismatch)?;
    if actor_facts.type_index != type_index || world.unit_type_id(row) != Some(type_index) {
        return Err(BuildAtWorkRuntimeError::UnitTypeProjectionMismatch);
    }
    let branch = actor_facts.branch;
    if (
        branch.guys_length,
        branch.guys_capacity,
        branch.guys_increment,
    ) != (1, 1, 1)
        || branch.guys_flags != 0
        || !branch.slot_zero_present
        || i32::from(world.units.guy_mark()[row]) != branch.guys_length
    {
        return Err(BuildAtWorkRuntimeError::InvalidGuyArrayFacts);
    }
    let order = work_order(
        world
            .orders(row)
            .current()
            .ok_or(BuildAtWorkRuntimeError::MissingBuildAtOrder)?,
    )?;
    let build_row = build_row(world, builds, order.target_who, order.target_o)?;
    let build = &builds[build_row];
    if build.uid != order.target_uid {
        return Err(BuildAtWorkRuntimeError::BuildIdentityMismatch);
    }
    let site_facts = authority
        .site(build.who, build.object_id(), build.uid)
        .ok_or(BuildAtWorkRuntimeError::MissingAuthority)?;
    if site_facts.type_index != build.orig_type {
        return Err(BuildAtWorkRuntimeError::SiteAuthorityMismatch);
    }
    if build_types.get(build_row).copied().flatten() != Some(site_facts.type_index) {
        return Err(BuildAtWorkRuntimeError::SiteAuthorityMismatch);
    }
    if ai_speed != branch.ai_speed || korean_bonus != branch.korean_bonus {
        return Err(BuildAtWorkRuntimeError::ContentProjectionMismatch);
    }
    if production::construct_rate(build.is_under_attack(), branch.korean_bonus, prod_rules)
        != branch.construct_rate
        || production::construct_time(
            build.constr_time,
            false,
            &production::ConstructQueryGates::default(),
            prod_rules,
        ) != branch.construct_time_result
    {
        return Err(BuildAtWorkRuntimeError::ContentProjectionMismatch);
    }
    let (site_x, site_y) = build.position();
    let before = BuildAtWorkSnapshot {
        authority_revision: authority.revision,
        actor: BuildAtActorImage {
            who: world.units.get_who(row),
            o: world.units.o()[row],
            uid: world.units.get_uid(row),
            type_index,
            active: world.units.get_flags(row) & OBJ_FLAG_ACTIVE != 0,
            x: world.units.x_internal()[row],
            y: world.units.y_internal()[row],
            angle: world.units.angle()[row],
            unit_masks: world.units.get_unit_masks(row),
        },
        order,
        site: BuildAtSiteImage {
            who: build.who,
            o: build.object_id(),
            uid: build.uid,
            type_index: build.orig_type,
            flags: build.flags,
            x: site_x,
            y: site_y,
            build_masks: build.build_masks,
            recharging: build.recharging,
            helpers: build.helpers,
            job_counter: build.job_counter,
            job_counter_2: build.job_counter_2,
            construct_time: build.constr_time,
        },
        branch_facts: branch,
    };
    let builder_key = ObjectKey {
        who: i32::from(before.actor.who),
        o: i32::from(before.actor.o),
        uid: before.actor.uid,
    };
    let target_key = ObjectKey {
        who: i32::from(before.site.who),
        o: i32::from(before.site.o),
        uid: before.site.uid,
    };
    let plan = plan_fresh_build_at_tick(before)?;
    let preflight = construction_builder::preflight(PreflightInput {
        builder: builder_key,
        target_order: target_key,
        target_is_valid_wall: branch.target_is_valid_wall,
        target_is_active: build.is_active(),
        has_next_action_after_retire: false,
        adjacent: branch.adjacent,
        builder_tile_is_covered: branch.builder_tile_is_covered,
        target_is_farm: branch.target_is_farm,
        order_flags: order.flags,
        builder_x: before.actor.x,
        builder_y: before.actor.y,
        target_x: before.site.x,
        target_y: before.site.y,
        builder_angle: before.actor.angle,
        unit_decoy: before.actor.unit_masks & 1 != 0,
    });
    if preflight
        != (PreflightPlan::AnimateFace {
            animation: construction_builder::CHAR_BUILD,
            set_angle: None,
            contribute: true,
        })
    {
        return Err(BuildAtWorkRuntimeError::ConstructionCoreMismatch);
    }
    let mut staged = build.clone();
    let receipt = construction::execute_builder(
        &mut staged,
        target_key,
        builder_key,
        BuildOrderTarget::new(target_key, order.flags & ORDER_GROUP != 0),
        BuilderContribution {
            ai_speed,
            korean_build_under_fire_bonus: korean_bonus,
            construct_query: production::ConstructQueryGates::default(),
        },
        prod_rules,
        &mut SavedBuildEffects {
            builder: builder_key,
            target: target_key,
        },
    )
    .map_err(|_| BuildAtWorkRuntimeError::ConstructionCoreMismatch)?;
    if receipt.outcome
        != (BuildOutcome::Progressed {
            credited: FRESH_BUILD_RATE as u32,
            started_this_call: false,
        })
        || receipt.rng_draws != 0
        || !receipt.checksums.builds
        || (
            staged.build_masks,
            staged.recharging,
            staged.helpers,
            staged.job_counter,
            staged.job_counter_2,
        ) != (
            plan.after.site.build_masks,
            plan.after.site.recharging,
            plan.after.site.helpers,
            plan.after.site.job_counter,
            plan.after.site.job_counter_2,
        )
    {
        return Err(BuildAtWorkRuntimeError::ConstructionCoreMismatch);
    }
    Ok(PreparedBuildAtWorkActivation {
        row,
        build_row,
        authority_revision: authority.revision,
        authority_digest: authority.composition_digest,
        actor_facts,
        site_facts,
        plan,
    })
}

/// Re-read the whole plan, then publish only the five saved BuildData field changes.
pub fn commit_build_at_work_activation(
    world: &World,
    builds: &mut [BuildData],
    unit_types: &[i32],
    build_types: &[Option<i32>],
    ai_speed: i32,
    korean_bonus: bool,
    prod_rules: &ProdRules,
    authority: &BuildAtWorkAuthority,
    prepared: PreparedBuildAtWorkActivation,
) -> Result<BuildAtWorkActivationReceipt, BuildAtWorkRuntimeError> {
    let current = prepare_build_at_work_activation(
        world,
        builds,
        unit_types,
        build_types,
        ai_speed,
        korean_bonus,
        prod_rules,
        authority,
        prepared.row,
    )?;
    if current != prepared {
        return Err(BuildAtWorkRuntimeError::StaleState);
    }
    let before = prepared.plan.before;
    let after = prepared.plan.after;
    let build = &mut builds[prepared.build_row];
    build.build_masks = after.site.build_masks;
    build.recharging = after.site.recharging;
    build.helpers = after.site.helpers;
    build.job_counter = after.site.job_counter;
    build.job_counter_2 = after.site.job_counter_2;
    Ok(BuildAtWorkActivationReceipt {
        actor: prepared.actor_facts.actor,
        site_who: prepared.site_facts.who,
        site_o: prepared.site_facts.o,
        site_uid: prepared.site_facts.uid,
        authority_revision: prepared.authority_revision,
        branch: prepared.plan.branch,
        progress_before: before.site.job_counter,
        progress_after: after.site.job_counter,
        recharge_before: before.site.recharging,
        recharge_after: after.site.recharging,
        helper_latch_set: after.site.build_masks & production::mask::HELPER_COUNTED != 0,
        order_unchanged: before.order == after.order,
        guy_hold_attack_zero_write: true,
        rng_draws: 0,
        external_effects: 0,
    })
}
