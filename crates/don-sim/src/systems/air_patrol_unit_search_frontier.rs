// SPDX-License-Identifier: GPL-3.0-or-later
//! Executable frontier for `Unit::do_air_patrol`'s mod-16 unit search.
//!
//! This registered module owns the instruction-derived call order, finder arguments,
//! scratch-list traversal, ranking, option fallback, and patrol acceptance test. The object
//! grid, `Object::valid_target`, and `Object::compare_target` remain mandatory host reads; an
//! unavailable read fails closed instead of becoming an empty search.

pub const SHIPPED_EXE_SHA256: &str =
    "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079";

pub const UNIT_DO_AIR_PATROL_VA: u32 = 0x005e_a620;
pub const UNIT_DO_AIR_PATROL_BYTES: usize = 1_248;
pub const FIND_NEW_BOMBER_TARGET_VA: u32 = 0x005e_b960;
pub const FIND_NEW_BOMBER_TARGET_BYTES: usize = 781;
pub const FIND_NEW_AIR_TARGET_VA: u32 = 0x005e_bc70;
pub const FIND_NEW_AIR_TARGET_BYTES: usize = 873;
pub const OBJECTS_FIND_BUILDS_VA: u32 = 0x0065_a120;
pub const OBJECTS_FIND_UNITS_VA: u32 = 0x0065_a620;
pub const OBJECT_VALID_TARGET_VA: u32 = 0x0064_8ba0;
pub const OBJECT_COMPARE_TARGET_VA: u32 = 0x0064_e5c0;

pub const WORLD_UNITS_PER_TILE: i32 = 0xc0;
pub const BROAD_RADIUS_SCALE: i32 = 0x3c0;
pub const PATROL_UNIT_SCAN_PERIOD: i32 = 16;
pub const SEARCH_INDEX_BH_3: i32 = 3;
pub const AIR_FILTER_LOCAL: i32 = 13;
pub const AIR_FILTER_GUARDED: i32 = 11;
pub const AIR_FILTER_GENERAL: i32 = 0;
pub const BOMBER_FILTER_BUILD: i32 = 0;
pub const FORCE_ACTOR_ORIGIN_FLAG: u32 = 0x0004_0000;
pub const INVERSE_SEARCH_FALLBACK_OPTION: u32 = 0x2;
pub const DOMAIN_AIR: i32 = 2;

/// The two PDB-named rule fields read by the shipped search functions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RespondRanges {
    /// `Constants+0x2c`, `AIRCRAFT_RESPOND_RANGE`; shipped value 10 tiles.
    pub aircraft: i32,
    /// `Constants+0x30`, `BOMBER_RESPOND_RANGE`; shipped value 12 tiles.
    pub bomber: i32,
}

