//! Retail gathering-site state and the worker/site adapter contract.
//!
//! This module owns the seam between map-derived gathering capacity and consumers such as
//! the playable arena. It deliberately does **not** map a terrain label to a slot count.
//! Retail obtains `BuildData::gather_max` by calling
//! `BuildTypeData::max_gatherers` (`0x0063C430`), which in turn runs the 7,668-byte
//! `BuildTypeData::calc_gather` (`0x00639E40`) over the real world cells, mountain/cliff
//! objects, ownership/diplomacy gates, player properties, and the building's `MiningList`.
//! A caller must therefore supply that evaluator's result; a guessed Farm/Camp/Mine table
//! is not an accepted input.
//!
//! What is complete here is the persistent site state on either side of that evaluator:
//!
//! * `BuildData::gather_max` is a signed byte at `+0x80`.
//! * `BuildData::gather_down` at `+0x70` heads an owner-local chain of unit object indices.
//! * every linked `UnitData::gather_down` is a signed short at `+0x92`.
//! * `Build::add_gatherer` (`0x0062F640`), `check_gatherers` (`0x0062F710`) and
//!   `remove_gatherer` (`0x0062F8D0`) mutate that chain.
//! * active membership is decided by `UnitData::is_gathering_at` (`0x00608880`), including
//!   the `GatherOrder::been_there` byte.
//!
//! Structure and transitions are read from the named retail functions and the matching PDB.
//! They are Tier C: they have not been run differentially against retail.

use crate::mechanics::{credit_resource, resource_period};

use super::economy::{self, EconRules, NUM_RESOURCES};
use super::map_terrain::World;

/// Retail's null owner-local object link.
pub const NO_OBJECT: i16 = -1;
/// `BuildTypeData::max_flat_gatherers` (`0x006365F0`).
pub const MAX_FLAT_GATHERERS: i32 = 1;
/// `BuildTypeData::max_knowledge_gatherers` (`0x006365E0`).
pub const MAX_KNOWLEDGE_GATHERERS: i32 = 7;

/// One fine-cell coordinate returned by the authoritative gathering-terrain search.
/// `BuildData::gather_from` stores these pairs; this type does not decide which cells qualify.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherTile {
    pub tx: i32,
    pub ty: i32,
}

/// Read the retail `WorldData::is_gathered_from` reservation bit (`0x1000`).
#[inline]
pub fn gather_tile_reserved(world: &World, tile: GatherTile) -> Option<bool> {
    world
        .valid_t(tile.tx, tile.ty)
        .then(|| world.is_gathered_from(tile.tx, tile.ty))
}

/// Apply `World::set_gathered_at` to coordinates already selected by the authoritative
/// type/terrain evaluator. Bounds are checked for the whole list before any mutation.
/// The bit is a site reservation/claim, not a resource-depletion amount.
pub fn set_gather_tiles_reserved(
    world: &mut World,
    tiles: &[GatherTile],
    reserved: bool,
) -> Result<(), GatherTile> {
    if let Some(invalid) = tiles
        .iter()
        .copied()
        .find(|tile| !world.valid_t(tile.tx, tile.ty))
    {
        return Err(invalid);
    }
    for tile in tiles {
        world.set_gathered_at(tile.tx, tile.ty, reserved);
    }
    Ok(())
}

/// The four worker TypeIndices accepted by `Build::add_gatherer` and
/// `UnitData::is_gathering_at` (`0x32..=0x35`).
#[inline]
pub fn is_building_gatherer_type(type_index: i32) -> bool {
    matches!(type_index, 0x32..=0x35)
}

/// The subset of a live GatherOrder needed to decide site membership.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherAssignment {
    /// `TargetOrder::whom` — owner of the target object.
    pub target_owner: i32,
    /// `TargetOrder::ox` — owner-local target object index.
    pub target_build: i32,
    /// `TargetOrder::uid`; `target_exists` uses it to reject a recycled object slot.
    pub target_uid: u16,
    /// `GatherOrder::been_there` (`+0x27`). `num_gatherers(1, 1)` requires it.
    pub been_there: bool,
    /// Types `0x34` and `0x35` can count while physically inside the target even before
    /// consulting the current order. `None` means the worker is not inside a building.
    pub inside_target: Option<(u8, i16)>,
}

