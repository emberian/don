// SPDX-License-Identifier: GPL-3.0-or-later
//! Atomic saved-order seam for the fresh retail `Unit::do_gather` cohort.
//!
//! The fresh v16 SVX contains 29 exact `GatherOrder` images: 17 active Farm orders and
//! 12 active Camp/Woodcutter orders.  This module binds both shapes without pretending
//! that all of `Unit::do_gather` is available.  It executes one substantive branch shared
//! by all 12 Camp images: a non-capacity phase, `goto_build == 0`, lead animation `0x19`,
//! and the animation-`0x19` wait loop, including its exact wait-zero
//! `Build::all_gathering`/single-RNG-draw tail.  It also executes the exact saved Farm
//! status-one grow for stable animations and for the exact saved type-50 animation-8→35
//! transition whose complete `UnitGuys` after-image is now canonically owned. The queued
//! owner-2/o-9 continuation also owns status-two `Farms::snip` and animation 8→36.
//!
//! Planning is read-only.  Publishing is one compare/exchange host call over the actor,
//! order, target building, RNG state, and authority revision.  Unadmitted Farm cell mutation,
//! relocation, Mine, destination selection, capacity, direct-resource, cast/special, and
//! retirement branches fail closed before that call.  There is no geometric, resource-rate,
//! or scalar approximation.

use crate::objects::{Band, BUILD_BAND_BASE, OWNER_SLOTS};
use crate::order::{Order, OrderIndex};
use crate::systems::economy_order_payload_authority::EconomyOrderPayload;
use crate::systems::gathering::{self, GatherAssignment, GatherSite, GatherWorker};
use crate::systems::groups_guys::UnitGuys;
use crate::systems::map_terrain::{Coord, TCoord};
use crate::systems::production::BuildData;
use crate::world::{Handle, World};

/// `ObjectTypeData::property` values stored in `GatherOrder::build_type`.
pub const FARM_PROPERTY: i32 = 0x1a1;
pub const CAMP_PROPERTY: i32 = 0x1a2;
pub const MINE_PROPERTY: i32 = 0x1a3;

/// Lead `GuyData` animation read by the supported `do_non_flat_gather` timer arm.
pub const GATHER_ANIMATION_19: u8 = 0x19;
/// Farm action selected by `Farms::update(index) == 1`.
pub const FARM_ANIMATION_23: u8 = 0x23;
/// Farm action selected by cell status `3`.
pub const FARM_ANIMATION_24: u8 = 0x24;
/// Unit action bits cleared at `0x005F0209` when `goto_build == 0`.
pub const GATHER_ACTION_MASKS: u32 = 0x7800_0000;
/// Building latch tested/set at the heads of `do_gather` and `do_non_flat_gather`.
pub const GATHER_SITE_LATCH: u16 = 0x0800;
/// The Camp/Mine capacity branch runs only on this 128-frame phase.
pub const CAPACITY_PHASE_MASK: i32 = 0x7f;

/// Exact 190-byte payload walked for one retail `FarmStruct` (PDB size 192).
///
/// Float fields are stored as IEEE-754 bits so save/checksum images and the shipped
/// single-precision `addss` in `Farms::grow` remain explicit and equality-safe.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FarmStruct {
    pub who: i32,
    pub o: i32,
    pub percent: [u32; 16],
    pub terrain_height: [u32; 25],
    pub status: [u8; 16],
    pub valid: u8,
    pub farm_type: u8,
}

impl FarmStruct {
    pub const WALKED_BYTES: usize = 190;

    pub fn image(self) -> [u8; Self::WALKED_BYTES] {
        let mut out = [0; Self::WALKED_BYTES];
        out[0..4].copy_from_slice(&self.who.to_le_bytes());
        out[4..8].copy_from_slice(&self.o.to_le_bytes());
        for (index, bits) in self.percent.into_iter().enumerate() {
            out[8 + index * 4..12 + index * 4].copy_from_slice(&bits.to_le_bytes());
        }
        for (index, bits) in self.terrain_height.into_iter().enumerate() {
            out[72 + index * 4..76 + index * 4].copy_from_slice(&bits.to_le_bytes());
        }
        out[172..188].copy_from_slice(&self.status);
        out[188] = self.valid;
        out[189] = self.farm_type;
        out
    }

