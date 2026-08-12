// SPDX-License-Identifier: GPL-3.0-or-later
//! Atomic saved-order seam for the fresh retail `Unit::do_gather` cohort.
//!
//! The fresh v16 SVX contains 29 exact `GatherOrder` images: 17 active Farm orders and
//! 12 active Camp/Woodcutter orders.  This module binds both shapes without pretending
//! that all of `Unit::do_gather` is available.  It executes one substantive branch shared
//! by all 12 Camp images: a non-capacity phase, `goto_build == 0`, lead animation `0x19`,
//! and the animation-`0x19` wait loop, including its exact wait-zero
//! `Build::all_gathering`/single-RNG-draw tail.
//!
//! Planning is read-only.  Publishing is one compare/exchange host call over the actor,
//! order, target building, RNG state, and authority revision.  Farm, Mine, destination
//! selection, capacity, direct-resource, cast/special, and retirement branches fail closed
//! before that call.  There is no geometric, resource-rate, or scalar approximation.

use crate::objects::{Band, BUILD_BAND_BASE, OWNER_SLOTS};
use crate::order::{Order, OrderIndex};
use crate::systems::economy_order_payload_authority::EconomyOrderPayload;
use crate::systems::gathering::{self, GatherAssignment, GatherSite, GatherWorker};
use crate::systems::production::BuildData;
use crate::world::{Handle, World};

/// `ObjectTypeData::property` values stored in `GatherOrder::build_type`.
pub const FARM_PROPERTY: i32 = 0x1a1;
pub const CAMP_PROPERTY: i32 = 0x1a2;
pub const MINE_PROPERTY: i32 = 0x1a3;

/// Lead `GuyData` animation read by the supported `do_non_flat_gather` timer arm.
pub const GATHER_ANIMATION_19: u8 = 0x19;
/// Unit action bits cleared at `0x005F0209` when `goto_build == 0`.
pub const GATHER_ACTION_MASKS: u32 = 0x7800_0000;
/// Building latch tested/set at the heads of `do_gather` and `do_non_flat_gather`.
pub const GATHER_SITE_LATCH: u16 = 0x0800;
/// The Camp/Mine capacity branch runs only on this 128-frame phase.
pub const CAPACITY_PHASE_MASK: i32 = 0x7f;

/// Complete scalar image of one retail Gather node after `OrderIndex` and node metric.
///
/// [`Self::retail_payload_image`] is the exact 31-byte `GatherOrder::walk_data` payload:
/// inherited flags, ten-byte TargetOrder identity, then the twenty-byte Gather suffix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherWorkOrder {
    pub node_metric: u8,
    pub flags: u8,
    pub target_o: i32,
    pub target_who: i32,
    pub target_uid: u16,
    pub tx: i32,
    pub ty: i32,
    pub build_type: i32,
    pub wait: i32,
    pub goto_build: u8,
    pub non_flat_gather: u8,
    pub dist_mod: u8,
    pub been_there: u8,
}

impl GatherWorkOrder {
    pub const RETAIL_PAYLOAD_BYTES: usize = 31;

    pub fn retail_payload_image(self) -> [u8; Self::RETAIL_PAYLOAD_BYTES] {
        let mut out = [0; Self::RETAIL_PAYLOAD_BYTES];
        out[0] = self.flags;
        out[1..5].copy_from_slice(&self.target_o.to_le_bytes());
        out[5..9].copy_from_slice(&self.target_who.to_le_bytes());
        out[9..11].copy_from_slice(&self.target_uid.to_le_bytes());
        out[11..15].copy_from_slice(&self.tx.to_le_bytes());
        out[15..19].copy_from_slice(&self.ty.to_le_bytes());
        out[19..23].copy_from_slice(&self.build_type.to_le_bytes());
        out[23..27].copy_from_slice(&self.wait.to_le_bytes());
        out[27] = self.goto_build;
        out[28] = self.non_flat_gather;
        out[29] = self.dist_mod;
        out[30] = self.been_there;
        out
    }

