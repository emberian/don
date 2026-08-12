// SPDX-License-Identifier: GPL-3.0-or-later
//! Atomic saved-order seam for the fresh retail `Unit::do_gather` cohort.
//!
//! The fresh v16 SVX contains 29 exact `GatherOrder` images: 17 active Farm orders and
//! 12 active Camp/Woodcutter orders.  This module binds both shapes without pretending
//! that all of `Unit::do_gather` is available.  It executes one substantive branch shared
//! by all 12 Camp images: a non-capacity phase, `goto_build == 0`, lead animation `0x19`,
//! and a positive wait which remains nonzero after its retail decrement.
//!
//! Planning is read-only.  Publishing is one compare/exchange host call over the actor,
//! order, target building, RNG state, and authority revision.  Farm, Mine, destination
//! selection, capacity, all-gathering/RNG, direct-resource, cast/special, and retirement
//! branches fail closed before that call.  There is no geometric, resource-rate, or scalar
//! approximation.

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
    ]
    .into_iter()
    .map(u8::from)
    .sum()
}

/// Plan the exact no-RNG Camp timer tick reached by the fresh saved payloads.
pub fn plan_fresh_gather_tick(
    before: GatherWorkSnapshot,
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
    // A decrement from one reaches `Build::all_gathering`, and possibly Random::get.
    if before.order.wait <= 1 {
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

    Ok(GatherWorkPlan {
        before,
        after,
        branch: GatherWorkBranch::CampAnimation19Waiting,
        changed_fields: changed_fields(&before, &after),
        rng_draws: 0,
        external_effects: 0,
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