impl GatherAssignment {
    /// `TargetOrder::target_exists` (`0x0072FF10`) identity check after the live-bit gate.
    #[inline]
    pub fn targets_live_site(self, site: &GatherSite) -> bool {
        self.target_owner == i32::from(site.owner)
            && self.target_build == i32::from(site.build_o)
            && self.target_uid == site.uid
    }
}

/// Persistent gathering fields from one `UnitData` row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherWorker {
    pub owner: u8,
    pub unit_o: i16,
    pub type_index: i32,
    /// The object still passes the retail validity/unit virtual gates.
    pub valid_unit: bool,
    pub assignment: Option<GatherAssignment>,
    /// `UnitData::gather_down` (`+0x92`).
    pub gather_down: i16,
    /// `UnitData::good_obj` (`+0x94`), the nearby good selected by
    /// `UnitData::calc_gather`; `-1` when no good is selected.
    pub good_obj: i16,
}

impl GatherWorker {
    pub fn new(owner: u8, unit_o: i16, type_index: i32) -> Self {
        Self {
            owner,
            unit_o,
            type_index,
            valid_unit: true,
            assignment: None,
            gather_down: NO_OBJECT,
            good_obj: NO_OBJECT,
        }
    }

    /// `UnitData::is_gathering_at` (`0x00608880`) reduced to its persistent inputs.
    pub fn is_gathering_at(&self, site: &GatherSite, require_been_there: bool) -> bool {
        if !self.valid_unit
            || self.owner != site.owner
            || !is_building_gatherer_type(self.type_index)
        {
            return false;
        }
        let Some(a) = self.assignment else {
            return false;
        };
        if matches!(self.type_index, 0x34 | 0x35)
            && a.inside_target == Some((site.owner, site.build_o))
            && !require_been_there
        {
            return true;
        }
        a.target_owner == i32::from(site.owner)
            && a.target_build == i32::from(site.build_o)
            && (!require_been_there || a.been_there)
    }
}

/// Persistent gathering fields from one `BuildData` row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherSite {
    pub owner: u8,
    pub build_o: i16,
    /// Object generation captured into new `GatherOrder::uid` values.
    pub uid: u16,
    /// `BuildData::gather_max` (`+0x80`), sign-extended by `max_gatherers`.
    pub gather_max: i8,
    /// `BuildData::gather_down` (`+0x70`).
    pub gather_down: i16,
}

impl GatherSite {
    pub fn new(owner: u8, build_o: i16) -> Self {
        Self {
            owner,
            build_o,
            uid: 0,
            gather_max: 0,
            gather_down: NO_OBJECT,
        }
    }

    /// Store the result returned by retail `BuildTypeData::max_gatherers`.
    ///
    /// That function clamps a negative computed capacity to zero; `Build::update_max_gatherers`
    /// (`0x00623310`) then stores only `AL`. The signed-byte round trip is intentional.
    pub fn set_authoritative_capacity(&mut self, computed: i32) {
        self.gather_max = computed.max(0) as u8 as i8;
    }

    #[inline]
    pub fn max_gatherers(&self) -> i32 {
        i32::from(self.gather_max)
    }
}

/// Which retail count the caller needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatherCount {
    /// `num_gatherers(0, 0)`: order targets the site, whether or not the worker arrived.
    Assigned,
    /// `num_gatherers(1, 1)`: requires `GatherOrder::been_there`.
    Active,
}

/// Result required by arena and command adapters. Retail's `add_gatherer` returns void;
/// these distinctions expose why it made no mutation without inventing another transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachResult {
    Attached,
    Full,
    AlreadyAttached,
    Invalid,
}

fn worker_pos(workers: &[GatherWorker], owner: u8, unit_o: i16) -> Option<usize> {
    workers
        .iter()
        .position(|w| w.owner == owner && w.unit_o == unit_o)
}

fn set_link(
    site: &mut GatherSite,
    workers: &mut [GatherWorker],
    previous: Option<usize>,
    next: i16,
) {
    if let Some(previous) = previous {
        workers[previous].gather_down = next;
    } else {
        site.gather_down = next;
    }
}