    pub fn from_retail_payload(node_metric: u8, bytes: [u8; Self::RETAIL_PAYLOAD_BYTES]) -> Self {
        let i32_at = |at| i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        Self {
            node_metric,
            flags: bytes[0],
            target_o: i32_at(1),
            target_who: i32_at(5),
            target_uid: u16::from_le_bytes(bytes[9..11].try_into().unwrap()),
            tx: i32_at(11),
            ty: i32_at(15),
            build_type: i32_at(19),
            wait: i32_at(23),
            goto_build: bytes[27],
            non_flat_gather: bytes[28],
            dist_mod: bytes[29],
            been_there: bytes[30],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherActorImage {
    pub who: u8,
    pub o: i16,
    pub uid: u16,
    pub group: i16,
    pub unit_masks: u32,
    /// `GuyData::animation` for a proved live slot-zero Guys-list body. Retail dereferences
    /// this pointer without a length/null guard, so absence is a mandatory failed gate.
    pub lead_animation: Option<u8>,
}

/// Exact building and virtual-gate facts consumed before the supported local timer arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherSiteImage {
    pub who: i32,
    pub o: i32,
    pub uid: u16,
    /// The owner-array target resolved through the Build/Wall virtual projection.
    pub resolved_build: bool,
    /// Result of the returned WallData virtual `is_valid_wall` gate.
    pub valid_wall: bool,
    /// Result of the returned WallData virtual `is_active` gate.
    pub active: bool,
    pub property: i32,
    pub build_masks: u16,
    pub recharging: i16,
}

/// One coherent before/after image for the saved order tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherWorkSnapshot {
    pub revision: u64,
    pub frame: i32,
    pub rng_state: i32,
    pub actor: GatherActorImage,
    pub order: GatherWorkOrder,
    pub site: GatherSiteImage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreshGatherPayloadClass {
    FarmActive,
    CampActiveTimer,
}

/// Why an image is not one of the two exact fresh-SVX Gather shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreshGatherBindingError {
    NonzeroNodeMetric(u8),
    NonzeroFlags(u8),
    ForeignTargetOwner { actor: u8, target: i32 },
    TargetOutsideBuildBand(i32),
    FarmStateMismatch,
    CampStateMismatch,
    UnsupportedBuildType(i32),
}

/// Bind the complete image rather than choosing a branch from `build_type` alone.
///
/// These predicates are true for all 29 fresh-save witnesses and intentionally reject a
/// partial or normalized payload.  They are an admission profile, not a claim that other
/// retail Gather states do not exist.
pub fn bind_fresh_gather_payload(
    actor_who: u8,
    order: GatherWorkOrder,
) -> Result<FreshGatherPayloadClass, FreshGatherBindingError> {
    if order.node_metric != 0 {
        return Err(FreshGatherBindingError::NonzeroNodeMetric(
            order.node_metric,
        ));
    }
    if order.flags != 0 {
        return Err(FreshGatherBindingError::NonzeroFlags(order.flags));
    }
    if order.target_who != i32::from(actor_who) {
        return Err(FreshGatherBindingError::ForeignTargetOwner {
            actor: actor_who,
            target: order.target_who,
        });
    }
    if !(2_000..=i16::MAX as i32).contains(&order.target_o) {
        return Err(FreshGatherBindingError::TargetOutsideBuildBand(
            order.target_o,
        ));
    }
    match order.build_type {
        FARM_PROPERTY => {
            if (order.tx, order.ty, order.wait) != (-1, -1, 0)
                || (
                    order.goto_build,
                    order.non_flat_gather,
                    order.dist_mod,
                    order.been_there,
                ) != (1, 0, 0, 1)
            {
                return Err(FreshGatherBindingError::FarmStateMismatch);
            }
            Ok(FreshGatherPayloadClass::FarmActive)
        }
        CAMP_PROPERTY => {
            if order.tx < 0
                || order.ty < 0
                || order.wait <= 0
                || (
                    order.goto_build,
                    order.non_flat_gather,
                    order.dist_mod,
                    order.been_there,
                ) != (0, 1, 4, 1)
            {
                return Err(FreshGatherBindingError::CampStateMismatch);
            }
            Ok(FreshGatherPayloadClass::CampActiveTimer)
        }
        MINE_PROPERTY => Err(FreshGatherBindingError::UnsupportedBuildType(MINE_PROPERTY)),
        other => Err(FreshGatherBindingError::UnsupportedBuildType(other)),
    }
}

/// Recovered Gather branches whose external owners are not mounted by this transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum UnownedGatherArm {
    TargetRetirementAndReplacement,
    CapacityAndGathererCount,
    FarmWorldAndAnimation,
    MineWorldAndMiningList,
    NonFlatDestinationAndCollision,
    AllGatheringAndRng,
    DirectResourceNode,
    CastOrSpecialGather,
    MissingLeadGuy,
    LeadAnimation(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatherWorkPlanError {
    Binding(FreshGatherBindingError),
    TargetIdentityMismatch,
    TargetPropertyMismatch { order: i32, site: i32 },
    Unowned(UnownedGatherArm),
}

impl From<FreshGatherBindingError> for GatherWorkPlanError {
    fn from(error: FreshGatherBindingError) -> Self {
        Self::Binding(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatherWorkBranch {
    CampAnimation19Waiting,
    CampAnimation19AllGathering,
    CampAnimation19Rescheduled,
}

/// Whole-owner plan.  `after` differs only at exact writes in the selected PE branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherWorkPlan {
    pub before: GatherWorkSnapshot,
    pub after: GatherWorkSnapshot,
    pub branch: GatherWorkBranch,
    pub changed_fields: u8,
    pub rng_draws: u8,
    pub external_effects: u8,
}

fn changed_fields(before: &GatherWorkSnapshot, after: &GatherWorkSnapshot) -> u8 {
    [
        before.actor.group != after.actor.group,
        before.actor.unit_masks != after.actor.unit_masks,
        before.order.wait != after.order.wait,
        before.site.build_masks != after.site.build_masks,
        before.site.recharging != after.site.recharging,
        before.rng_state != after.rng_state,
    ]
    .into_iter()
    .map(u8::from)
    .sum()
}

/// Plan the exact no-RNG Camp timer tick reached by the fresh saved payloads.
pub fn plan_fresh_gather_tick(
    before: GatherWorkSnapshot,
) -> Result<GatherWorkPlan, GatherWorkPlanError> {
    plan_fresh_gather_tick_inner(before, None)
}

/// Plan the same exact Camp timer arm when the decrement reaches zero.
///
/// `all_gathering` must be the result of the structurally executed
/// `Build::check_gatherers` + `Build::all_gathering` chain walk. It is deliberately not an
/// arbitrary policy bit at the production boundary: [`prepare_gather_work_activation`]
/// derives it from canonical Build/Unit/order owners and records every chain unlink in the
/// transaction.
pub fn plan_fresh_gather_tick_at_wait_zero(
    before: GatherWorkSnapshot,
    all_gathering: bool,
) -> Result<GatherWorkPlan, GatherWorkPlanError> {
    plan_fresh_gather_tick_inner(before, Some(all_gathering))
}

fn plan_fresh_gather_tick_inner(
    before: GatherWorkSnapshot,
    all_gathering: Option<bool>,
) -> Result<GatherWorkPlan, GatherWorkPlanError> {
    let class = bind_fresh_gather_payload(before.actor.who, before.order)?;
    if class == FreshGatherPayloadClass::FarmActive {
        return Err(GatherWorkPlanError::Unowned(
            UnownedGatherArm::FarmWorldAndAnimation,
        ));
    }
    if (before.site.who, before.site.o, before.site.uid)
        != (
            before.order.target_who,
            before.order.target_o,
            before.order.target_uid,
        )
    {
        return Err(GatherWorkPlanError::TargetIdentityMismatch);
    }
    if !before.site.resolved_build || !before.site.valid_wall || !before.site.active {
        return Err(GatherWorkPlanError::Unowned(
            UnownedGatherArm::TargetRetirementAndReplacement,
        ));
    }
    if before.site.property != before.order.build_type {
        return Err(GatherWorkPlanError::TargetPropertyMismatch {
            order: before.order.build_type,
            site: before.site.property,
        });
    }
    if before
        .frame
        .wrapping_add(i32::from(before.actor.o).wrapping_mul(4))
        & CAPACITY_PHASE_MASK
        == 0
    {
        return Err(GatherWorkPlanError::Unowned(
            UnownedGatherArm::CapacityAndGathererCount,
        ));
    }
    let Some(lead_animation) = before.actor.lead_animation else {
        return Err(GatherWorkPlanError::Unowned(
            UnownedGatherArm::MissingLeadGuy,
        ));
    };
    if lead_animation != GATHER_ANIMATION_19 {
        return Err(GatherWorkPlanError::Unowned(
            UnownedGatherArm::LeadAnimation(lead_animation),
        ));
    }
    if before.order.wait < 1 || (before.order.wait == 1 && all_gathering.is_none()) {
        return Err(GatherWorkPlanError::Unowned(
            UnownedGatherArm::AllGatheringAndRng,
        ));
    }

    let mut after = before;
    after.actor.group = -1;
    after.actor.unit_masks &= !GATHER_ACTION_MASKS;
    if after.site.build_masks & GATHER_SITE_LATCH == 0 {
        after.site.recharging = after.site.recharging.wrapping_add(1);
        after.site.build_masks |= GATHER_SITE_LATCH;
    }
    after.order.wait = after.order.wait.wrapping_sub(1);
    let (branch, rng_draws) = if after.order.wait != 0 {
        (GatherWorkBranch::CampAnimation19Waiting, 0)
    } else if all_gathering.expect("wait-one caller supplied the structural result") {
        after.order.wait = -1;
        (GatherWorkBranch::CampAnimation19AllGathering, 0)
    } else {
        // `Random::get(0, 0xffff)`, exactly: one LCG step, low sixteen bits, half-open
        // scaling, then the animation-0x19 `% 100 + 300` reschedule.
        after.rng_state = after
            .rng_state
            .wrapping_mul(0x0019_660d)
            .wrapping_add(0x3c6e_f35f);
        let low = (after.rng_state as u32 & 0xffff) as i32;
        let draw = ((low.wrapping_mul(0xffff) as u32) >> 16) as i32;
        after.order.wait = draw % 100 + 300;
        (GatherWorkBranch::CampAnimation19Rescheduled, 1)
    };

    Ok(GatherWorkPlan {
        before,
        after,
        branch,
        changed_fields: changed_fields(&before, &after),
        rng_draws,
        external_effects: 0,
    })
}

/// Revision-bound facts for the slot-zero `GuyData` body which is not represented in the
/// generated Unit columns.  The fresh SVX witnesses all have the exact `(1,1,1,0)` array
/// header pinned here; a normalized "has animation" bit is not accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherActorRuntimeFacts {
    pub actor: Handle,
    pub who: u8,
    pub o: i16,
    pub uid: u16,
    pub type_index: i32,
    pub guys_length: i32,
    pub guys_capacity: i32,
    pub guys_increment: i32,
    pub guys_flags: u8,
    pub slot_zero_present: bool,
    pub lead_animation: u8,
}

/// Revision-bound result of the target's Build/Wall virtual projection. Canonical
/// [`BuildData`] remains the mutable owner of flags, property, latch, and recharge count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherSiteRuntimeFacts {
    pub who: u8,
    pub o: i16,
    pub uid: u16,
    pub property: i32,
    pub resolves_build: bool,
    pub valid_wall_projection: bool,
}

/// Installed content/executable projection for production Gather work.
///
/// It is intentionally absent from DoNSave. A loaded simulation restores the canonical
/// World/Build/order bytes and remains fail-closed until the same nonzero composition digest
/// and revision are installed again.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GatherWorkAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub actors: Vec<GatherActorRuntimeFacts>,
    pub sites: Vec<GatherSiteRuntimeFacts>,
}

impl GatherWorkAuthority {
    fn actor(&self, actor: Handle) -> Option<GatherActorRuntimeFacts> {
        self.actors
            .iter()
            .copied()
            .find(|facts| facts.actor == actor)
    }

