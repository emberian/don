//! Retail construction-site lifecycle transactions.
//!
//! [`super::construction`] owns builder scheduling and construction arithmetic.  This
//! module owns the deterministic local writes and the ordered host-call plans around the
//! four retail bodies which that arithmetic enters:
//!
//! - `Wall::start` `0x0063E810`;
//! - `Object::disband(1)` `0x006455C0` for a rejected unstarted `Build`;
//! - `Wall::activate` `0x0063E4B0`;
//! - `Build::activate` `0x00623E20`, including `Farms::add_animals` `0x008D8F30`.
//!
//! Terrain, city, leader, object-pool and presentation stores are not duplicated here.
//! They are mandatory methods on [`ConstructionLifecycleHost`], in executable order.
//! There are deliberately no default implementations and no flags-only fallback.

use super::construction::{ChecksumEffects, EffectReceipt, ObjectKey};
use super::production::{self, BuildData};

/// This module is an exact transaction seam, not a declaration that every host-side
/// body has been ported.  Keep the public runtime gate closed until the production host
/// supplies every callback and retail-oracle coverage exists.
pub const RUNTIME_FIDELITY_READY: bool = false;

pub const RESOURCE_COUNT: usize = 6;
pub const RESOURCE_XOR: u32 = 0x0000_8221;
pub const WALL_ACTIVE_MASK: u16 = 0x1000;
pub const AIR_DEFENSE_TYPE: i32 = 0x20B;
pub const FARM_TYPE: i32 = 0x1A1;
pub const FARM_PIG_TYPE: i32 = 0x195;
pub const FARM_CHICKEN_TYPE: i32 = 0x196;
pub const FARM_ANIMAL_OWNER: i32 = 9;
pub const FARM_ANIMAL_COUNT: u8 = 5;
pub const FARM_RANDOM_HIGH: u32 = 0xFFFF;
pub const FARM_OFFSET_MODULUS: u32 = 0x180;
pub const FARM_OFFSET_BIAS: i32 = 0xC0;
pub const LEADER_ACTIVATION_DIRTY: u32 = 0x0200_0000 | 0x0800_0000;

/// Integer terrain coordinate.  Retail `TCoord` arithmetic in the recovered bodies is
/// signed 32-bit arithmetic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TCoord {
    pub x: i32,
    pub y: i32,
}

impl TCoord {
    #[inline]
    fn offset(self, dx: i32, dy: i32) -> Self {
        Self {
            x: self.x.wrapping_add(dx),
            y: self.y.wrapping_add(dy),
        }
    }

    #[inline]
    fn half(self) -> Self {
        Self {
            x: self.x >> 1,
            y: self.y >> 1,
        }
    }
}

/// One value plus any transitive mutation/RNG accounting needed to obtain it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueryReceipt<T> {
    pub value: T,
    pub effect: EffectReceipt,
}

/// Exact result of the `Wall::do_construct` admission partition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdmissionReceipt {
    pub raw_code: i32,
    pub accepted: bool,
    pub linked_city_num_wonders: Option<i32>,
    pub wonder_capacity: Option<i32>,
    pub effect: EffectReceipt,
}

/// All non-object inputs read by `Wall::start` and the `Build::start` wonder tail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WallStartPlan {
    pub frame: i32,
    pub corner: TCoord,
    pub x_size: i32,
    pub y_size: i32,
    pub is_wonder: bool,
    /// The actual `Build::start` argument.  `Wall::do_construct` passes one.
    pub notify: i32,
}

/// Exact visibility bytes after `Wall::check_ever_seen` or `update_local_seen`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibilityReceipt {
    pub ever_seen: u8,
    pub ever_seen_completed: u8,
    pub effect: EffectReceipt,
}

/// A host-side close must expose the resulting checksummed flag byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloseReceipt {
    pub flags_after: u8,
    pub effect: EffectReceipt,
}

/// Refund amounts in retail good-index order, food through oil.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RejectedDisbandReceipt {
    pub refunded: [i32; RESOURCE_COUNT],
    pub flags_after: u8,
    pub effect: EffectReceipt,
}

/// Inputs which `BuildData` does not carry as typed fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildActivationPlan {
    pub type_index: i32,
    pub build_flags: i32,
    pub completion_seen_mask: u8,
    pub local_completion_event: bool,
    /// Required only if `site.flags & STARTED == 0`, matching the defensive start inside
    /// `Wall::activate`.  Normal `Wall::do_construct` completion is already started.
    pub start_if_needed: Option<WallStartPlan>,
}

/// The farm record is authoritative for the parent identity and coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FarmParent {
    pub parent: ObjectKey,
    pub x: i32,
    pub y: i32,
}