    /// `TerrainOut::find_data_z` over the 5×5 vertex image saved beside this Farm.
    /// Farm arrays use the same x-major indexing as `status[x][y]`.
    pub fn data_z(self, corner_tx: i32, corner_ty: i32, x: i32, y: i32) -> Option<i32> {
        let tx = TCoord::from_coord(Coord(x)).0;
        let ty = TCoord::from_coord(Coord(y)).0;
        let dx = tx.checked_sub(corner_tx)?;
        let dy = ty.checked_sub(corner_ty)?;
        if !(0..4).contains(&dx) || !(0..4).contains(&dy) {
            return None;
        }
        let vertex = |vx: i32, vy: i32| f32::from_bits(self.terrain_height[(vx * 5 + vy) as usize]);
        let lx = x.wrapping_sub(tx.wrapping_mul(TCoord::SCALE));
        let ly = y.wrapping_sub(ty.wrapping_mul(TCoord::SCALE));
        if !(0..TCoord::SCALE).contains(&lx) || !(0..TCoord::SCALE).contains(&ly) {
            return None;
        }
        let scale = f32::from_bits(0x3baa_aaab); // shipped `0.0052083335f`
        let f00 = vertex(dx, dy);
        let f10 = vertex(dx + 1, dy);
        let f01 = vertex(dx, dy + 1);
        let z = if TCoord::SCALE.wrapping_sub(ly) < lx {
            let f11 = vertex(dx + 1, dy + 1);
            let a = (f01 - f11) * scale * (TCoord::SCALE - lx) as f32;
            let b = (f11 - f10) * scale * ly as f32;
            f10 + b + 0.0 + a
        } else {
            let a = (f10 - f00) * scale * lx as f32;
            let b = (f01 - f00) * scale * ly as f32;
            a + f00 + 0.0 + b
        };
        z.is_finite().then_some(z as i32)
    }
}

/// Canonical synchronized owner for retail's global `Farms::farm_data` array.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Farms {
    records: Vec<FarmStruct>,
    capacity: i32,
    increment: i16,
    flags: u8,
}

impl Default for Farms {
    fn default() -> Self {
        Self::with_header(5, -1, 0)
    }
}

impl Farms {
    pub fn with_header(capacity: i32, increment: i16, flags: u8) -> Self {
        Self {
            records: Vec::with_capacity(capacity.max(0) as usize),
            capacity,
            increment,
            flags,
        }
    }

    pub fn header(&self) -> (i32, i32, i16, u8) {
        (
            self.records.len() as i32,
            self.capacity,
            self.increment,
            self.flags,
        )
    }

    pub fn records(&self) -> &[FarmStruct] {
        &self.records
    }

    /// Exact dynamic-array header followed by every 190-byte FarmStruct walk image.
    pub fn walked_image(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(11 + self.records.len() * FarmStruct::WALKED_BYTES);
        out.extend_from_slice(&(self.records.len() as i32).to_le_bytes());
        out.extend_from_slice(&self.capacity.to_le_bytes());
        out.extend_from_slice(&self.increment.to_le_bytes());
        out.push(self.flags);
        for record in &self.records {
            out.extend_from_slice(&record.image());
        }
        out
    }

    pub fn get(&self, index: usize) -> Option<&FarmStruct> {
        self.records.get(index)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut FarmStruct> {
        self.records.get_mut(index)
    }

    pub fn push(&mut self, record: FarmStruct) -> Result<usize, ()> {
        if self.records.len() as i32 >= self.capacity {
            return Err(());
        }
        self.records.push(record);
        Ok(self.records.len() - 1)
    }
}

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
    FarmPeriodicTargetSearch,
    FarmOutsideFootprint,
    FarmCellMutation(u8),
    FarmRelocationAndRng,
    FarmAnimationMutation { current: u8, requested: u8 },
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
    FarmUpdateOneAnimation23GateMiss,
    FarmPeriodicNoRepairAnimation24,
    FarmStatus3Animation24,
    FarmStatus1Grow,
    FarmStatus2Snip,
    CampAnimation19Waiting,
    CampAnimation19AllGathering,
    CampAnimation19Rescheduled,
}