/// `Build::check_gatherers` (`0x0062F710`): unlink workers that no longer have a valid
/// gather order for this site. A corrupt/missing owner-local link fails rather than being
/// silently repaired; retail assumes its object table is internally valid too.
pub fn check_gatherers(
    site: &mut GatherSite,
    workers: &mut [GatherWorker],
) -> Result<usize, &'static str> {
    let mut previous = None;
    let mut current = site.gather_down;
    let mut visited = 0usize;
    let mut removed = 0usize;
    while current >= 0 {
        if visited >= workers.len() {
            return Err("gather chain contains a cycle");
        }
        visited += 1;
        let Some(pos) = worker_pos(workers, site.owner, current) else {
            return Err("gather chain references a missing owner-local unit");
        };
        let next = workers[pos].gather_down;
        if workers[pos].is_gathering_at(site, false) {
            previous = Some(pos);
        } else {
            set_link(site, workers, previous, next);
            workers[pos].gather_down = NO_OBJECT;
            removed += 1;
        }
        current = next;
    }
    Ok(removed)
}

/// Count the linked workers accepted by the corresponding retail `num_gatherers` mode.
/// `count_inside` is the separately evaluated `ObjectData::count_inside` contribution:
/// property `0x1A6` counts type `0x32`, and property `0x1A4` counts type `0x34` (with
/// inside mode `0x11` or `0x13`). Ordinary sites pass zero.
pub fn num_gatherers(
    site: &GatherSite,
    workers: &[GatherWorker],
    count: GatherCount,
    count_inside: i32,
) -> Result<i32, &'static str> {
    let require_been_there = count == GatherCount::Active;
    let mut total = count_inside;
    let mut current = site.gather_down;
    let mut visited = 0usize;
    while current >= 0 {
        if visited >= workers.len() {
            return Err("gather chain contains a cycle");
        }
        visited += 1;
        let Some(pos) = worker_pos(workers, site.owner, current) else {
            return Err("gather chain references a missing owner-local unit");
        };
        if workers[pos].is_gathering_at(site, require_been_there) {
            total = total.wrapping_add(1);
        }
        current = workers[pos].gather_down;
    }
    Ok(total)
}

/// Attach one worker against the site's already-refreshed authoritative capacity.
///
/// Capacity refresh is deliberately separate: retail `Build::add_gatherer` reads persistent
/// `BuildData::gather_max`; `Build::find_gather_tiles`/`update_max_gatherers` refresh it when
/// terrain state changes. No building-type fallback exists here.
pub fn attach_worker(
    site: &mut GatherSite,
    workers: &mut [GatherWorker],
    unit_o: i16,
) -> AttachResult {
    let Ok(occupied) = num_gatherers(site, workers, GatherCount::Assigned, 0) else {
        return AttachResult::Invalid;
    };
    if occupied >= site.max_gatherers() {
        return AttachResult::Full;
    }
    let Some(pos) = worker_pos(workers, site.owner, unit_o) else {
        return AttachResult::Invalid;
    };
    if !workers[pos].is_gathering_at(site, false)
        || !workers[pos]
            .assignment
            .is_some_and(|assignment| assignment.targets_live_site(site))
    {
        return AttachResult::Invalid;
    }

    let mut current = site.gather_down;
    while current >= 0 {
        if current == unit_o {
            return AttachResult::AlreadyAttached;
        }
        let Some(i) = worker_pos(workers, site.owner, current) else {
            return AttachResult::Invalid;
        };
        current = workers[i].gather_down;
    }
    if check_gatherers(site, workers).is_err() {
        return AttachResult::Invalid;
    }

    workers[pos].gather_down = site.gather_down;
    site.gather_down = unit_o;
    AttachResult::Attached
}

/// `Build::remove_gatherer` (`0x0062F8D0`). Returns whether the worker was linked.
pub fn detach_worker(
    site: &mut GatherSite,
    workers: &mut [GatherWorker],
    unit_o: i16,
) -> Result<bool, &'static str> {
    let mut previous = None;
    let mut current = site.gather_down;
    let mut visited = 0usize;
    while current >= 0 {
        if visited >= workers.len() {
            return Err("gather chain contains a cycle");
        }
        visited += 1;
        let Some(pos) = worker_pos(workers, site.owner, current) else {
            return Err("gather chain references a missing owner-local unit");
        };
        let next = workers[pos].gather_down;
        if current == unit_o {
            set_link(site, workers, previous, next);
            workers[pos].gather_down = NO_OBJECT;
            return Ok(true);
        }
        previous = Some(pos);
        current = next;
    }
    Ok(false)
}