/// One exact `ObjectsData::add` plan from `Farms::add_animals`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FarmAnimalSpawn {
    pub spawn_owner: i32,
    pub type_index: i32,
    pub x: i32,
    pub y: i32,
    /// Written to `UnitData +0x150`.
    pub parent_o: i16,
    /// Written to `UnitData +0x152`.
    pub parent_who: i16,
    /// Written to `UnitData +0x154`.
    pub ordinal: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FarmAnimalReceipt {
    pub spawned: u8,
    pub effect: EffectReceipt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActivationReceipt {
    pub flags_after: u8,
    pub farm_animals_spawned: u8,
    pub effect: EffectReceipt,
}

/// Fail-loud errors for recovered invariants.  Host errors remain concrete.
#[derive(Debug, PartialEq, Eq)]
pub enum LifecycleError<E> {
    Host(E),
    UnexpectedRng { operation: &'static str, draws: u32 },
    RandomOutOfRange { value: u32, high: u32 },
    MissingStartPlan,
    InvalidFootprint { x_size: i32, y_size: i32 },
    RejectedDisbandLeftValid { flags: u8 },
    FarmParentIndexOutOfRange { who: i32, o: i32 },
}

/// Mandatory external half of the recovered lifecycle.
///
/// Query and ordinary state callbacks in these bodies consume no shared RNG.  Their
/// receipts are checked accordingly.  `kill_competing_at` is the one exception: after
/// removing a local-player competing site, retail may draw once from the shared RNG to
/// choose a presentation sound.  [`shared_random_get`] is the other explicit RNG seam.
pub trait ConstructionLifecycleHost {
    type Error;

    // -- placement admission -----------------------------------------------------------
    fn blocked_site(
        &mut self,
        site_key: ObjectKey,
        site: &BuildData,
    ) -> Result<QueryReceipt<i32>, Self::Error>;

    fn city_num_wonders(
        &mut self,
        owner: i32,
        city: i16,
        include_unbuilt: i32,
    ) -> Result<QueryReceipt<i32>, Self::Error>;

    fn leader_has_tribe_bonus(
        &mut self,
        owner: i32,
        bonus: i32,
    ) -> Result<QueryReceipt<bool>, Self::Error>;

    // -- Wall::start -------------------------------------------------------------------
    fn kill_competing_at(
        &mut self,
        site_key: ObjectKey,
        tile: TCoord,
    ) -> Result<EffectReceipt, Self::Error>;

    fn terrain_object_placed(
        &mut self,
        corner: TCoord,
        x_size: i32,
        y_size: i32,
        placed: i32,
    ) -> Result<EffectReceipt, Self::Error>;

    fn mask_wall(
        &mut self,
        site_key: ObjectKey,
        mask: i32,
        regen_roads: i32,
    ) -> Result<EffectReceipt, Self::Error>;

    fn world_contains_tcoord(&mut self, tile: TCoord) -> Result<QueryReceipt<bool>, Self::Error>;

    fn or_footprint_owner(
        &mut self,
        owner: i32,
        fine_tile: TCoord,
        half_tile: TCoord,
    ) -> Result<EffectReceipt, Self::Error>;

    fn check_ever_seen(
        &mut self,
        site_key: ObjectKey,
        site: &BuildData,
        force: i32,
    ) -> Result<VisibilityReceipt, Self::Error>;

    fn mark_behind_tiles(
        &mut self,
        site_key: ObjectKey,
        set: i32,
    ) -> Result<EffectReceipt, Self::Error>;

    fn update_local_seen(
        &mut self,
        site_key: ObjectKey,
        site: &BuildData,
    ) -> Result<VisibilityReceipt, Self::Error>;

    fn emit_wonder_start_notice(
        &mut self,
        site_key: ObjectKey,
    ) -> Result<EffectReceipt, Self::Error>;

    // -- rejected Build::disband(1) ----------------------------------------------------
    fn close_rejected_build(
        &mut self,
        site_key: ObjectKey,
        site: &mut BuildData,
        type_index: i32,
    ) -> Result<CloseReceipt, Self::Error>;

    fn leader_type_avail(
        &mut self,
        owner: i32,
        type_index: i32,
        mode: i32,
    ) -> Result<QueryReceipt<bool>, Self::Error>;

    fn full_build_cost(
        &mut self,
        owner: i32,
        type_index: i32,
        good: usize,
    ) -> Result<QueryReceipt<i32>, Self::Error>;

    /// Add `amount` through the owner's XOR-`0x8221` resource slot.  Hosts which expose
    /// the slot directly should use [`add_encrypted_resource`].
    fn refund_owner_resource(
        &mut self,
        owner: i32,
        good: usize,
        amount: i32,
    ) -> Result<EffectReceipt, Self::Error>;

    // -- Build::activate / Wall::activate ---------------------------------------------
    /// `Build::new_library` when applicable.  Retail calls this before clearing
    /// `BuildData::recharging`.
    fn activate_before_recharging(
        &mut self,
        site_key: ObjectKey,
        site: &mut BuildData,
        plan: BuildActivationPlan,
    ) -> Result<EffectReceipt, Self::Error>;

    /// Captured/Chinese conversion and the last-built tables, between the recharging
    /// clear and `Wall::activate`.
    fn activate_before_wall(
        &mut self,
        site_key: ObjectKey,
        site: &mut BuildData,
        plan: BuildActivationPlan,
    ) -> Result<EffectReceipt, Self::Error>;

    fn increment_wall_stats(
        &mut self,
        site_key: ObjectKey,
        plan: BuildActivationPlan,
    ) -> Result<EffectReceipt, Self::Error>;

    fn decrement_in_progress_counters(
        &mut self,
        owner: i32,
        type_index: i32,
    ) -> Result<EffectReceipt, Self::Error>;

    fn or_leader_flags(&mut self, owner: i32, flags: u32) -> Result<EffectReceipt, Self::Error>;

    fn emit_local_completion_event(
        &mut self,
        site_key: ObjectKey,
        sound_event: i32,
    ) -> Result<EffectReceipt, Self::Error>;

    fn remove_cover_doober(
        &mut self,
        site_key: ObjectKey,
        type_index: i32,
    ) -> Result<EffectReceipt, Self::Error>;

    /// City/fort/dock/oil/wonder registries, city flags/roads/borders, leader counters,
    /// gather slots/high-water bonuses, training and tech grants through the exact point
    /// where retail calls `Farms::add_animals`.
    fn activate_build_through_gather(
        &mut self,
        site_key: ObjectKey,
        site: &mut BuildData,
        plan: BuildActivationPlan,
    ) -> Result<EffectReceipt, Self::Error>;

    /// Returns `None` exactly when `Farms::add_animals` finds an invalid farm record or
    /// an inactive parent.
    fn farm_parent(
        &mut self,
        owner: i32,
        farm_index: i16,
    ) -> Result<QueryReceipt<Option<FarmParent>>, Self::Error>;

    /// Must be the lockstep `GameAccess::game_random`. Retail `Random::get` treats this
    /// as `[low, high)`: `high` itself is unreachable.
    fn shared_random_get(&mut self, low: u32, high: u32) -> Result<u32, Self::Error>;

    fn spawn_farm_animal(&mut self, spawn: FarmAnimalSpawn) -> Result<EffectReceipt, Self::Error>;

    /// Wonder processing, later type/tribe bonuses, diplomacy visibility, transport
    /// checks, `update_hits`, `update_los`, city upgrade and `update_seen`.
    fn activate_build_after_gather(
        &mut self,
        site_key: ObjectKey,
        site: &mut BuildData,
        plan: BuildActivationPlan,
    ) -> Result<EffectReceipt, Self::Error>;
}

/// `Wall::do_construct`'s exact admitted-code partition, including the `0x2A` linked
/// city wonder-capacity dependency.
pub fn check_site_admission<H: ConstructionLifecycleHost>(
    host: &mut H,
    site_key: ObjectKey,
    site: &BuildData,
) -> Result<AdmissionReceipt, LifecycleError<H::Error>> {
    let mut total = Effects::default();
    let blocked = host
        .blocked_site(site_key, site)
        .map_err(LifecycleError::Host)?;
    add_zero_rng(&mut total, "blocked_site", blocked.effect)?;

    let mut linked_city_num_wonders = None;
    let mut wonder_capacity = None;
    let accepted = match blocked.value {
        0 | 0x27 | 0x28 | 0x29 | 0x2B => true,
        0x2A if site.city >= 0 => {
            let count = host
                .city_num_wonders(site_key.who, site.city, 1)
                .map_err(LifecycleError::Host)?;
            add_zero_rng(&mut total, "CityData::num_wonders", count.effect)?;
            let bonus = host
                .leader_has_tribe_bonus(site_key.who, 7)
                .map_err(LifecycleError::Host)?;
            add_zero_rng(&mut total, "LeaderData::has_tribe_bonus(7)", bonus.effect)?;
            let capacity = 1 + i32::from(bonus.value);
            linked_city_num_wonders = Some(count.value);
            wonder_capacity = Some(capacity);
            count.value <= capacity
        }
        _ => false,
    };

    Ok(AdmissionReceipt {
        raw_code: blocked.value,
        accepted,
        linked_city_num_wonders,
        wonder_capacity,
        effect: total.finish(),
    })
}

/// `Wall::start` followed by the `Build::start` wonder tail.
pub fn start_wall<H: ConstructionLifecycleHost>(
    host: &mut H,
    site_key: ObjectKey,
    site: &mut BuildData,
    plan: WallStartPlan,
) -> Result<EffectReceipt, LifecycleError<H::Error>> {
    let mut total = Effects::default();

    if plan.x_size <= 0 || plan.y_size <= 0 {
        return Err(LifecycleError::InvalidFootprint {
            x_size: plan.x_size,
            y_size: plan.y_size,
        });
    }

    // The first two writes precede every terrain/object call in Wall::start.
    site.flags |= production::flag::STARTED;
    site.frame_started = plan.frame;
    total.checksums = total.checksums.union(builds());

    // Wall::kill_competing_buildings: x outer, y inner.
    for dx in 0..plan.x_size {
        for dy in 0..plan.y_size {
            let effect = host
                .kill_competing_at(site_key, plan.corner.offset(dx, dy))
                .map_err(LifecycleError::Host)?;
            // May spend one shared draw per removed local site for sound selection.
            total.add(effect);
        }
    }

    let effect = host
        .terrain_object_placed(plan.corner, plan.x_size, plan.y_size, 1)
        .map_err(LifecycleError::Host)?;
    add_zero_rng(&mut total, "Terrain::object_placed", effect)?;

    let effect = host
        .mask_wall(site_key, 1, 3)
        .map_err(LifecycleError::Host)?;
    add_zero_rng(&mut total, "Wall::mask_me(1,3)", effect)?;

    // Retail coalesces consecutive fine cells which map to the same half-resolution
    // owner cell.  Out-of-bounds fine cells do not update the remembered half cell.
    let mut previous_half = None;
    for dx in 0..plan.x_size {
        for dy in 0..plan.y_size {
            let fine = plan.corner.offset(dx, dy);
            let inside = host
                .world_contains_tcoord(fine)
                .map_err(LifecycleError::Host)?;
            add_zero_rng(&mut total, "world_contains_tcoord", inside.effect)?;
            if !inside.value {
                continue;
            }
            let half = fine.half();
            if previous_half == Some(half) {
                continue;
            }
            let effect = host
                .or_footprint_owner(site_key.who, fine, half)
                .map_err(LifecycleError::Host)?;
            add_zero_rng(&mut total, "or_footprint_owner", effect)?;
            previous_half = Some(half);
        }
    }

    let seen = host
        .check_ever_seen(site_key, site, 0)
        .map_err(LifecycleError::Host)?;
    add_zero_rng(&mut total, "Wall::check_ever_seen", seen.effect)?;
    site.ever_seen = seen.ever_seen;
    site.ever_seen_completed = seen.ever_seen_completed;

    let effect = host
        .mark_behind_tiles(site_key, 1)
        .map_err(LifecycleError::Host)?;
    add_zero_rng(&mut total, "Wall::mark_behind_tiles", effect)?;

    if plan.is_wonder {
        let seen = host
            .update_local_seen(site_key, site)
            .map_err(LifecycleError::Host)?;
        add_zero_rng(&mut total, "Wall::update_local_seen", seen.effect)?;
        site.ever_seen = seen.ever_seen;
        site.ever_seen_completed = seen.ever_seen_completed;
        if plan.notify != 0 {
            let effect = host
                .emit_wonder_start_notice(site_key)
                .map_err(LifecycleError::Host)?;
            add_zero_rng(&mut total, "wonder start presentation", effect)?;
        }
    }

    total.checksums = total.checksums.union(builds()).union(world());
    Ok(total.finish())
}

/// Exact rejected-unstarted-Build specialization of `Object::disband(1)`.
///
/// The object/type cleanup occurs first.  Retail then visits all six good indices in
/// ascending order, asks `LeaderData::type_avail(good,1)`, recomputes full type cost, and
/// adds it to the owner's XOR-`0x8221` resource total.
pub fn disband_rejected_build<H: ConstructionLifecycleHost>(
    host: &mut H,
    site_key: ObjectKey,
    site: &mut BuildData,
    type_index: i32,
) -> Result<RejectedDisbandReceipt, LifecycleError<H::Error>> {
    let mut total = Effects::default();
    let close = host
        .close_rejected_build(site_key, site, type_index)
        .map_err(LifecycleError::Host)?;
    add_zero_rng(&mut total, "rejected Build close", close.effect)?;
    site.flags = close.flags_after;
    if site.flags & production::flag::VALID != 0 {
        return Err(LifecycleError::RejectedDisbandLeftValid { flags: site.flags });
    }

    let mut refunded = [0; RESOURCE_COUNT];
    for good in 0..RESOURCE_COUNT {
        let has = host
            .leader_type_avail(site_key.who, good as i32, 1)
            .map_err(LifecycleError::Host)?;
        add_zero_rng(&mut total, "LeaderData::type_avail", has.effect)?;
        if !has.value {
            continue;
        }
        let cost = host
            .full_build_cost(site_key.who, type_index, good)
            .map_err(LifecycleError::Host)?;
        add_zero_rng(&mut total, "full BuildType cost", cost.effect)?;
        refunded[good] = cost.value;
        if cost.value == 0 {
            continue;
        }
        let effect = host
            .refund_owner_resource(site_key.who, good, cost.value)
            .map_err(LifecycleError::Host)?;
        add_zero_rng(&mut total, "leader resource refund", effect)?;
    }

    total.checksums = total
        .checksums
        .union(builds())
        .union(leaders())
        .union(world());
    Ok(RejectedDisbandReceipt {
        refunded,
        flags_after: site.flags,
        effect: total.finish(),
    })
}

/// Add a signed retail amount to one encrypted leader resource slot.
#[inline]
pub fn add_encrypted_resource(slot: &mut u32, amount: i32) {
    let plain = *slot ^ RESOURCE_XOR;
    *slot = plain.wrapping_add(amount as u32) ^ RESOURCE_XOR;
}

/// Normal construction completion: `Build::activate(0,1,1)`.
pub fn activate_build<H: ConstructionLifecycleHost>(
    host: &mut H,
    site_key: ObjectKey,
    site: &mut BuildData,
    plan: BuildActivationPlan,
) -> Result<ActivationReceipt, LifecycleError<H::Error>> {
    let mut total = Effects::default();

    let effect = host
        .activate_before_recharging(site_key, site, plan)
        .map_err(LifecycleError::Host)?;
    add_zero_rng(&mut total, "Build::new_library phase", effect)?;

    site.recharging = 0;
    total.checksums = total.checksums.union(builds());

    let effect = host
        .activate_before_wall(site_key, site, plan)
        .map_err(LifecycleError::Host)?;
    add_zero_rng(&mut total, "Build pre-Wall activation", effect)?;

    // Wall::activate writes completion visibility before its defensive Wall::start.
    site.ever_seen_completed |= plan.completion_seen_mask;
    if !site.is_started() {
        let start = plan
            .start_if_needed
            .ok_or(LifecycleError::MissingStartPlan)?;
        let effect = start_wall(host, site_key, site, start)?;
        total.add(effect);
    }

    site.flags |= production::flag::ACTIVE;
    site.build_masks |= WALL_ACTIVE_MASK;
    site.job_counter = 0;
    site.job_counter_2 = 0;
    if plan.type_index == AIR_DEFENSE_TYPE {
        site.job_counter = 0x4000_0000;
    }
    total.checksums = total.checksums.union(builds());

    // Build's vtable +0x20 is the folded-true `BuildData::is_build`; the second arm is
    // therefore any Build linked to a city, not only a city-center type.
    if plan.build_flags & 0x10 != 0 || site.city >= 0 {
        let effect = host
            .increment_wall_stats(site_key, plan)
            .map_err(LifecycleError::Host)?;
        add_zero_rng(&mut total, "Wall::increment_stats", effect)?;
    }

    let effect = host
        .decrement_in_progress_counters(site_key.who, plan.type_index)
        .map_err(LifecycleError::Host)?;
    add_zero_rng(&mut total, "in-progress counters", effect)?;

    let effect = host
        .or_leader_flags(site_key.who, LEADER_ACTIVATION_DIRTY)
        .map_err(LifecycleError::Host)?;
    add_zero_rng(&mut total, "leader activation flags", effect)?;

    // Wall::activate calls Object::update_seen only when virtual +0x20 (`is_build`) is
    // false.  This entry point is Build::activate, so that generic-Wall arm is skipped.

    if plan.local_completion_event {
        let effect = host
            .emit_local_completion_event(site_key, 8)
            .map_err(LifecycleError::Host)?;
        add_zero_rng(&mut total, "local completion presentation", effect)?;
    }

    let effect = host
        .remove_cover_doober(site_key, plan.type_index)
        .map_err(LifecycleError::Host)?;
    add_zero_rng(&mut total, "Doober::remove_cover_doober", effect)?;

    let effect = host
        .activate_build_through_gather(site_key, site, plan)
        .map_err(LifecycleError::Host)?;
    add_zero_rng(&mut total, "Build activation through gather", effect)?;

    let mut farm_animals_spawned = 0;
    if plan.type_index == FARM_TYPE {
        let animals = add_farm_animals(host, site_key.who, site.dock)?;
        farm_animals_spawned = animals.spawned;
        total.add(animals.effect);
    }

    let effect = host
        .activate_build_after_gather(site_key, site, plan)
        .map_err(LifecycleError::Host)?;
    add_zero_rng(&mut total, "Build activation after gather", effect)?;

    total.checksums = total.checksums.union(builds()).union(leaders());
    Ok(ActivationReceipt {
        flags_after: site.flags,
        farm_animals_spawned,
        effect: total.finish(),
    })
}

/// `Farms::add_animals` `0x008D8F30`.
pub fn add_farm_animals<H: ConstructionLifecycleHost>(
    host: &mut H,
    owner: i32,
    farm_index: i16,
) -> Result<FarmAnimalReceipt, LifecycleError<H::Error>> {
    let mut total = Effects::default();
    let parent = host
        .farm_parent(owner, farm_index)
        .map_err(LifecycleError::Host)?;
    add_zero_rng(&mut total, "Farms parent lookup", parent.effect)?;
    let Some(parent) = parent.value else {
        return Ok(FarmAnimalReceipt {
            spawned: 0,
            effect: total.finish(),
        });
    };
    let parent_who = i16::try_from(parent.parent.who).map_err(|_| {
        LifecycleError::FarmParentIndexOutOfRange {
            who: parent.parent.who,
            o: parent.parent.o,
        }
    })?;
    let parent_o =
        i16::try_from(parent.parent.o).map_err(|_| LifecycleError::FarmParentIndexOutOfRange {
            who: parent.parent.who,
            o: parent.parent.o,
        })?;

    for ordinal in 0..FARM_ANIMAL_COUNT {
        // Draw order is species, y, x.  Both species branches perform the latter two
        // draws in the same order.
        let species = checked_random(host, FARM_RANDOM_HIGH)?;
        let y_draw = checked_random(host, FARM_RANDOM_HIGH)?;
        let x_draw = checked_random(host, FARM_RANDOM_HIGH)?;
        total.rng_draws = total.rng_draws.wrapping_add(3);

        let type_index = if species & 0x8000_0001 == 0 {
            FARM_CHICKEN_TYPE
        } else {
            FARM_PIG_TYPE
        };
        let y = parent
            .y
            .wrapping_add((y_draw % FARM_OFFSET_MODULUS) as i32)
            .wrapping_sub(FARM_OFFSET_BIAS);
        let x = parent
            .x
            .wrapping_add((x_draw % FARM_OFFSET_MODULUS) as i32)
            .wrapping_sub(FARM_OFFSET_BIAS);
        let effect = host
            .spawn_farm_animal(FarmAnimalSpawn {
                spawn_owner: FARM_ANIMAL_OWNER,
                type_index,
                x,
                y,
                parent_o,
                parent_who,
                ordinal,
            })
            .map_err(LifecycleError::Host)?;
        add_zero_rng(&mut total, "farm animal spawn", effect)?;
    }

    total.checksums = total.checksums.union(units_and_objects());
    Ok(FarmAnimalReceipt {
        spawned: FARM_ANIMAL_COUNT,
        effect: total.finish(),
    })
}

fn checked_random<H: ConstructionLifecycleHost>(
    host: &mut H,
    high: u32,
) -> Result<u32, LifecycleError<H::Error>> {
    let value = host
        .shared_random_get(0, high)
        .map_err(LifecycleError::Host)?;
    if value >= high {
        return Err(LifecycleError::RandomOutOfRange { value, high });
    }
    Ok(value)
}

#[derive(Clone, Copy, Debug)]
struct Effects {
    rng_draws: u32,
    checksums: ChecksumEffects,
}

impl Default for Effects {
    fn default() -> Self {
        Self {
            rng_draws: 0,
            checksums: ChecksumEffects::NONE,
        }
    }
}

impl Effects {
    fn add(&mut self, effect: EffectReceipt) {
        self.rng_draws = self.rng_draws.wrapping_add(effect.rng_draws);
        self.checksums = self.checksums.union(effect.checksums);
    }

    fn finish(self) -> EffectReceipt {
        EffectReceipt {
            rng_draws: self.rng_draws,
            checksums: self.checksums,
        }
    }
}

fn add_zero_rng<E>(
    total: &mut Effects,
    operation: &'static str,
    effect: EffectReceipt,
) -> Result<(), LifecycleError<E>> {
    if effect.rng_draws != 0 {
        return Err(LifecycleError::UnexpectedRng {
            operation,
            draws: effect.rng_draws,
        });
    }
    total.add(effect);
    Ok(())
}

const fn builds() -> ChecksumEffects {
    ChecksumEffects {
        builds: true,
        units: false,
        guys: false,
        leaders: false,
        cities: false,
        groups: false,
        world: false,
        objects_other: false,
    }
}

const fn leaders() -> ChecksumEffects {
    ChecksumEffects {
        leaders: true,
        ..ChecksumEffects::NONE
    }
}

const fn world() -> ChecksumEffects {
    ChecksumEffects {
        world: true,
        ..ChecksumEffects::NONE
    }
}

const fn units_and_objects() -> ChecksumEffects {
    ChecksumEffects {
        units: true,
        objects_other: true,
        ..ChecksumEffects::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SITE: ObjectKey = ObjectKey {
        who: 2,
        o: 2004,
        uid: 77,
    };

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Call {
        Blocked,
        NumWonders(i16),
        Bonus(i32),
        Kill(TCoord),
        Placed,
        Mask,
        Owner(TCoord),
        Seen,
        Behind,
        Close,
        Refund(usize, i32),
        PreRecharge,
        PreWall,
        Decrement,
        LeaderFlags,
        Doober,
        ThroughGather,
        Spawn(FarmAnimalSpawn),
        AfterGather,
    }

    struct Host {
        blocked: i32,
        num_wonders: i32,
        bonus: bool,
        dimensions: TCoord,
        resources: [u32; RESOURCE_COUNT],
        costs: [i32; RESOURCE_COUNT],
        farm_parent: Option<FarmParent>,
        random: std::collections::VecDeque<u32>,
        calls: Vec<Call>,
    }

    impl Default for Host {
        fn default() -> Self {
            Self {
                blocked: 0,
                num_wonders: 0,
                bonus: false,
                dimensions: TCoord { x: 64, y: 64 },
                resources: [RESOURCE_XOR; RESOURCE_COUNT],
                costs: [10, 20, 30, 40, 50, 60],
                farm_parent: None,
                random: std::collections::VecDeque::new(),
                calls: Vec::new(),
            }
        }
    }

    fn none() -> EffectReceipt {
        EffectReceipt {
            rng_draws: 0,
            checksums: ChecksumEffects::NONE,
        }
    }

    impl ConstructionLifecycleHost for Host {
        type Error = &'static str;

        fn blocked_site(
            &mut self,
            _site_key: ObjectKey,
            _site: &BuildData,
        ) -> Result<QueryReceipt<i32>, Self::Error> {
            self.calls.push(Call::Blocked);
            Ok(QueryReceipt {
                value: self.blocked,
                effect: none(),
            })
        }

        fn city_num_wonders(
            &mut self,
            _owner: i32,
            city: i16,
            include_unbuilt: i32,
        ) -> Result<QueryReceipt<i32>, Self::Error> {
            assert_eq!(include_unbuilt, 1);
            self.calls.push(Call::NumWonders(city));
            Ok(QueryReceipt {
                value: self.num_wonders,
                effect: none(),
            })
        }

        fn leader_has_tribe_bonus(
            &mut self,
            _owner: i32,
            bonus: i32,
        ) -> Result<QueryReceipt<bool>, Self::Error> {
            self.calls.push(Call::Bonus(bonus));
            Ok(QueryReceipt {
                value: self.bonus,
                effect: none(),
            })
        }

        fn kill_competing_at(
            &mut self,
            _site_key: ObjectKey,
            tile: TCoord,
        ) -> Result<EffectReceipt, Self::Error> {
            self.calls.push(Call::Kill(tile));
            Ok(none())
        }

        fn terrain_object_placed(
            &mut self,
            _corner: TCoord,
            _x_size: i32,
            _y_size: i32,
            placed: i32,
        ) -> Result<EffectReceipt, Self::Error> {
            assert_eq!(placed, 1);
            self.calls.push(Call::Placed);
            Ok(none())
        }

        fn mask_wall(
            &mut self,
            _site_key: ObjectKey,
            mask: i32,
            regen_roads: i32,
        ) -> Result<EffectReceipt, Self::Error> {
            assert_eq!((mask, regen_roads), (1, 3));
            self.calls.push(Call::Mask);
            Ok(none())
        }

        fn world_contains_tcoord(
            &mut self,
            tile: TCoord,
        ) -> Result<QueryReceipt<bool>, Self::Error> {
            Ok(QueryReceipt {
                value: tile.x >= 0
                    && tile.y >= 0
                    && tile.x < self.dimensions.x
                    && tile.y < self.dimensions.y,
                effect: none(),
            })
        }

        fn or_footprint_owner(
            &mut self,
            _owner: i32,
            _fine_tile: TCoord,
            half_tile: TCoord,
        ) -> Result<EffectReceipt, Self::Error> {
            self.calls.push(Call::Owner(half_tile));
            Ok(none())
        }

        fn check_ever_seen(
            &mut self,
            _site_key: ObjectKey,
            site: &BuildData,
            _force: i32,
        ) -> Result<VisibilityReceipt, Self::Error> {
            self.calls.push(Call::Seen);
            Ok(VisibilityReceipt {
                ever_seen: site.ever_seen | 4,
                ever_seen_completed: site.ever_seen_completed,
                effect: none(),
            })
        }

        fn mark_behind_tiles(
            &mut self,
            _site_key: ObjectKey,
            _set: i32,
        ) -> Result<EffectReceipt, Self::Error> {
            self.calls.push(Call::Behind);
            Ok(none())
        }

        fn update_local_seen(
            &mut self,
            _site_key: ObjectKey,
            site: &BuildData,
        ) -> Result<VisibilityReceipt, Self::Error> {
            Ok(VisibilityReceipt {
                ever_seen: site.ever_seen | 2,
                ever_seen_completed: site.ever_seen_completed,
                effect: none(),
            })
        }

        fn emit_wonder_start_notice(
            &mut self,
            _site_key: ObjectKey,
        ) -> Result<EffectReceipt, Self::Error> {
            Ok(none())
        }

        fn close_rejected_build(
            &mut self,
            _site_key: ObjectKey,
            site: &mut BuildData,
            _type_index: i32,
        ) -> Result<CloseReceipt, Self::Error> {
            self.calls.push(Call::Close);
            site.flags = 0;
            Ok(CloseReceipt {
                flags_after: 0,
                effect: none(),
            })
        }

        fn leader_type_avail(
            &mut self,
            _owner: i32,
            _type_index: i32,
            _mode: i32,
        ) -> Result<QueryReceipt<bool>, Self::Error> {
            Ok(QueryReceipt {
                value: true,
                effect: none(),
            })
        }

        fn full_build_cost(
            &mut self,
            _owner: i32,
            _type_index: i32,
            good: usize,
        ) -> Result<QueryReceipt<i32>, Self::Error> {
            Ok(QueryReceipt {
                value: self.costs[good],
                effect: none(),
            })
        }

        fn refund_owner_resource(
            &mut self,
            _owner: i32,
            good: usize,
            amount: i32,
        ) -> Result<EffectReceipt, Self::Error> {
            self.calls.push(Call::Refund(good, amount));
            add_encrypted_resource(&mut self.resources[good], amount);
            Ok(none())
        }

        fn activate_before_recharging(
            &mut self,
            _site_key: ObjectKey,
            _site: &mut BuildData,
            _plan: BuildActivationPlan,
        ) -> Result<EffectReceipt, Self::Error> {
            self.calls.push(Call::PreRecharge);
            Ok(none())
        }

        fn activate_before_wall(
            &mut self,
            _site_key: ObjectKey,
            _site: &mut BuildData,
            _plan: BuildActivationPlan,
        ) -> Result<EffectReceipt, Self::Error> {
            self.calls.push(Call::PreWall);
            Ok(none())
        }

        fn increment_wall_stats(
            &mut self,
            _site_key: ObjectKey,
            _plan: BuildActivationPlan,
        ) -> Result<EffectReceipt, Self::Error> {
            Ok(none())
        }

        fn decrement_in_progress_counters(
            &mut self,
            _owner: i32,
            _type_index: i32,
        ) -> Result<EffectReceipt, Self::Error> {
            self.calls.push(Call::Decrement);
            Ok(none())
        }

        fn or_leader_flags(
            &mut self,
            _owner: i32,
            flags: u32,
        ) -> Result<EffectReceipt, Self::Error> {
            assert_eq!(flags, LEADER_ACTIVATION_DIRTY);
            self.calls.push(Call::LeaderFlags);
            Ok(none())
        }

        fn emit_local_completion_event(
            &mut self,
            _site_key: ObjectKey,
            _sound_event: i32,
        ) -> Result<EffectReceipt, Self::Error> {
            Ok(none())
        }

        fn remove_cover_doober(
            &mut self,
            _site_key: ObjectKey,
            _type_index: i32,
        ) -> Result<EffectReceipt, Self::Error> {
            self.calls.push(Call::Doober);
            Ok(none())
        }

        fn activate_build_through_gather(
            &mut self,
            _site_key: ObjectKey,
            _site: &mut BuildData,
            _plan: BuildActivationPlan,
        ) -> Result<EffectReceipt, Self::Error> {
            self.calls.push(Call::ThroughGather);
            Ok(none())
        }

        fn farm_parent(
            &mut self,
            _owner: i32,
            _farm_index: i16,
        ) -> Result<QueryReceipt<Option<FarmParent>>, Self::Error> {
            Ok(QueryReceipt {
                value: self.farm_parent,
                effect: none(),
            })
        }

        fn shared_random_get(&mut self, low: u32, high: u32) -> Result<u32, Self::Error> {
            assert_eq!((low, high), (0, FARM_RANDOM_HIGH));
            self.random.pop_front().ok_or("random exhausted")
        }

        fn spawn_farm_animal(
            &mut self,
            spawn: FarmAnimalSpawn,
        ) -> Result<EffectReceipt, Self::Error> {
            self.calls.push(Call::Spawn(spawn));
            Ok(none())
        }

        fn activate_build_after_gather(
            &mut self,
            _site_key: ObjectKey,
            _site: &mut BuildData,
            _plan: BuildActivationPlan,
        ) -> Result<EffectReceipt, Self::Error> {
            self.calls.push(Call::AfterGather);
            Ok(none())
        }
    }

    fn site() -> BuildData {
        BuildData {
            flags: production::flag::VALID,
            who: SITE.who as u8,
            uid: SITE.uid,
            city: -1,
            dock: 3,
            ..BuildData::default()
        }
    }

    #[test]
    fn admission_uses_inclusive_linked_city_wonder_capacity() {
        let mut host = Host {
            blocked: 0x2A,
            num_wonders: 2,
            bonus: true,
            ..Host::default()
        };
        let mut build = site();
        build.city = 4;
        let receipt = check_site_admission(&mut host, SITE, &build).unwrap();
        assert!(receipt.accepted); // 2 <= 1 + true
        assert_eq!(receipt.linked_city_num_wonders, Some(2));
        assert_eq!(receipt.wonder_capacity, Some(2));

        host.num_wonders = 3;
        assert!(
            !check_site_admission(&mut host, SITE, &build)
                .unwrap()
                .accepted
        );
    }

    #[test]
    fn start_writes_local_fields_and_preserves_retail_footprint_order() {
        let mut host = Host::default();
        let mut build = site();
        build.job_counter = 17;
        build.job_counter_2 = 19;
        let receipt = start_wall(
            &mut host,
            SITE,
            &mut build,
            WallStartPlan {
                frame: 1234,
                corner: TCoord { x: 2, y: 2 },
                x_size: 2,
                y_size: 3,
                is_wonder: false,
                notify: 1,
            },
        )
        .unwrap();
        assert!(build.is_started());
        assert_eq!(build.frame_started, 1234);
        assert_eq!((build.job_counter, build.job_counter_2), (17, 19));
        assert_eq!(receipt.rng_draws, 0);
        assert_eq!(
            host.calls[..6],
            [
                Call::Kill(TCoord { x: 2, y: 2 }),
                Call::Kill(TCoord { x: 2, y: 3 }),
                Call::Kill(TCoord { x: 2, y: 4 }),
                Call::Kill(TCoord { x: 3, y: 2 }),
                Call::Kill(TCoord { x: 3, y: 3 }),
                Call::Kill(TCoord { x: 3, y: 4 }),
            ]
        );
        let owners: Vec<_> = host
            .calls
            .iter()
            .filter_map(|call| match call {
                Call::Owner(tile) => Some(*tile),
                _ => None,
            })
            .collect();
        assert_eq!(
            owners,
            [
                TCoord { x: 1, y: 1 },
                TCoord { x: 1, y: 2 },
                TCoord { x: 1, y: 1 },
                TCoord { x: 1, y: 2 },
            ]
        );
    }

    #[test]
    fn invalid_footprint_fails_before_started_or_world_writes() {
        let mut host = Host::default();
        let mut build = site();
        let before = build.image();
        assert!(matches!(
            start_wall(
                &mut host,
                SITE,
                &mut build,
                WallStartPlan {
                    frame: 1,
                    corner: TCoord::default(),
                    x_size: 0,
                    y_size: 3,
                    is_wonder: false,
                    notify: 1,
                },
            ),
            Err(LifecycleError::InvalidFootprint {
                x_size: 0,
                y_size: 3
            })
        ));
        assert_eq!(build.image(), before);
        assert!(host.calls.is_empty());
    }

    #[test]
    fn rejected_disband_refunds_all_six_encrypted_resource_slots() {
        let mut host = Host::default();
        let mut build = site();
        let receipt = disband_rejected_build(&mut host, SITE, &mut build, 0x1A7).unwrap();
        assert_eq!(receipt.refunded, host.costs);
        assert_eq!(receipt.flags_after, 0);
        for (encrypted, expected) in host.resources.iter().zip(host.costs) {
            assert_eq!(*encrypted ^ RESOURCE_XOR, expected as u32);
        }
        assert!(receipt.effect.checksums.builds);
        assert!(receipt.effect.checksums.leaders);
        assert!(receipt.effect.checksums.world);
    }

    #[test]
    fn rejected_disband_skips_zero_cost_resource_write() {
        let mut host = Host::default();
        host.costs[3] = 0;
        let mut build = site();
        let receipt = disband_rejected_build(&mut host, SITE, &mut build, 0x1A7).unwrap();
        assert_eq!(receipt.refunded[3], 0);
        assert!(!host.calls.contains(&Call::Refund(3, 0)));
    }

    #[test]
    fn farm_activation_resets_both_counters_and_spends_fifteen_draws() {
        let parent = FarmParent {
            parent: SITE,
            x: 1000,
            y: 2000,
        };
        let mut draws = std::collections::VecDeque::new();
        for ordinal in 0..FARM_ANIMAL_COUNT {
            // Even species draw -> chicken, then y and x.
            draws.push_back(if ordinal & 1 == 0 { 0 } else { 1 });
            draws.push_back(ordinal as u32);
            draws.push_back(ordinal as u32 + 10);
        }
        let mut host = Host {
            farm_parent: Some(parent),
            random: draws,
            ..Host::default()
        };
        let mut build = site();
        build.flags |= production::flag::STARTED;
        build.job_counter = 500;
        build.job_counter_2 = 501;
        build.recharging = 7;
        let receipt = activate_build(
            &mut host,
            SITE,
            &mut build,
            BuildActivationPlan {
                type_index: FARM_TYPE,
                build_flags: 0,
                completion_seen_mask: 4,
                local_completion_event: false,
                start_if_needed: None,
            },
        )
        .unwrap();
        assert_eq!(receipt.effect.rng_draws, 15);
        assert_eq!(receipt.farm_animals_spawned, 5);
        assert_eq!((build.job_counter, build.job_counter_2), (0, 0));
        assert_eq!(build.recharging, 0);
        assert_ne!(build.flags & production::flag::ACTIVE, 0);
        assert_ne!(build.build_masks & WALL_ACTIVE_MASK, 0);
        assert_eq!(build.ever_seen_completed & 4, 4);

        let spawns: Vec<_> = host
            .calls
            .iter()
            .filter_map(|call| match call {
                Call::Spawn(spawn) => Some(*spawn),
                _ => None,
            })
            .collect();
        assert_eq!(spawns.len(), 5);
        assert_eq!(spawns[0].type_index, FARM_CHICKEN_TYPE);
        assert_eq!(spawns[1].type_index, FARM_PIG_TYPE);
        assert_eq!((spawns[0].x, spawns[0].y), (818, 1808));
        assert_eq!((spawns[0].parent_o, spawns[0].parent_who), (2004, 2));
        assert_eq!(spawns[4].ordinal, 4);
    }

    #[test]
    fn retail_random_upper_bound_is_exclusive() {
        let mut host = Host {
            farm_parent: Some(FarmParent {
                parent: SITE,
                x: 0,
                y: 0,
            }),
            random: [FARM_RANDOM_HIGH].into_iter().collect(),
            ..Host::default()
        };
        assert!(matches!(
            add_farm_animals(&mut host, SITE.who, 3),
            Err(LifecycleError::RandomOutOfRange {
                value: FARM_RANDOM_HIGH,
                high: FARM_RANDOM_HIGH
            })
        ));
    }

    #[test]
    fn air_defense_rearms_primary_counter_after_reset() {
        let mut host = Host::default();
        let mut build = site();
        build.flags |= production::flag::STARTED;
        build.job_counter = 9;
        build.job_counter_2 = 10;
        activate_build(
            &mut host,
            SITE,
            &mut build,
            BuildActivationPlan {
                type_index: AIR_DEFENSE_TYPE,
                build_flags: 0,
                completion_seen_mask: 0,
                local_completion_event: false,
                start_if_needed: None,
            },
        )
        .unwrap();
        assert_eq!(build.job_counter, 0x4000_0000);
        assert_eq!(build.job_counter_2, 0);
    }
}