/// Installed geometry/content expectations read by the admitted saved-Farm branches.
///
/// `FarmStruct` and a materialized `UnitGuys` row are canonical gameplay owners. These facts
/// bind their interpretation to exact Build virtuals, footprint geometry, and animation packet
/// clocks; prepare validates them against the owners before staging a whole-owner after-image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherFarmRuntimeFacts {
    pub actor: Handle,
    pub site_who: u8,
    pub site_o: i16,
    pub site_uid: u16,
    pub farm_index: i16,
    pub farm_record_who: u8,
    pub farm_record_o: i16,
    pub farm_record_valid: bool,
    /// `FarmStruct::farm_type`; `Farms::update` returns its low bit.
    pub farm_type: u8,
    /// Exact result of `WallData::covers_tile(actor_tx, actor_ty)`.
    pub covers_actor_tile: bool,
    pub corner_tx: i32,
    pub corner_ty: i32,
    pub x_size: i32,
    pub y_size: i32,
    /// Selected `FarmStruct::status[x][y]`. Retail flattens this as `x * y_size + y`, not
    /// conventional row-major `y * x_size + x`. Unread on the `farm_type & 1` branch.
    pub selected_cell_status: Option<u8>,
    pub lead_cur_time: u32,
    pub lead_end_time: u32,
    pub lead_hold_attack: u8,
    /// Exact `LeaderData::get_diff()` result read on this actor's 256-frame Farm phase.
    /// `None` keeps that phase fail-closed.
    pub periodic_effective_difficulty: Option<i32>,
}

/// Canonical mutable reads which are not part of the existing Camp snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherFarmReadImage {
    pub actor_x: i32,
    pub actor_y: i32,
    pub actor_tx: i32,
    pub actor_ty: i32,
    pub site_city: i16,
    pub site_farm_index: i16,
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
    pub farm_before: Option<FarmStruct>,
    pub farm_after: Option<FarmStruct>,
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
        farm_before: None,
        farm_after: None,
    })
}

/// Plan the exact Farm continuations whose complete mutation surfaces are owned.
///
/// These are deliberately separate from [`plan_fresh_gather_tick`]: a Farm needs an exact
/// `FarmStruct` cell and lead-Guy clock image. Status one grows the canonical FarmStruct;
/// status zero relocates with RNG, status two snips, and animation changes stay closed.
pub fn plan_fresh_farm_tick(
    before: GatherWorkSnapshot,
    facts: GatherFarmRuntimeFacts,
    reads: GatherFarmReadImage,
    farm: FarmStruct,
) -> Result<GatherWorkPlan, GatherWorkPlanError> {
    plan_fresh_farm_tick_inner(before, facts, reads, farm, false, None)
}