/// Gross-income units contributed by one ordinary building worker before terrain/player
/// modifiers. `BuildTypeData::calc_gather` unscales `PEASANT_RATE`/`OIL_RATE` from 8.8,
/// then shifts the result left four before adding it to the six-slot leader income.
#[inline]
pub fn base_worker_gross(rules: &EconRules, oil: bool) -> i32 {
    economy::worker_rate(rules, oil).wrapping_shl(4)
}

/// Apply a fully evaluated per-worker six-slot terrain/type result to the live active
/// worker count. The caller is responsible for producing `per_worker` through the
/// authoritative `BuildTypeData::calc_gather` path, including bonuses and resource choice.
pub fn site_gross(
    per_worker: [i32; NUM_RESOURCES],
    active_workers: i32,
    gather_max: i8,
) -> [i32; NUM_RESOURCES] {
    let used = active_workers.max(0).min(i32::from(gather_max).max(0));
    per_worker.map(|v| v.wrapping_mul(used))
}

/// The crowding tail of `UnitData::calc_gather` (`0x00609180`). Resource nodes gathered
/// directly by units divide every slot independently by `competitors + 1`.
#[inline]
pub fn direct_unit_gross(
    evaluated: [i32; NUM_RESOURCES],
    competitors: i32,
) -> [i32; NUM_RESOURCES] {
    economy::share_among_gatherers(evaluated, competitors)
}

/// Move this frame's six gross-income slots through retail's `GATHER_RATE << 4`
/// accumulators. Commerce caps/expenses/handicaps are deliberately not repeated here;
/// `Leader::do_gather` applies those through `resource_tick` before this accumulator.
pub fn credit_gather_frame(
    income: [i32; NUM_RESOURCES],
    gather_rate: i32,
    accumulators: &mut [i32; NUM_RESOURCES],
) -> [i32; NUM_RESOURCES] {
    let period = resource_period(gather_rate);
    std::array::from_fn(|i| credit_resource(income[i], period, &mut accumulators[i]))
}

/// GatherOrder's checksum-visible scalar payload.
///
/// `GatherOrder::walk_data` (`0x00486E60`) walks the order base byte, TargetOrder's
/// `[ox, whom, uid]` ten-byte span, then GatherOrder's twenty bytes from `tx` through
/// `been_there`. Container metadata belongs to the surrounding `OrderList` walker.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GatherOrderWalk {
    pub order_base_byte: u8,
    pub ox: i32,
    pub whom: i32,
    pub uid: u16,
    pub tx: i32,
    pub ty: i32,
    pub build_type: i32,
    pub wait: i32,
    pub goto_build: u8,
    pub non_flat_gather: u8,
    pub dist_mod: u8,
    pub been_there: u8,
}

/// The complete network command payload accepted by `Unit::add_gather_order`
/// (`0x0061A5C0`): command `0x13`, target owner-local object index, and `QueuePos`.
/// The command does not transmit a UID; order construction captures the target's current
/// UID so later `TargetOrder::target_exists` can reject slot reuse.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GatherCommandWalk {
    pub opcode: u8,
    pub target_o: i32,
    pub queue_pos: i32,
}

impl GatherCommandWalk {
    pub const WALKED_BYTES: usize = 9;

    pub fn image(self) -> [u8; Self::WALKED_BYTES] {
        let mut out = [0; Self::WALKED_BYTES];
        out[0] = self.opcode;
        out[1..5].copy_from_slice(&self.target_o.to_le_bytes());
        out[5..9].copy_from_slice(&self.queue_pos.to_le_bytes());
        out
    }
}

impl GatherOrderWalk {
    pub const WALKED_BYTES: usize = 31;