impl Default for RespondRanges {
    fn default() -> Self {
        Self {
            aircraft: 10,
            bomber: 12,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectRef {
    pub o: i32,
    pub who: i32,
}

impl ObjectRef {
    pub const NONE: Self = Self { o: -1, who: -1 };
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ObjectFacts {
    pub target: ObjectRef,
    pub uid: u16,
    pub x: i32,
    pub y: i32,
    /// `ObjectTypeData+0x218`.
    pub domain: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorFacts {
    pub o: i16,
    pub who: i32,
    pub x: i32,
    pub y: i32,
    /// `ObjectData+0x68`. Bit `0x40000` makes both search functions replace their passed
    /// origin with `(actor.x, actor.y)`.
    pub object_flags: u32,
}

/// A raw call argument whose value is register residue at this optimized callsite.
///
/// The value is not used on the reached filter arm.  Recording that fact is more exact than
/// inventing a deterministic zero and accidentally turning it into adapter state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawArg {
    Value(i32),
    RegisterResidue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchPass {
    /// First AIR pass, centred on the actor, `FilterIndex(13)`.
    AirActorLocal,
    /// Optional AIR pass when `guard_o >= 0`, `FilterIndex(11)`.
    AirGuarded,
    /// Final AIR pass at the requested origin, `FilterIndex(0)`.
    AirGeneral,
    /// Optional BOMBER wide build pass when `guard_o >= 0`.
    BomberGuarded,
    /// Final BOMBER short build pass.
    BomberShort,
}

/// Exact stable arguments to `Objects::find_units` `0x0065A620`.
///
/// PDB parameters are `(x,y,search,whom,max_dist,search_mask,filter,filter_data,
/// filter_data2,<unnamed>,ignore_animals)`. The optimized callee does not read the unnamed
/// tenth stack argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindUnitsQuery {
    pub pass: SearchPass,
    pub x: i32,
    pub y: i32,
    pub search: i32,
    pub whom: i32,
    pub max_dist: i32,
    pub search_mask: i32,
    pub filter: i32,
    pub filter_data: i32,
    pub filter_data2: i32,
    pub unread_arg: RawArg,
    pub ignore_animals: i32,
}

/// Exact stable arguments to `Objects::find_builds` `0x0065A120`.
///
/// With `FilterIndex(0)`, the optimized caller leaves `filter_data2` as register residue.
/// That value does not affect the reached filter arm; `add_to_list` is explicitly zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindBuildsQuery {
    pub pass: SearchPass,
    pub x: i32,
    pub y: i32,
    pub search: i32,
    pub whom: i32,
    pub max_dist: i32,
    pub search_mask: i32,
    pub filter: i32,
    pub filter_data: i32,
    pub filter_data2: RawArg,
    pub add_to_list: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinderQuery {
    Units(FindUnitsQuery),
    Builds(FindBuildsQuery),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingSearchFact {
    Finder(SearchPass),
    Object(ObjectRef),
    ValidTarget(ObjectRef),
    CompareTarget(ObjectRef),
    ArithmeticDomain,
}

/// Mandatory world half of the search.
///
/// `find` must preserve the `Objects` scratch-array order. `valid_target` and
/// `compare_target` stand for the exact retail calls with `extra/check_path=0` and
/// `(check_path,mode)=(1,0)` respectively. No method has a default.
pub trait UnitSearchHost {
    fn find(&mut self, query: FinderQuery) -> Result<Vec<ObjectRef>, MissingSearchFact>;
    fn object(&mut self, target: ObjectRef) -> Result<ObjectFacts, MissingSearchFact>;
    fn valid_target(&mut self, target: ObjectRef) -> Result<bool, MissingSearchFact>;
    fn compare_target(&mut self, target: ObjectRef) -> Result<i32, MissingSearchFact>;
}

/// The low-level return shape of both retail functions.
///
/// On the early "origin too far from actor" return, retail returns `o=-1` without touching
/// `*targ_who`. Every completed traversal writes either the winning owner or `-1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawSearchResult {
    pub target_o: i32,
    pub target_who_write: Option<i32>,
}

impl RawSearchResult {
    const fn hit(target: ObjectRef) -> Self {
        Self {
            target_o: target.o,
            target_who_write: Some(target.who),
        }
    }

    const fn miss_written() -> Self {
        Self {
            target_o: -1,
            target_who_write: Some(-1),
        }
    }

    const fn miss_unwritten() -> Self {
        Self {
            target_o: -1,
            target_who_write: None,
        }
    }

    /// Apply the pointer write to the caller-owned slot and form the pair retail passes to
    /// its following `Object::valid_target` call.
    pub fn apply_target_who(self, target_who_slot: &mut i32) -> ObjectRef {
        if let Some(who) = self.target_who_write {
            *target_who_slot = who;
        }
        ObjectRef {
            o: self.target_o,
            who: *target_who_slot,
        }
    }
}

#[inline]
fn normal_origin(actor: ActorFacts, x: i32, y: i32) -> (i32, i32) {
    if actor.object_flags & FORCE_ACTOR_ORIGIN_FLAG != 0 {
        (actor.x, actor.y)
    } else {
        (x, y)
    }
}

/// `vector_dist` `0x0046CFF0`, including the large-coordinate unsigned arm.
pub fn vector_dist(a: i32, b: i32) -> i32 {
    let ai = a.wrapping_abs();
    let bi = b.wrapping_abs();
    let (big, small) = if ai > bi { (ai, bi) } else { (bi, ai) };
    if big == 0 {
        return 0;
    }
    if small >= 60_000 {
        return (((small as u32).wrapping_add((big as u32) << 1)) >> 1) as i32;
    }
    let square = (small as u32).wrapping_mul(small as u32);
    (square / ((big as u32) << 1)) as i32 + big
}

#[inline]
fn distance(from_x: i32, from_y: i32, to_x: i32, to_y: i32) -> i32 {
    vector_dist(to_x.wrapping_sub(from_x), to_y.wrapping_sub(from_y))
}

fn fold_air<H: UnitSearchHost>(
    host: &mut H,
    candidates: Vec<ObjectRef>,
    distance_gate: Option<(i32, i32, i32)>,
) -> Result<Option<ObjectRef>, MissingSearchFact> {
    let mut best_value = -1;
    let mut best = None;
    for target in candidates {
        if !host.valid_target(target)? {
            continue;
        }
        if let Some((x, y, radius)) = distance_gate {
            let object = host.object(target)?;
            if distance(x, y, object.x, object.y) > radius {
                continue;
            }
        }
        let value = host.compare_target(target)?;
        // Strict-greater preserves the first scratch-list candidate on equal scores.
        if value > best_value {
            best_value = value;
            best = Some(target);
        }
    }
    Ok(best)
}

fn bomber_value(priority: i32, distance: i32) -> Result<i32, MissingSearchFact> {
    // The ordinary world-coordinate domain keeps vector_dist non-negative. Fail closed on
    // overflow-domain values rather than making Rust panic or inventing x86 #DE handling.
    if distance < 0 {
        return Err(MissingSearchFact::ArithmeticDomain);
    }
    Ok(priority / (distance / WORLD_UNITS_PER_TILE + 1))
}

fn fold_bomber<H: UnitSearchHost>(
    host: &mut H,
    candidates: Vec<ObjectRef>,
    origin_x: i32,
    origin_y: i32,
    distance_gate: Option<i32>,
) -> Result<Option<ObjectRef>, MissingSearchFact> {
    let mut best_value = -1;
    let mut best = None;
    for target in candidates {
        if !host.valid_target(target)? {
            continue;
        }
        let object = host.object(target)?;
        let dist = distance(origin_x, origin_y, object.x, object.y);
        if distance_gate.is_some_and(|radius| dist > radius) {
            continue;
        }
        let priority = host.compare_target(target)?;
        let value = bomber_value(priority, dist)?;
        if value > best_value {
            best_value = value;
            best = Some(target);
        }
    }
    Ok(best)
}

/// `Unit::find_new_air_target` `0x005EBC70`.
pub fn find_new_air_target<H: UnitSearchHost>(
    host: &mut H,
    actor: ActorFacts,
    requested_x: i32,
    requested_y: i32,
    guard_o: i32,
    guard_who: i32,
    aircraft_respond_range: i32,
) -> Result<RawSearchResult, MissingSearchFact> {
    let (origin_x, origin_y) = normal_origin(actor, requested_x, requested_y);
    let short_radius = aircraft_respond_range.wrapping_mul(WORLD_UNITS_PER_TILE);
    let broad_radius = aircraft_respond_range.wrapping_mul(BROAD_RADIUS_SCALE);

    // Pass 1 is always actor-centred, even when the caller supplied a distant patrol point.
    let local_query = FindUnitsQuery {
        pass: SearchPass::AirActorLocal,
        x: actor.x,
        y: actor.y,
        search: SEARCH_INDEX_BH_3,
        whom: actor.who,
        max_dist: short_radius,
        search_mask: 1,
        filter: AIR_FILTER_LOCAL,
        filter_data: 2,
        filter_data2: 0,
        unread_arg: RawArg::Value(actor.y),
        ignore_animals: 1,
    };
    let local = host.find(FinderQuery::Units(local_query))?;
    if let Some(target) = fold_air(host, local, Some((actor.x, actor.y, short_radius)))? {
        return Ok(RawSearchResult::hit(target));
    }

    // Retail performs this gate only after the local pass failed.
    if distance(actor.x, actor.y, origin_x, origin_y) > broad_radius {
        return Ok(RawSearchResult::miss_unwritten());
    }

    if guard_o >= 0 {
        let guarded_query = FindUnitsQuery {
            pass: SearchPass::AirGuarded,
            x: origin_x,
            y: origin_y,
            search: SEARCH_INDEX_BH_3,
            whom: actor.who,
            max_dist: broad_radius,
            search_mask: 1,
            filter: AIR_FILTER_GUARDED,
            filter_data: guard_o,
            filter_data2: guard_who,
            unread_arg: RawArg::Value(broad_radius),
            ignore_animals: 1,
        };
        let guarded = host.find(FinderQuery::Units(guarded_query))?;
        // No redundant explicit distance test on this pass; `find_units` owns the radius.
        if let Some(target) = fold_air(host, guarded, None)? {
            return Ok(RawSearchResult::hit(target));
        }
    }

    let general_query = FindUnitsQuery {
        pass: SearchPass::AirGeneral,
        x: origin_x,
        y: origin_y,
        search: SEARCH_INDEX_BH_3,
        whom: actor.who,
        max_dist: short_radius,
        search_mask: 1,
        filter: AIR_FILTER_GENERAL,
        filter_data: 0,
        filter_data2: 0,
        unread_arg: RawArg::RegisterResidue,
        ignore_animals: 1,
    };
    let general = host.find(FinderQuery::Units(general_query))?;
    Ok(
        match fold_air(host, general, Some((origin_x, origin_y, short_radius)))? {
            Some(target) => RawSearchResult::hit(target),
            None => RawSearchResult::miss_written(),
        },
    )
}

/// `Unit::find_new_bomber_target` `0x005EB960`.
pub fn find_new_bomber_target<H: UnitSearchHost>(
    host: &mut H,
    actor: ActorFacts,
    requested_x: i32,
    requested_y: i32,
    guard_o: i32,
    _guard_who: i32,
    bomber_respond_range: i32,
) -> Result<RawSearchResult, MissingSearchFact> {
    let (origin_x, origin_y) = normal_origin(actor, requested_x, requested_y);
    let short_radius = bomber_respond_range.wrapping_mul(WORLD_UNITS_PER_TILE);
    let broad_radius = bomber_respond_range.wrapping_mul(BROAD_RADIUS_SCALE);

    if distance(actor.x, actor.y, origin_x, origin_y) > broad_radius {
        return Ok(RawSearchResult::miss_unwritten());
    }

    if guard_o >= 0 {
        let guarded_query = FindBuildsQuery {
            pass: SearchPass::BomberGuarded,
            x: origin_x,
            y: origin_y,
            search: SEARCH_INDEX_BH_3,
            whom: actor.who,
            max_dist: broad_radius,
            search_mask: 0,
            filter: BOMBER_FILTER_BUILD,
            filter_data: 0,
            filter_data2: RawArg::RegisterResidue,
            add_to_list: 0,
        };
        let guarded = host.find(FinderQuery::Builds(guarded_query))?;
        if let Some(target) = fold_bomber(host, guarded, origin_x, origin_y, None)? {
            return Ok(RawSearchResult::hit(target));
        }
    }

    let short_query = FindBuildsQuery {
        pass: SearchPass::BomberShort,
        x: origin_x,
        y: origin_y,
        search: SEARCH_INDEX_BH_3,
        whom: actor.who,
        max_dist: short_radius,
        search_mask: 0,
        filter: BOMBER_FILTER_BUILD,
        filter_data: 0,
        filter_data2: RawArg::RegisterResidue,
        add_to_list: 0,
    };
    let short = host.find(FinderQuery::Builds(short_query))?;
    Ok(
        match fold_bomber(host, short, origin_x, origin_y, Some(short_radius))? {
            Some(target) => RawSearchResult::hit(target),
            None => RawSearchResult::miss_written(),
        },
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatrolSearchKind {
    Air,
    Bomber,
}

impl PatrolSearchKind {
    const fn inverse(self) -> Self {
        match self {
            Self::Air => Self::Bomber,
            Self::Bomber => Self::Air,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatrolUnitScanInput {
    pub actor: ActorFacts,
    pub frame: i32,
    pub is_animal: bool,
    pub returning: i32,
    /// The last AIR_PATROL waypoint after the caller's fighter-bomber/home projection and
    /// world restriction. Fallback deliberately ignores this and uses the actor position.
    pub primary_x: i32,
    pub primary_y: i32,
    pub actor_is_bomber: bool,
    pub game_option_flags: u32,
    /// The acceptance test observes the cursor after the same-frame waypoint-arrival update.
    pub at_last_waypoint: bool,
    pub ranges: RespondRanges,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatrolUnitScanAction {
    NotDue,
    NoValidTarget,
    /// A non-air target is ignored until the patrol reaches its last waypoint.
    RejectGroundBeforeLast {
        target: ObjectFacts,
    },
    InsertStrafe {
        target: ObjectFacts,
        mandatory: u8,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatrolUnitScanResult {
    pub primary: PatrolSearchKind,
    pub fallback_called: Option<PatrolSearchKind>,
    pub action: PatrolUnitScanAction,
}

#[inline]
pub fn patrol_unit_scan_due(input: &PatrolUnitScanInput) -> bool {
    !input.is_animal
        && input.returning == 0
        && (input.actor.o as i32).wrapping_add(input.frame) % PATROL_UNIT_SCAN_PERIOD == 0
}

fn invoke<H: UnitSearchHost>(
    host: &mut H,
    kind: PatrolSearchKind,
    actor: ActorFacts,
    x: i32,
    y: i32,
    ranges: RespondRanges,
) -> Result<RawSearchResult, MissingSearchFact> {
    match kind {
        PatrolSearchKind::Air => find_new_air_target(host, actor, x, y, -1, -1, ranges.aircraft),
        // `guard_who` is dead in find_new_bomber_target. Some optimized callers pass
        // register residue; zero here cannot affect its behavior.
        PatrolSearchKind::Bomber => find_new_bomber_target(host, actor, x, y, -1, 0, ranges.bomber),
    }
}

/// Exact mod-16 primary/fallback/acceptance owner from `Unit::do_air_patrol`.
pub fn run_patrol_unit_scan<H: UnitSearchHost>(
    host: &mut H,
    input: PatrolUnitScanInput,
) -> Result<PatrolUnitScanResult, MissingSearchFact> {
    let primary = if input.actor_is_bomber {
        PatrolSearchKind::Bomber
    } else {
        PatrolSearchKind::Air
    };
    if !patrol_unit_scan_due(&input) {
        return Ok(PatrolUnitScanResult {
            primary,
            fallback_called: None,
            action: PatrolUnitScanAction::NotDue,
        });
    }

    // In do_air_patrol this stack slot previously held `returning`, proven zero by the due
    // gate. An early search return therefore leaves target_who as zero, not `-1`.
    let mut target_who_slot = 0;
    let mut raw = invoke(
        host,
        primary,
        input.actor,
        input.primary_x,
        input.primary_y,
        input.ranges,
    )?;
    let mut target = raw.apply_target_who(&mut target_who_slot);
    let primary_valid = host.valid_target(target)?;

    let mut fallback_called = None;
    if !primary_valid && input.game_option_flags & INVERSE_SEARCH_FALLBACK_OPTION != 0 {
        let fallback = primary.inverse();
        fallback_called = Some(fallback);
        raw = invoke(
            host,
            fallback,
            input.actor,
            input.actor.x,
            input.actor.y,
            input.ranges,
        )?;
        target = raw.apply_target_who(&mut target_who_slot);
    }

    // Retail performs a second/common validation even when the primary validation succeeded.
    if !host.valid_target(target)? {
        return Ok(PatrolUnitScanResult {
            primary,
            fallback_called,
            action: PatrolUnitScanAction::NoValidTarget,
        });
    }
    let object = host.object(target)?;
    let action = if input.at_last_waypoint || object.domain == DOMAIN_AIR {
        PatrolUnitScanAction::InsertStrafe {
            target: object,
            mandatory: 0,
        }
    } else {
        PatrolUnitScanAction::RejectGroundBeforeLast { target: object }
    };
    Ok(PatrolUnitScanResult {
        primary,
        fallback_called,
        action,
    })
}

/// Remaining integration seams. The first four are adjacent retail owners; the last two are
/// publication work and do not weaken the exactness of this isolated control owner.
pub const AIR_PATROL_UNIT_SEARCH_OPEN_SEAMS: [&str; 6] = [
    "Objects::find_units/find_builds spatial grids and scratch-array order",
    "Object::valid_target diplomacy, visibility, and targetability adapter",
    "Object::compare_target complete live object/type/leader adapter",
    "coherent object identity, UID, coordinates, and ObjectTypeData domain snapshot",
    "registration in systems/mod.rs and convergence with order_dispatch callback",
    "atomic EnvWorld AirPatrolHost publication and save/replay coverage",
];