fn plan_fresh_farm_tick_inner(
    before: GatherWorkSnapshot,
    facts: GatherFarmRuntimeFacts,
    reads: GatherFarmReadImage,
    farm: FarmStruct,
    owns_animation_8_to_farm_work: bool,
    target_damage: Option<i32>,
) -> Result<GatherWorkPlan, GatherWorkPlanError> {
    if bind_fresh_gather_payload(before.actor.who, before.order)?
        != FreshGatherPayloadClass::FarmActive
    {
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
    let periodic_phase = before.actor.unit_masks & 0x0004_0000 != 0
        && before
            .frame
            .wrapping_add(i32::from(before.actor.o))
            .wrapping_add(i32::from(before.actor.who))
            & 0xff
            == 0;
    let periodic_no_repair = if periodic_phase {
        let (Some(difficulty), Some(damage)) = (facts.periodic_effective_difficulty, target_damage)
        else {
            return Err(GatherWorkPlanError::Unowned(
                UnownedGatherArm::FarmPeriodicTargetSearch,
            ));
        };
        if difficulty > 1 && damage != 0 {
            return Err(GatherWorkPlanError::Unowned(
                UnownedGatherArm::FarmPeriodicTargetSearch,
            ));
        }
        true
    } else {
        false
    };
    if reads.site_city < 0
        || reads.site_farm_index < 0
        || reads.site_farm_index != facts.farm_index
        || (
            facts.farm_record_who,
            facts.farm_record_o,
            facts.farm_record_valid,
        ) != (before.site.who as u8, before.site.o as i16, true)
        || (farm.who, farm.o, farm.valid) != (before.site.who, before.site.o, 1)
        || farm.farm_type != facts.farm_type
        || !facts.covers_actor_tile
    {
        return Err(GatherWorkPlanError::Unowned(
            UnownedGatherArm::FarmWorldAndAnimation,
        ));
    }

    let (branch, requested_animation) = if facts.farm_type & 1 != 0 {
        let gate = before
            .frame
            .wrapping_add(i32::from(before.actor.o).wrapping_mul(7))
            .wrapping_add(i32::from(before.actor.who))
            & 0xff;
        if gate == 0 {
            return Err(GatherWorkPlanError::Unowned(
                UnownedGatherArm::FarmRelocationAndRng,
            ));
        }
        (
            GatherWorkBranch::FarmUpdateOneAnimation23GateMiss,
            FARM_ANIMATION_23,
        )
    } else {
        if facts.x_size <= 0 || facts.y_size <= 0 {
            return Err(GatherWorkPlanError::Unowned(
                UnownedGatherArm::FarmOutsideFootprint,
            ));
        }
        let dx = reads.actor_tx.wrapping_sub(facts.corner_tx);
        let dy = reads.actor_ty.wrapping_sub(facts.corner_ty);
        if dx < 0 || dy < 0 || dx >= facts.x_size || dy >= facts.y_size {
            return Err(GatherWorkPlanError::Unowned(
                UnownedGatherArm::FarmOutsideFootprint,
            ));
        }
        // `0x005eff2d..0x005eff4b`: base + 0xac + local_x*4 + local_y. The first
        // subscript is x; transposing these indices invents relocation/snip branches which
        // none of the fresh witnesses actually executes.
        let cell = (dx * facts.y_size + dy) as usize;
        let status = farm.status.get(cell).copied();
        if status != facts.selected_cell_status {
            return Err(GatherWorkPlanError::Unowned(
                UnownedGatherArm::FarmWorldAndAnimation,
            ));
        }
        match status {
            Some(3) if periodic_no_repair => (
                GatherWorkBranch::FarmPeriodicNoRepairAnimation24,
                FARM_ANIMATION_24,
            ),
            Some(3) => (GatherWorkBranch::FarmStatus3Animation24, FARM_ANIMATION_24),
            Some(0) => {
                return Err(GatherWorkPlanError::Unowned(
                    UnownedGatherArm::FarmRelocationAndRng,
                ));
            }
            Some(1) => (GatherWorkBranch::FarmStatus1Grow, FARM_ANIMATION_23),
            Some(2) => (GatherWorkBranch::FarmStatus2Snip, FARM_ANIMATION_24),
            _ => {
                return Err(GatherWorkPlanError::Unowned(
                    UnownedGatherArm::FarmWorldAndAnimation,
                ));
            }
        }
    };

    let current = before
        .actor
        .lead_animation
        .ok_or(GatherWorkPlanError::Unowned(
            UnownedGatherArm::MissingLeadGuy,
        ))?;
    let exact_owned_animation_mutation = owns_animation_8_to_farm_work
        && current == 8
        && matches!(requested_animation, FARM_ANIMATION_23 | FARM_ANIMATION_24);
    if (current != requested_animation && !exact_owned_animation_mutation)
        || facts.lead_hold_attack != 0
        || facts.lead_cur_time >= facts.lead_end_time
    {
        return Err(GatherWorkPlanError::Unowned(
            UnownedGatherArm::FarmAnimationMutation {
                current,
                requested: requested_animation,
            },
        ));
    }

    let mut farm_after = farm;
    if branch == GatherWorkBranch::FarmStatus1Grow {
        let dx = reads.actor_tx.wrapping_sub(facts.corner_tx) as usize;
        let dy = reads.actor_ty.wrapping_sub(facts.corner_ty) as usize;
        let cell = dx * facts.y_size as usize + dy;
        farm_after.status[cell] = 1;
        let grown = f32::from_bits(farm_after.percent[cell]) + f32::from_bits(0x3ba3_d70a);
        farm_after.percent[cell] = grown.min(1.0).to_bits();
        if grown > 1.0 {
            farm_after.status[cell] = 2;
        }
    } else if branch == GatherWorkBranch::FarmStatus2Snip {
        let dx = reads.actor_tx.wrapping_sub(facts.corner_tx) as usize;
        let dy = reads.actor_ty.wrapping_sub(facts.corner_ty) as usize;
        let cell = dx * facts.y_size as usize + dy;
        // `Farms::snip` `0x008D9240`: the selected byte alone changes from two to three.
        farm_after.status[cell] = 3;
    }

    Ok(GatherWorkPlan {
        before,
        after: before,
        branch,
        changed_fields: u8::from(farm_after != farm),
        rng_draws: 0,
        external_effects: 0,
        farm_before: Some(farm),
        farm_after: Some(farm_after),
    })
}

/// Revision-bound expected identity/header for the slot-zero `GuyData` body. A materialized
/// canonical UnitGuys row is revalidated against these exact `(1,1,1,0)` fresh-SVX facts; a
/// normalized "has animation" bit is not accepted.
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
    /// Exact shipped type/constant projection for this actor's queued-MOVE continuation.
    pub move_runtime: Option<GatherMoveRuntimeFacts>,
}