    pub fn image(&self) -> [u8; Self::WALKED_BYTES] {
        let mut out = [0u8; Self::WALKED_BYTES];
        out[0] = self.order_base_byte;
        out[1..5].copy_from_slice(&self.ox.to_le_bytes());
        out[5..9].copy_from_slice(&self.whom.to_le_bytes());
        out[9..11].copy_from_slice(&self.uid.to_le_bytes());
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assigned(owner: u8, unit_o: i16, target: i16, been_there: bool) -> GatherWorker {
        let mut w = GatherWorker::new(owner, unit_o, 0x32);
        w.assignment = Some(GatherAssignment {
            target_owner: i32::from(owner),
            target_build: i32::from(target),
            target_uid: 0,
            been_there,
            inside_target: None,
        });
        w
    }

    #[test]
    fn attach_and_detach_use_the_owner_local_short_chain() {
        let mut site = GatherSite::new(2, 41);
        site.set_authoritative_capacity(2);
        let mut workers = [assigned(2, 7, 41, true), assigned(2, 9, 41, true)];
        assert_eq!(
            attach_worker(&mut site, &mut workers, 7),
            AttachResult::Attached
        );
        assert_eq!(
            attach_worker(&mut site, &mut workers, 9),
            AttachResult::Attached
        );
        assert_eq!(site.gather_down, 9);
        assert_eq!(workers[1].gather_down, 7);
        assert_eq!(workers[0].gather_down, NO_OBJECT);
        assert_eq!(
            num_gatherers(&site, &workers, GatherCount::Active, 0),
            Ok(2)
        );

        assert_eq!(detach_worker(&mut site, &mut workers, 9), Ok(true));
        assert_eq!(site.gather_down, 7);
        assert_eq!(workers[1].gather_down, NO_OBJECT);
        assert_eq!(detach_worker(&mut site, &mut workers, 7), Ok(true));
        assert_eq!(site.gather_down, NO_OBJECT);
    }

    #[test]
    fn capacity_and_arrival_are_separate_retail_counts() {
        let mut site = GatherSite::new(0, 4);
        site.set_authoritative_capacity(2);
        let mut workers = [assigned(0, 1, 4, false), assigned(0, 2, 4, true)];
        assert_eq!(
            attach_worker(&mut site, &mut workers, 1),
            AttachResult::Attached
        );
        assert_eq!(
            attach_worker(&mut site, &mut workers, 2),
            AttachResult::Attached
        );
        assert_eq!(
            num_gatherers(&site, &workers, GatherCount::Assigned, 0),
            Ok(2)
        );
        assert_eq!(
            num_gatherers(&site, &workers, GatherCount::Active, 0),
            Ok(1)
        );
    }

    #[test]
    fn stale_orders_are_pruned_and_capacity_is_not_a_type_table() {
        let mut site = GatherSite::new(0, 12);
        let mut workers = [assigned(0, 3, 12, true), assigned(0, 5, 99, true)];
        workers[0].gather_down = 5;
        site.gather_down = 3;
        assert_eq!(check_gatherers(&mut site, &mut workers), Ok(1));
        assert_eq!(site.gather_down, 3);
        assert_eq!(workers[0].gather_down, NO_OBJECT);
        site.set_authoritative_capacity(0);
        assert_eq!(
            attach_worker(&mut site, &mut workers, 3),
            AttachResult::Full
        );
        assert_eq!(site.max_gatherers(), 0);
    }

    #[test]
    fn gather_tiles_are_reservations_not_depletion_counters() {
        let mut world = World::init_default_rules(4, 4);
        let tiles = [GatherTile { tx: 3, ty: 5 }, GatherTile { tx: 4, ty: 5 }];
        assert_eq!(gather_tile_reserved(&world, tiles[0]), Some(false));
        assert_eq!(set_gather_tiles_reserved(&mut world, &tiles, true), Ok(()));
        assert_eq!(gather_tile_reserved(&world, tiles[0]), Some(true));
        assert_eq!(gather_tile_reserved(&world, tiles[1]), Some(true));
        assert_eq!(set_gather_tiles_reserved(&mut world, &tiles, false), Ok(()));
        assert_eq!(gather_tile_reserved(&world, tiles[0]), Some(false));

        let invalid = GatherTile { tx: -1, ty: 0 };
        assert_eq!(
            set_gather_tiles_reserved(&mut world, &[tiles[0], invalid], true),
            Err(invalid)
        );
        assert_eq!(gather_tile_reserved(&world, tiles[0]), Some(false));
        assert_eq!(gather_tile_reserved(&world, invalid), None);
    }

    #[test]
    fn base_rate_and_accumulator_keep_retails_sixteenth_scale() {
        let rules = EconRules::shipped();
        assert_eq!(base_worker_gross(&rules, false), 160);
        assert_eq!(base_worker_gross(&rules, true), 560);
        let mut acc = [0; NUM_RESOURCES];
        let mut credited = [0; NUM_RESOURCES];
        for _ in 0..rules.gather_rate() {
            let frame = credit_gather_frame([160, 0, 0, 0, 0, 0], rules.gather_rate(), &mut acc);
            for i in 0..NUM_RESOURCES {
                credited[i] += frame[i];
            }
        }
        assert_eq!(credited, [10, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn retail_fixed_capacity_helpers_are_one_and_seven() {
        assert_eq!(MAX_FLAT_GATHERERS, 1);
        assert_eq!(MAX_KNOWLEDGE_GATHERERS, 7);
        assert_eq!(
            site_gross([0, 0, 0, 80, 0, 0], 8, MAX_KNOWLEDGE_GATHERERS as i8),
            [0, 0, 0, 560, 0, 0]
        );
    }

    #[test]
    fn generated_rules_bridge_preserves_all_six_scholar_rates_and_cap_seven() {
        let generated = don_rules::Rules::shipped();
        let rules = EconRules::from_block(&generated.raw);
        assert_eq!(
            std::array::from_fn::<_, 6, _>(|i| rules.scholar_rate(i)),
            [1280, 1792, 2560, 3840, 5120, 6400]
        );
        assert_eq!(
            std::array::from_fn::<_, 6, _>(|i| {
                economy::scholar_rate_for_level(&rules, i as i32 + 1)
            }),
            [80, 112, 160, 240, 320, 400]
        );
        assert_eq!(MAX_KNOWLEDGE_GATHERERS, 7);
    }

    #[test]
    fn site_capacity_and_direct_crowding_have_distinct_arithmetic() {
        assert_eq!(site_gross([160, 0, 0, 0, 0, 0], 5, 3), [480, 0, 0, 0, 0, 0]);
        assert_eq!(
            direct_unit_gross([11, -11, 0, 0, 0, 0], 2),
            [3, -3, 0, 0, 0, 0]
        );
    }

    #[test]
    fn gather_order_walk_has_the_three_retail_windows_in_order() {
        let order = GatherOrderWalk {
            order_base_byte: 0xaa,
            ox: 0x0102_0304,
            whom: 0x1112_1314,
            uid: 0x2122,
            tx: 0x3132_3334,
            ty: 0x4142_4344,
            build_type: 0x5152_5354,
            wait: 0x6162_6364,
            goto_build: 0x71,
            non_flat_gather: 0x72,
            dist_mod: 0x73,
            been_there: 0x74,
        };
        assert_eq!(
            order.image(),
            [
                0xaa, 4, 3, 2, 1, 0x14, 0x13, 0x12, 0x11, 0x22, 0x21, 0x34, 0x33, 0x32, 0x31, 0x44,
                0x43, 0x42, 0x41, 0x54, 0x53, 0x52, 0x51, 0x64, 0x63, 0x62, 0x61, 0x71, 0x72, 0x73,
                0x74,
            ]
        );
    }

    #[test]
    fn gather_command_is_nine_bytes_and_order_uid_rejects_slot_reuse() {
        let command = GatherCommandWalk {
            opcode: 0x13,
            target_o: 0x0102_0304,
            queue_pos: 2,
        };
        assert_eq!(command.image(), [0x13, 4, 3, 2, 1, 2, 0, 0, 0]);

        let mut site = GatherSite::new(3, 9);
        site.uid = 44;
        let current = GatherAssignment {
            target_owner: 3,
            target_build: 9,
            target_uid: 44,
            been_there: false,
            inside_target: None,
        };
        assert!(current.targets_live_site(&site));
        assert!(!GatherAssignment {
            target_uid: 45,
            ..current
        }
        .targets_live_site(&site));
    }
}