    fn site(&self, who: u8, o: i16, uid: u16) -> Option<GatherSiteRuntimeFacts> {
        self.sites
            .iter()
            .copied()
            .find(|facts| (facts.who, facts.o, facts.uid) == (who, o, uid))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GatherWorkRuntimeError {
    MissingAuthority,
    MissingCompositionDigest,
    DuplicateActorAuthority(Handle),
    DuplicateSiteAuthority { who: u8, o: i16, uid: u16 },
    InvalidGuyArrayFacts,
    UnitTypeProjectionMismatch,
    MissingGatherOrder,
    MissingTypedPayload,
    TargetOutOfRange,
    MissingBuildTarget,
    BuildIdentityMismatch,
    SiteAuthorityMismatch,
    UnsupportedChainWorkerType(i32),
    CorruptGatherChain,
    Plan(GatherWorkPlanError),
    StaleState,
}

impl From<GatherWorkPlanError> for GatherWorkRuntimeError {
    fn from(error: GatherWorkPlanError) -> Self {
        Self::Plan(error)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GatherChainProjection {
    pub site_head_before: i16,
    pub site_head_after: i16,
    /// `(unit row, gather_down before, gather_down after)` in canonical row order.
    pub unit_links: Vec<(usize, i16, i16)>,
    pub removed: usize,
    pub all_gathering: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedGatherWorkActivation {
    pub row: usize,
    pub build_row: usize,
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub actor_facts: GatherActorRuntimeFacts,
    pub site_facts: GatherSiteRuntimeFacts,
    pub plan: GatherWorkPlan,
    pub chain: Option<GatherChainProjection>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherWorkActivationReceipt {
    pub actor: Handle,
    pub site_who: u8,
    pub site_o: i16,
    pub site_uid: u16,
    pub authority_revision: u64,
    pub frame: i32,
    pub branch: GatherWorkBranch,
    pub wait_before: i32,
    pub wait_after: i32,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub changed_fields: u8,
    pub chain_unlinks: usize,
    pub rng_draws: u8,
}

fn work_order(order: &Order) -> Result<GatherWorkOrder, GatherWorkRuntimeError> {
    if order.kind != OrderIndex::Gather {
        return Err(GatherWorkRuntimeError::MissingGatherOrder);
    }
    let Some(EconomyOrderPayload::Gather(payload)) = order.economy else {
        return Err(GatherWorkRuntimeError::MissingTypedPayload);
    };
    Ok(GatherWorkOrder {
        node_metric: order.node_metric,
        flags: order.flags,
        target_o: i32::from(order.target_o),
        target_who: i32::from(order.target_who),
        target_uid: order.target_uid,
        tx: payload.tx,
        ty: payload.ty,
        build_type: payload.build_type,
        wait: payload.wait,
        goto_build: payload.goto_build,
        non_flat_gather: payload.non_flat_gather,
        dist_mod: payload.dist_mod,
        been_there: payload.been_there,
    })
}

fn validate_authority(authority: &GatherWorkAuthority) -> Result<(), GatherWorkRuntimeError> {
    if authority.composition_digest == [0; 32] {
        return Err(GatherWorkRuntimeError::MissingCompositionDigest);
    }
    for (index, facts) in authority.actors.iter().enumerate() {
        if authority.actors[..index]
            .iter()
            .any(|old| old.actor == facts.actor)
        {
            return Err(GatherWorkRuntimeError::DuplicateActorAuthority(facts.actor));
        }
    }
    for (index, facts) in authority.sites.iter().enumerate() {
        if authority.sites[..index]
            .iter()
            .any(|old| (old.who, old.o, old.uid) == (facts.who, facts.o, facts.uid))
        {
            return Err(GatherWorkRuntimeError::DuplicateSiteAuthority {
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
) -> Result<usize, GatherWorkRuntimeError> {
    if !(0..OWNER_SLOTS as i32).contains(&who)
        || !(BUILD_BAND_BASE as i32..=i16::MAX as i32).contains(&o)
    {
        return Err(GatherWorkRuntimeError::TargetOutOfRange);
    }
    let offset = (o - BUILD_BAND_BASE as i32) as usize;
    let row = *world
        .objects
        .slot(who as usize)
        .band(Band::Build)
        .get(offset)
        .ok_or(GatherWorkRuntimeError::MissingBuildTarget)? as usize;
    let build = builds
        .get(row)
        .ok_or(GatherWorkRuntimeError::MissingBuildTarget)?;
    if (i32::from(build.who), i32::from(build.object_id())) != (who, o) {
        return Err(GatherWorkRuntimeError::BuildIdentityMismatch);
    }
    Ok(row)
}

fn gather_assignment(
    order: Option<&Order>,
) -> Result<Option<GatherAssignment>, GatherWorkRuntimeError> {
    let Some(order) = order else {
        return Ok(None);
    };
    if order.kind != OrderIndex::Gather {
        return Ok(None);
    }
    let work = work_order(order)?;
    Ok(Some(GatherAssignment {
        target_owner: work.target_who,
        target_build: work.target_o,
        target_uid: work.target_uid,
        been_there: work.been_there != 0,
        inside_target: None,
    }))
}

fn structural_all_gathering(
    world: &World,
    unit_types: &[i32],
    build: &BuildData,
) -> Result<GatherChainProjection, GatherWorkRuntimeError> {
    let live = world.live_count() as usize;
    if unit_types.len() != live {
        return Err(GatherWorkRuntimeError::UnitTypeProjectionMismatch);
    }
    let mut workers = Vec::with_capacity(live);
    for row in 0..live {
        workers.push(GatherWorker {
            owner: world.units.get_who(row),
            unit_o: world.units.o()[row],
            type_index: unit_types[row],
            valid_unit: world.units.get_flags(row) & 1 != 0,
            assignment: gather_assignment(world.orders(row).current())?,
            gather_down: world.units.gather_down()[row],
            good_obj: world.units.good_obj()[row],
            group: world.units.group()[row],
            unit_masks: world.units.get_unit_masks(row),
            hold_doober: world.units.doober()[row],
        });
    }

    // Scholars/special gatherers consult containment before their order. That projection is
    // deliberately absent from this Camp transaction, so encountering one in the live chain
    // is a hard boundary rather than an on-map assumption.
    let mut current = build.gather_down;
    let mut visited = 0usize;
    while current >= 0 {
        if visited >= workers.len() {
            return Err(GatherWorkRuntimeError::CorruptGatherChain);
        }
        visited += 1;
        let worker = workers
            .iter()
            .find(|worker| worker.owner == build.who && worker.unit_o == current)
            .ok_or(GatherWorkRuntimeError::CorruptGatherChain)?;
        if matches!(worker.type_index, 0x34 | 0x35) {
            return Err(GatherWorkRuntimeError::UnsupportedChainWorkerType(
                worker.type_index,
            ));
        }
        current = worker.gather_down;
    }

    let before_links = workers
        .iter()
        .map(|worker| worker.gather_down)
        .collect::<Vec<_>>();
    let mut site = GatherSite {
        owner: build.who,
        build_o: build.object_id(),
        uid: build.uid,
        gather_max: build.gather_max,
        gather_down: build.gather_down,
        build_masks: build.build_masks,
        recharging: build.recharging,
    };
    let removed = gathering::check_gatherers(&mut site, &mut workers)
        .map_err(|_| GatherWorkRuntimeError::CorruptGatherChain)?;

    let mut all_gathering = true;
    current = site.gather_down;
    visited = 0;
    while current >= 0 {
        if visited >= workers.len() {
            return Err(GatherWorkRuntimeError::CorruptGatherChain);
        }
        visited += 1;
        let row = workers
            .iter()
            .position(|worker| worker.owner == site.owner && worker.unit_o == current)
            .ok_or(GatherWorkRuntimeError::CorruptGatherChain)?;
        let Some(order) = world.orders(row).current() else {
            all_gathering = false;
            break;
        };
        let work = work_order(order)?;
        if work.goto_build != 0 || work.wait < 0 {
            all_gathering = false;
            break;
        }
        current = workers[row].gather_down;
    }

    let unit_links = workers
        .iter()
        .enumerate()
        .filter_map(|(row, worker)| {
            (before_links[row] != worker.gather_down).then_some((
                row,
                before_links[row],
                worker.gather_down,
            ))
        })
        .collect();
    Ok(GatherChainProjection {
        site_head_before: build.gather_down,
        site_head_after: site.gather_down,
        unit_links,
        removed,
        all_gathering,
    })
}

/// Read and plan one production `Unit::work` Gather activation from canonical owners.
pub fn prepare_gather_work_activation(
    world: &World,
    builds: &[BuildData],
    unit_types: &[i32],
    authority: &GatherWorkAuthority,
    row: usize,
) -> Result<PreparedGatherWorkActivation, GatherWorkRuntimeError> {
    validate_authority(authority)?;
    let actor = world
        .handle_at_row(row)
        .ok_or(GatherWorkRuntimeError::MissingAuthority)?;
    let actor_facts = authority
        .actor(actor)
        .ok_or(GatherWorkRuntimeError::MissingAuthority)?;
    if (actor_facts.who, actor_facts.o, actor_facts.uid)
        != (
            world.units.get_who(row),
            world.units.o()[row],
            world.units.get_uid(row),
        )
    {
        return Err(GatherWorkRuntimeError::MissingAuthority);
    }
    if (
        actor_facts.guys_length,
        actor_facts.guys_capacity,
        actor_facts.guys_increment,
    ) != (1, 1, 1)
        || actor_facts.guys_flags != 0
        || !actor_facts.slot_zero_present
        || i32::from(world.units.guy_mark()[row]) != actor_facts.guys_length
    {
        return Err(GatherWorkRuntimeError::InvalidGuyArrayFacts);
    }
    let type_index = unit_types
        .get(row)
        .copied()
        .ok_or(GatherWorkRuntimeError::UnitTypeProjectionMismatch)?;
    if world.unit_type_id(row) != Some(type_index) || actor_facts.type_index != type_index {
        return Err(GatherWorkRuntimeError::UnitTypeProjectionMismatch);
    }
    let order = work_order(
        world
            .orders(row)
            .current()
            .ok_or(GatherWorkRuntimeError::MissingGatherOrder)?,
    )?;
    let build_row = build_row(world, builds, order.target_who, order.target_o)?;
    let build = &builds[build_row];
    if build.uid != order.target_uid {
        return Err(GatherWorkRuntimeError::BuildIdentityMismatch);
    }
    let site_facts = authority
        .site(build.who, build.object_id(), build.uid)
        .ok_or(GatherWorkRuntimeError::MissingAuthority)?;
    if site_facts.property != build.orig_type
        || !site_facts.resolves_build
        || !site_facts.valid_wall_projection
    {
        return Err(GatherWorkRuntimeError::SiteAuthorityMismatch);
    }
    let before = GatherWorkSnapshot {
        revision: authority.revision,
        frame: world.frame,
        rng_state: world.random.state(),
        actor: GatherActorImage {
            who: world.units.get_who(row),
            o: world.units.o()[row],
            uid: world.units.get_uid(row),
            group: world.units.group()[row],
            unit_masks: world.units.get_unit_masks(row),
            lead_animation: Some(actor_facts.lead_animation),
        },
        order,
        site: GatherSiteImage {
            who: i32::from(build.who),
            o: i32::from(build.object_id()),
            uid: build.uid,
            resolved_build: site_facts.resolves_build,
            valid_wall: build.is_valid() && site_facts.valid_wall_projection,
            active: build.is_active(),
            property: build.orig_type,
            build_masks: build.build_masks,
            recharging: build.recharging,
        },
    };
    let (chain, plan) = if order.wait == 1 {
        let chain = structural_all_gathering(world, unit_types, build)?;
        let plan = plan_fresh_gather_tick_at_wait_zero(before, chain.all_gathering)?;
        (Some(chain), plan)
    } else {
        (None, plan_fresh_gather_tick(before)?)
    };
    Ok(PreparedGatherWorkActivation {
        row,
        build_row,
        authority_revision: authority.revision,
        authority_digest: authority.composition_digest,
        actor_facts,
        site_facts,
        plan,
        chain,
    })
}

/// Re-read the complete plan and publish actor/order/Build/chain/RNG writes by assignment.
/// Every possible failure precedes the first write.
pub fn commit_gather_work_activation(
    world: &mut World,
    builds: &mut [BuildData],
    unit_types: &[i32],
    authority: &GatherWorkAuthority,
    prepared: PreparedGatherWorkActivation,
) -> Result<GatherWorkActivationReceipt, GatherWorkRuntimeError> {
    let current =
        prepare_gather_work_activation(world, builds, unit_types, authority, prepared.row)?;
    if current != prepared {
        return Err(GatherWorkRuntimeError::StaleState);
    }

    let before = prepared.plan.before;
    let after = prepared.plan.after;
    world.units.group_mut()[prepared.row] = after.actor.group;
    world
        .units
        .set_unit_masks(prepared.row, after.actor.unit_masks);
    builds[prepared.build_row].build_masks = after.site.build_masks;
    builds[prepared.build_row].recharging = after.site.recharging;
    if let Some(chain) = &prepared.chain {
        builds[prepared.build_row].gather_down = chain.site_head_after;
        for &(row, _, after_link) in &chain.unit_links {
            world.units.gather_down_mut()[row] = after_link;
        }
    }
    let current_order = world
        .orders_mut(prepared.row)
        .current_mut()
        .expect("preflight retained the current Gather order");
    let EconomyOrderPayload::Gather(payload) = current_order
        .economy
        .as_mut()
        .expect("preflight retained the concrete Gather payload")
    else {
        unreachable!("preflight retained the concrete Gather payload")
    };
    payload.wait = after.order.wait;
    if after.rng_state != before.rng_state {
        world.random.reseed(after.rng_state);
    }
    Ok(GatherWorkActivationReceipt {
        actor: prepared.actor_facts.actor,
        site_who: prepared.site_facts.who,
        site_o: prepared.site_facts.o,
        site_uid: prepared.site_facts.uid,
        authority_revision: prepared.authority_revision,
        frame: before.frame,
        branch: prepared.plan.branch,
        wait_before: before.order.wait,
        wait_after: after.order.wait,
        random_state_before: before.rng_state,
        random_state_after: after.rng_state,
        changed_fields: prepared.plan.changed_fields,
        chain_unlinks: prepared.chain.as_ref().map_or(0, |chain| chain.removed),
        rng_draws: prepared.plan.rng_draws,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatherWorkHostError {
    Unavailable,
    StaleSnapshot,
}

/// Proof returned only after an atomic whole-image compare/exchange.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherWorkCommitReceipt {
    pub before_revision: u64,
    pub after_revision: u64,
    pub after: GatherWorkSnapshot,
    pub changed_fields: u8,
    pub rng_draws: u8,
    pub external_effects: u8,
}

impl GatherWorkCommitReceipt {
    pub fn applied(plan: &GatherWorkPlan) -> Self {
        let mut after = plan.after;
        after.revision = plan.before.revision.wrapping_add(1);
        Self {
            before_revision: plan.before.revision,
            after_revision: after.revision,
            after,
            changed_fields: plan.changed_fields,
            rng_draws: plan.rng_draws,
            external_effects: plan.external_effects,
        }
    }

    pub fn validates(self, plan: &GatherWorkPlan) -> bool {
        let expected = Self::applied(plan);
        self == expected
    }
}

/// Host boundary required to make actor/order/build publication indivisible.
pub trait AtomicGatherWorkHost {
    fn snapshot(
        &mut self,
        actor_who: u8,
        actor_o: i16,
    ) -> Result<GatherWorkSnapshot, GatherWorkHostError>;

    /// Compare `plan.before` against the current complete image, then apply either all of
    /// `plan.after` or none of it.  No RNG/effect call is authorized by this plan.
    fn compare_exchange(
        &mut self,
        plan: &GatherWorkPlan,
    ) -> Result<GatherWorkCommitReceipt, GatherWorkHostError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatherWorkTransactionError {
    Host(GatherWorkHostError),
    Plan(GatherWorkPlanError),
    InvalidCommitReceipt,
}

/// Acquire, plan, and atomically publish one recovered Gather tick.
pub fn resume_fresh_gather_tick<H: AtomicGatherWorkHost>(
    host: &mut H,
    actor_who: u8,
    actor_o: i16,
) -> Result<GatherWorkCommitReceipt, GatherWorkTransactionError> {
    let before = host
        .snapshot(actor_who, actor_o)
        .map_err(GatherWorkTransactionError::Host)?;
    if (before.actor.who, before.actor.o) != (actor_who, actor_o) {
        return Err(GatherWorkTransactionError::Plan(
            GatherWorkPlanError::TargetIdentityMismatch,
        ));
    }
    let plan = plan_fresh_gather_tick(before).map_err(GatherWorkTransactionError::Plan)?;
    let receipt = host
        .compare_exchange(&plan)
        .map_err(GatherWorkTransactionError::Host)?;
    if !receipt.validates(&plan) {
        return Err(GatherWorkTransactionError::InvalidCommitReceipt);
    }
    Ok(receipt)
}