/// Immutable facts read by `Unit::move_step`, `Guy::turn_speed`, and the admitted one-Guy
/// `Unit::process` tail. These are reinstalled content facts, never a second saved owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherMoveRuntimeFacts {
    pub expected_unit_masks: u32,
    pub expected_myspeed: i16,
    pub type_unit_flags: u32,
    pub type_special_wide_turner: bool,
    pub unit_type: crate::systems::groups_guys::UnitTypeStats,
    pub turn_scale: i32,
    pub turn_scale2: i32,
    pub ai_speed: i32,
    pub lead_gpiece: i32,
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
    pub farms: Vec<GatherFarmRuntimeFacts>,
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

    fn farm(&self, actor: Handle, who: u8, o: i16, uid: u16) -> Option<GatherFarmRuntimeFacts> {
        self.farms.iter().copied().find(|facts| {
            facts.actor == actor && (facts.site_who, facts.site_o, facts.site_uid) == (who, o, uid)
        })
    }

    pub(crate) fn move_actor(
        &self,
        actor: Handle,
    ) -> Option<(GatherActorRuntimeFacts, GatherMoveRuntimeFacts)> {
        if self.composition_digest == [0; 32] {
            return None;
        }
        let actor = self.actor(actor)?;
        Some((actor, actor.move_runtime?))
    }

    pub(crate) fn farm_for_actor(
        &self,
        actor: Handle,
        who: u8,
        o: i16,
        uid: u16,
    ) -> Option<GatherFarmRuntimeFacts> {
        self.farm(actor, who, o, uid)
    }

    pub(crate) fn first_farm_for_actor(&self, actor: Handle) -> Option<GatherFarmRuntimeFacts> {
        self.farms
            .iter()
            .copied()
            .find(|facts| facts.actor == actor)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GatherWorkRuntimeError {
    MissingAuthority,
    MissingCompositionDigest,
    DuplicateActorAuthority(Handle),
    DuplicateSiteAuthority {
        who: u8,
        o: i16,
        uid: u16,
    },
    DuplicateFarmAuthority {
        actor: Handle,
        who: u8,
        o: i16,
        uid: u16,
    },
    InvalidGuyArrayFacts,
    UnitTypeProjectionMismatch,
    MissingGatherOrder,
    MissingTypedPayload,
    TargetOutOfRange,
    MissingBuildTarget,
    BuildIdentityMismatch,
    SiteAuthorityMismatch,
    FarmAuthorityMismatch,
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
    pub farm_facts: Option<GatherFarmRuntimeFacts>,
    pub farm_reads: Option<GatherFarmReadImage>,
    pub farm_index: Option<usize>,
    /// Complete Guys-array compare/exchange images for the exact animation-8→35 arm.
    pub unit_guys_before: Option<UnitGuys>,
    pub unit_guys_after: Option<UnitGuys>,
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
    pub guy_changed_fields: u8,
    pub chain_unlinks: usize,
    pub rng_draws: u8,
    pub farm_index: Option<usize>,
    pub farm_percent_before: Option<u32>,
    pub farm_percent_after: Option<u32>,
    pub guy_animation_before: Option<i8>,
    pub guy_animation_after: Option<i8>,
    pub guy_cur_time_before: Option<u32>,
    pub guy_cur_time_after: Option<u32>,
    pub guy_end_time_before: Option<u32>,
    pub guy_end_time_after: Option<u32>,
    pub guy_last_time_before: Option<i32>,
    pub guy_last_time_after: Option<i32>,
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
    for (index, facts) in authority.farms.iter().enumerate() {
        if authority.farms[..index].iter().any(|old| {
            (old.actor, old.site_who, old.site_o, old.site_uid)
                == (facts.actor, facts.site_who, facts.site_o, facts.site_uid)
        }) {
            return Err(GatherWorkRuntimeError::DuplicateFarmAuthority {
                actor: facts.actor,
                who: facts.site_who,
                o: facts.site_o,
                uid: facts.site_uid,
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
    farms: &Farms,
    unit_guys: &[Option<UnitGuys>],
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
    if unit_guys.len() != world.live_count() as usize {
        return Err(GatherWorkRuntimeError::InvalidGuyArrayFacts);
    }
    let owned_guys = unit_guys
        .get(row)
        .ok_or(GatherWorkRuntimeError::InvalidGuyArrayFacts)?
        .as_ref();
    let owned_lead = if let Some(guys) = owned_guys {
        if (
            guys.guys.len(),
            guys.size,
            i32::from(guys.increment),
            guys.flags,
        ) != (
            actor_facts.guys_length as usize,
            actor_facts.guys_capacity,
            actor_facts.guys_increment,
            actor_facts.guys_flags,
        ) || guys.guy_mark != world.units.guy_mark()[row]
            || guys.guy_mark != 1
        {
            return Err(GatherWorkRuntimeError::InvalidGuyArrayFacts);
        }
        let lead = guys
            .guys
            .first()
            .and_then(Option::as_ref)
            .copied()
            .ok_or(GatherWorkRuntimeError::InvalidGuyArrayFacts)?;
        let derived_post_snip_lead = actor_facts.move_runtime.is_some()
            && actor_facts.lead_animation == 8
            && lead.ty == 50
            && lead.gpiece == 6_336
            && lead.cur_anim == FARM_ANIMATION_24 as i8
            && lead.cur_time < 85
            && lead.end_time == 85
            && lead.last_time == lead.cur_time as i32 - 1
            && lead.hold_attack == 0;
        if (lead.who, lead.o, lead.guy_num, lead.ty)
            != (
                actor_facts.who as i8,
                actor_facts.o,
                0,
                actor_facts.type_index,
            )
            || (lead.cur_anim as u8 != actor_facts.lead_animation && !derived_post_snip_lead)
        {
            return Err(GatherWorkRuntimeError::InvalidGuyArrayFacts);
        }
        Some(lead)
    } else {
        None
    };
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
    let payload_class =
        bind_fresh_gather_payload(actor_facts.who, order).map_err(GatherWorkPlanError::from)?;
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
            lead_animation: Some(
                owned_lead.map_or(actor_facts.lead_animation, |lead| lead.cur_anim as u8),
            ),
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
    let (farm_facts, farm_reads, chain, unit_guys_before, unit_guys_after, plan) =
        match payload_class {
            FreshGatherPayloadClass::FarmActive => {
                let authority_farm_facts = authority
                    .farm(actor, build.who, build.object_id(), build.uid)
                    .ok_or(GatherWorkRuntimeError::MissingAuthority)?;
                if (
                    authority_farm_facts.site_who,
                    authority_farm_facts.site_o,
                    authority_farm_facts.site_uid,
                ) != (build.who, build.object_id(), build.uid)
                    || authority_farm_facts.farm_record_who != build.who
                    || authority_farm_facts.farm_record_o != build.object_id()
                {
                    return Err(GatherWorkRuntimeError::FarmAuthorityMismatch);
                }
                let actor_x = world.units.x_internal()[row];
                let actor_y = world.units.y_internal()[row];
                let reads = GatherFarmReadImage {
                    actor_x,
                    actor_y,
                    actor_tx: TCoord::from_coord(Coord(actor_x)).0,
                    actor_ty: TCoord::from_coord(Coord(actor_y)).0,
                    site_city: build.city,
                    site_farm_index: build.dock,
                };
                let farm_index = usize::try_from(build.dock)
                    .map_err(|_| GatherWorkRuntimeError::FarmAuthorityMismatch)?;
                let farm = farms
                    .get(farm_index)
                    .copied()
                    .ok_or(GatherWorkRuntimeError::FarmAuthorityMismatch)?;
                let mut farm_facts = authority_farm_facts;
                let local_x = reads.actor_tx.wrapping_sub(farm_facts.corner_tx);
                let local_y = reads.actor_ty.wrapping_sub(farm_facts.corner_ty);
                let selected = (local_x >= 0
                    && local_y >= 0
                    && local_x < farm_facts.x_size
                    && local_y < farm_facts.y_size)
                    .then(|| (local_x * farm_facts.y_size + local_y) as usize)
                    .and_then(|cell| farm.status.get(cell).copied());
                let derived_post_snip = owned_lead.is_some_and(|lead| {
                    actor_facts.move_runtime.is_some()
                        && actor_facts.lead_animation == 8
                        && authority_farm_facts.selected_cell_status == Some(2)
                        && selected == Some(3)
                        && lead.ty == 50
                        && lead.gpiece == 6_336
                        && lead.cur_anim == FARM_ANIMATION_24 as i8
                        && lead.cur_time < 85
                        && lead.end_time == 85
                        && lead.last_time == lead.cur_time as i32 - 1
                        && lead.hold_attack == 0
                });
                if derived_post_snip {
                    let lead = owned_lead.expect("derived state retained lead Guy");
                    farm_facts.selected_cell_status = Some(3);
                    farm_facts.lead_cur_time = lead.cur_time;
                    farm_facts.lead_end_time = lead.end_time;
                    farm_facts.lead_hold_attack = lead.hold_attack as u8;
                }
                if owned_lead.is_some_and(|lead| {
                    (lead.cur_time, lead.end_time, lead.hold_attack as u8)
                        != (
                            farm_facts.lead_cur_time,
                            farm_facts.lead_end_time,
                            farm_facts.lead_hold_attack,
                        )
                }) {
                    return Err(GatherWorkRuntimeError::InvalidGuyArrayFacts);
                }
                let owns_animation_8_to_farm_work = actor_facts.lead_animation == 8
                    && owned_lead.is_some_and(|lead| {
                        // Exact content/witness binding: type 50's gpiece 6336 packet maps
                        // UnitAnim 35/36 to 47/85 frames in the shipped retail content.
                        lead.ty == 50
                            && lead.gpiece == 6_336
                            && lead.cur_time < 15
                            && lead.end_time == 15
                            && lead.last_time == lead.cur_time as i32 - 1
                            && lead.hold_attack == 0
                    });
                let plan = plan_fresh_farm_tick_inner(
                    before,
                    farm_facts,
                    reads,
                    farm,
                    owns_animation_8_to_farm_work,
                    Some(build.damage),
                )?;
                let (guys_before, guys_after) = if owns_animation_8_to_farm_work {
                    let before = owned_guys
                        .expect("owned animation preflight retained UnitGuys")
                        .clone();
                    let mut after = before.clone();
                    let lead = after.guys[0]
                        .as_mut()
                        .expect("owned animation preflight retained lead Guy");
                    let requested = match plan.branch {
                        GatherWorkBranch::FarmStatus2Snip
                        | GatherWorkBranch::FarmStatus3Animation24
                        | GatherWorkBranch::FarmPeriodicNoRepairAnimation24 => FARM_ANIMATION_24,
                        _ => FARM_ANIMATION_23,
                    };
                    // Unit::set_anim(requested,0,1) -> Guy::set_anim on the sole initialized
                    // Guy. Old class 8 clears hold_attack (already zero); the nonzero work arm
                    // resets this exact four-field walked surface without drawing RNG.
                    lead.cur_time = 0;
                    lead.end_time = if requested == FARM_ANIMATION_24 {
                        85
                    } else {
                        47
                    };
                    lead.last_time = -1;
                    lead.cur_anim = requested as i8;
                    (Some(before), Some(after))
                } else {
                    (None, None)
                };
                (
                    Some(farm_facts),
                    Some(reads),
                    None,
                    guys_before,
                    guys_after,
                    plan,
                )
            }
            FreshGatherPayloadClass::CampActiveTimer if order.wait == 1 => {
                let chain = structural_all_gathering(world, unit_types, build)?;
                let plan = plan_fresh_gather_tick_at_wait_zero(before, chain.all_gathering)?;
                (None, None, Some(chain), None, None, plan)
            }
            FreshGatherPayloadClass::CampActiveTimer => (
                None,
                None,
                None,
                None,
                None,
                plan_fresh_gather_tick(before)?,
            ),
        };
    Ok(PreparedGatherWorkActivation {
        row,
        build_row,
        authority_revision: authority.revision,
        authority_digest: authority.composition_digest,
        actor_facts,
        site_facts,
        farm_facts,
        farm_reads,
        farm_index: farm_reads.map(|reads| reads.site_farm_index as usize),
        unit_guys_before,
        unit_guys_after,
        plan,
        chain,
    })
}

/// Re-read the complete plan and publish actor/order/Build/chain/RNG writes by assignment.
/// Every possible failure precedes the first write.
pub fn commit_gather_work_activation(
    world: &mut World,
    builds: &mut [BuildData],
    farms: &mut Farms,
    unit_guys: &mut [Option<UnitGuys>],
    unit_types: &[i32],
    authority: &GatherWorkAuthority,
    prepared: PreparedGatherWorkActivation,
) -> Result<GatherWorkActivationReceipt, GatherWorkRuntimeError> {
    let current = prepare_gather_work_activation(
        world,
        builds,
        farms,
        unit_guys,
        unit_types,
        authority,
        prepared.row,
    )?;
    if current != prepared {
        return Err(GatherWorkRuntimeError::StaleState);
    }

    let before = prepared.plan.before;
    let after = prepared.plan.after;
    if after.actor.group != before.actor.group {
        world.units.group_mut()[prepared.row] = after.actor.group;
    }
    if after.actor.unit_masks != before.actor.unit_masks {
        world
            .units
            .set_unit_masks(prepared.row, after.actor.unit_masks);
    }
    if after.site.build_masks != before.site.build_masks {
        builds[prepared.build_row].build_masks = after.site.build_masks;
    }
    if after.site.recharging != before.site.recharging {
        builds[prepared.build_row].recharging = after.site.recharging;
    }
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
    if after.order.wait != before.order.wait {
        payload.wait = after.order.wait;
    }
    if after.rng_state != before.rng_state {
        world.random.reseed(after.rng_state);
    }
    if let (Some(index), Some(farm_before), Some(farm_after)) = (
        prepared.farm_index,
        prepared.plan.farm_before,
        prepared.plan.farm_after,
    ) {
        let farm = farms
            .get_mut(index)
            .expect("preflight retained the concrete FarmStruct");
        debug_assert_eq!(*farm, farm_before);
        *farm = farm_after;
    }
    if let (Some(guys_before), Some(guys_after)) =
        (&prepared.unit_guys_before, &prepared.unit_guys_after)
    {
        debug_assert_eq!(unit_guys[prepared.row].as_ref(), Some(guys_before));
        unit_guys[prepared.row] = Some(guys_after.clone());
    }
    let farm_cell = prepared.farm_facts.and_then(|facts| {
        prepared.farm_reads.map(|reads| {
            let dx = reads.actor_tx.wrapping_sub(facts.corner_tx) as usize;
            let dy = reads.actor_ty.wrapping_sub(facts.corner_ty) as usize;
            dx * facts.y_size as usize + dy
        })
    });
    let guy_before = prepared
        .unit_guys_before
        .as_ref()
        .and_then(|guys| guys.guys.first())
        .and_then(Option::as_ref);
    let guy_after = prepared
        .unit_guys_after
        .as_ref()
        .and_then(|guys| guys.guys.first())
        .and_then(Option::as_ref);
    let guy_changed_fields = u8::from(guy_before.zip(guy_after).is_some()) * 4;
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
        changed_fields: prepared.plan.changed_fields + guy_changed_fields,
        guy_changed_fields,
        chain_unlinks: prepared.chain.as_ref().map_or(0, |chain| chain.removed),
        rng_draws: prepared.plan.rng_draws,
        farm_index: prepared.farm_index,
        farm_percent_before: farm_cell
            .and_then(|cell| prepared.plan.farm_before.map(|farm| farm.percent[cell])),
        farm_percent_after: farm_cell
            .and_then(|cell| prepared.plan.farm_after.map(|farm| farm.percent[cell])),
        guy_animation_before: guy_before.map(|guy| guy.cur_anim),
        guy_animation_after: guy_after.map(|guy| guy.cur_anim),
        guy_cur_time_before: guy_before.map(|guy| guy.cur_time),
        guy_cur_time_after: guy_after.map(|guy| guy.cur_time),
        guy_end_time_before: guy_before.map(|guy| guy.end_time),
        guy_end_time_after: guy_after.map(|guy| guy.end_time),
        guy_last_time_before: guy_before.map(|guy| guy.last_time),
        guy_last_time_after: guy_after.map(|guy| guy.last_time),
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
