//! Fail-closed adapters for Arena's recovered retail systems.
//!
//! This module does not make the arena construction-complete, gather-capable, naval or
//! airborne by declaration. It names the recovered retail kernels that are safe to call,
//! keeps every world-query prerequisite explicit, and inventories the missing host systems.
//! `arena::world` may consume these adapters only when it owns the corresponding state;
//! callers cannot obtain a convenient default for placement, gathering capacity, water
//! reachability, supply coverage, air target search, or diplomacy AI.

use crate::arena::types::TypeRow;
use don_sim::rng::Random;
use don_sim::systems::air::{self, AntiAirGate, AntiAirShot, FuelVerdict};
use don_sim::systems::borders_fog::{
    self, AttritionDamage, AttritionInput, AttritionRules, SupplyInput,
};
use don_sim::systems::construction::{self, ConstructionEffects};
use don_sim::systems::economy::NUM_RESOURCES;
use don_sim::systems::gathering::{self, GatherCount, GatherSite, GatherTile, GatherWorker};
use don_sim::systems::map_terrain::World;
use don_sim::systems::naval::{self, TransportNeed, WaterWorld};
use don_sim::systems::production::{BuildData, ProdRules};
use don_sim::systems::victory_score::{self, Diplo, NUM_LEADERS};

/// Literal Arena host transactions behind the former aggregate MODEL 2/3 declarations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleSubsystem {
    ConstructionScheduleAndIdentity,
    ConstructionPlacement,
    ConstructionLifecycle,
    ConstructionInterruption,
    GatherCapacity,
    GatherOccupancy,
    GatherTerrainReservation,
    GatherPayout,
}

/// Executable construction/gathering integration inventory. An adapter is not a product
/// completion claim: every row remains blocked until the real arena command/tick path owns
/// the listed transaction and its checksum/RNG effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LifecycleIntegrationItem {
    pub subsystem: LifecycleSubsystem,
    pub status: IntegrationStatus,
    pub recovered: &'static [&'static str],
    pub missing: &'static [&'static str],
}

pub const LIFECYCLE_INVENTORY: &[LifecycleIntegrationItem] = &[
    LifecycleIntegrationItem {
        subsystem: LifecycleSubsystem::ConstructionScheduleAndIdentity,
        status: IntegrationStatus::AdapterOnly,
        recovered: &[
            "persistent BuildData construction fields",
            "(who,o,uid) BuildAt target and frame/builder state machine",
        ],
        missing: &[
            "arena BuildData/object-key storage",
            "building-band then retail owner/slot builder traversal",
            "authoritative check_build_order builder gate",
        ],
    },
    LifecycleIntegrationItem {
        subsystem: LifecycleSubsystem::ConstructionPlacement,
        status: IntegrationStatus::Blocked,
        recovered: &["blocked_site result contract and wonder-capacity gate"],
        missing: &[
            "blocked_location and blocked_tcoord",
            "terrain, territory, cliff/water, adjacency, dock and city-limit queries",
        ],
    },
    LifecycleIntegrationItem {
        subsystem: LifecycleSubsystem::ConstructionLifecycle,
        status: IntegrationStatus::Blocked,
        recovered: &["lazy start, reject, progress and completion call ordering"],
        missing: &[
            "Build::start and Build::activate world transactions",
            "Object::disband and Unit::build_done/reassignment transactions",
            "complete transitive RNG and checksum receipts",
        ],
    },
    LifecycleIntegrationItem {
        subsystem: LifecycleSubsystem::ConstructionInterruption,
        status: IntegrationStatus::Blocked,
        recovered: &["builder death/cancel boundary and lazy stale-target behavior"],
        missing: &[
            "arena unit close/order-cancel transaction",
            "target close/disband object-generation transaction",
        ],
    },
    LifecycleIntegrationItem {
        subsystem: LifecycleSubsystem::GatherCapacity,
        status: IntegrationStatus::Blocked,
        recovered: &["signed-byte gather_max storage and fixed 1/7 helper cases"],
        missing: &[
            "complete BuildTypeData::calc_gather terrain/type evaluator",
            "ordered MiningList, access, ownership, diplomacy and player modifiers",
        ],
    },
    LifecycleIntegrationItem {
        subsystem: LifecycleSubsystem::GatherOccupancy,
        status: IntegrationStatus::AdapterOnly,
        recovered: &[
            "owner-local gather_down chain",
            "generational GatherOrder identity and exact attach/prune/detach",
        ],
        missing: &[
            "arena owner-local object table and persistent links",
            "gather command/order execution and close-path detach",
        ],
    },
    LifecycleIntegrationItem {
        subsystem: LifecycleSubsystem::GatherTerrainReservation,
        status: IntegrationStatus::Blocked,
        recovered: &["atomic TData 0x1000 reservation writes for selected tiles"],
        missing: &[
            "find_gather_tiles and verify_gather_tiles ordered MiningList lifecycle",
            "non-flat gather selection/rotation and territory invalidation",
        ],
    },
    LifecycleIntegrationItem {
        subsystem: LifecycleSubsystem::GatherPayout,
        status: IntegrationStatus::Blocked,
        recovered: &["active-worker count, six-slot gross composition and carry accumulators"],
        missing: &[
            "authoritative per-worker terrain/type six-slot evaluation",
            "arena leader income/cap/expense transaction and checksum ownership",
        ],
    },
];

/// Stable construction boundary for an Arena host. The adapter deliberately provides no
/// fallback effects: constructing it requires a concrete [`ConstructionEffects`] owner,
/// and every callback error remains visible to the caller.
pub struct ArenaConstructionHost<'a, E: ConstructionEffects> {
    effects: &'a mut E,
}

impl<'a, E: ConstructionEffects> ArenaConstructionHost<'a, E> {
    pub fn new(effects: &'a mut E) -> Self {
        Self { effects }
    }

    pub fn begin_site_frame(
        &mut self,
        site: &mut BuildData,
        frame: construction::ConstructionFrame,
        rules: &ProdRules,
    ) -> construction::EffectReceipt {
        construction::begin_site_frame(site, frame, rules)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn execute_builder(
        &mut self,
        site: &mut BuildData,
        site_key: construction::ObjectKey,
        builder: construction::ObjectKey,
        target: construction::BuildOrderTarget,
        contribution: construction::BuilderContribution,
        rules: &ProdRules,
    ) -> Result<construction::BuildReceipt, construction::ConstructionError<E::Error>> {
        construction::execute_builder(
            site,
            site_key,
            builder,
            target,
            contribution,
            rules,
            self.effects,
        )
    }

    pub fn interrupt_builder(
        &mut self,
        builder: construction::ObjectKey,
        target: construction::ObjectKey,
        reason: construction::BuilderFinish,
    ) -> Result<construction::BuildReceipt, construction::ConstructionError<E::Error>> {
        construction::interrupt_builder(self.effects, builder, target, reason)
    }
}

/// Explicit Arena-owned gathering state. Capacity and ordered tiles must already come from
/// the authoritative evaluator; this type has no building-type/radius fallback and owns no
/// hidden worker membership.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArenaGatherHost {
    pub site: GatherSite,
    pub workers: Vec<GatherWorker>,
    pub ordered_tiles: Vec<GatherTile>,
}

impl ArenaGatherHost {
    pub fn from_authoritative_state(
        site: GatherSite,
        workers: Vec<GatherWorker>,
        ordered_tiles: Vec<GatherTile>,
    ) -> Self {
        Self {
            site,
            workers,
            ordered_tiles,
        }
    }

    pub fn set_tiles_reserved(&self, world: &mut World, reserved: bool) -> Result<(), GatherTile> {
        gathering::set_gather_tiles_reserved(world, &self.ordered_tiles, reserved)
    }

    pub fn attach_worker(&mut self, unit_o: i16) -> gathering::AttachResult {
        gathering::attach_worker(&mut self.site, &mut self.workers, unit_o)
    }

    pub fn prune_workers(&mut self) -> Result<usize, &'static str> {
        gathering::check_gatherers(&mut self.site, &mut self.workers)
    }

    pub fn detach_worker(&mut self, unit_o: i16) -> Result<bool, &'static str> {
        gathering::detach_worker(&mut self.site, &mut self.workers, unit_o)
    }

    pub fn gross_from_evaluated_worker(
        &self,
        per_worker: [i32; NUM_RESOURCES],
        count_inside: i32,
    ) -> Result<[i32; NUM_RESOURCES], &'static str> {
        let active =
            gathering::num_gatherers(&self.site, &self.workers, GatherCount::Active, count_inside)?;
        Ok(gathering::site_gross(
            per_worker,
            active,
            self.site.gather_max,
        ))
    }
}

/// One literal product blocker formerly hidden inside the single MODEL 6 sentence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model6Subsystem {
    WaterTerrain,
    Naval,
    Air,
    Diplomacy,
    Attrition,
    Supply,
}

/// How far an inventory item can honestly be used today.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegrationStatus {
    /// Exact local kernels have an arena-shaped, fail-closed adapter, but no arena tick caller.
    AdapterOnly,
    /// Recovery exists only as isolated primitives/proxies; product integration is forbidden.
    Blocked,
}

/// Executable integration inventory. Missing prerequisites are literal retail host systems,
/// not broad labels and not tasks that can be cleared by setting a capability flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntegrationItem {
    pub subsystem: Model6Subsystem,
    pub status: IntegrationStatus,
    pub recovered: &'static [&'static str],
    pub missing: &'static [&'static str],
}

pub const MODEL6_INVENTORY: &[IntegrationItem] = &[
    IntegrationItem {
        subsystem: Model6Subsystem::WaterTerrain,
        status: IntegrationStatus::AdapterOnly,
        recovered: &[
            "map_terrain water/WATERHALF storage and predicates",
            "naval WaterWorld coordinate and coast predicates",
            "bounded needs_transport and exact dock-shore adapters",
        ],
        missing: &[
            "one authoritative arena WData/TData water view (WaterWorld is currently separate)",
            "retail map water generation and continent construction",
            "tile/water astar_path domains and calc_cost",
            "water-specific compute_reg_territory radius and falloff",
        ],
    },
    IntegrationItem {
        subsystem: Model6Subsystem::Naval,
        status: IntegrationStatus::Blocked,
        recovered: &[
            "naval shipped roster and static predicates",
            "exact land/water embark-disembark boundary",
            "dock registry bookkeeping and conditional gull RNG fragment",
            "fish candidate scan and fixed retail move tables",
        ],
        missing: &[
            "arena live-type carry/carry_size fields and persistent containment state",
            "retail find_wpath (current route APIs are explicitly proxies)",
            "boarding, go_inside, unloading and rendezvous order mutations",
            "dock construction, production queues, gull/object lifetime and naval supply",
        ],
    },
    IntegrationItem {
        subsystem: Model6Subsystem::Air,
        status: IntegrationStatus::AdapterOnly,
        recovered: &[
            "live AirTypeData fields",
            "anti-air dud gate and exact main-RNG draw count",
            "fuel and capacity adapters with explicit completed host scans/queue inputs",
            "flight band and air-order local transitions",
        ],
        missing: &[
            "Unit::do_air_physics",
            "building-band then unit-band air target/host scans in retail traversal order",
            "live Ammo::init insertion at the measured RNG position",
            "aircraft order-list, hosted-object and checksum state",
        ],
    },
    IntegrationItem {
        subsystem: Model6Subsystem::Diplomacy,
        status: IntegrationStatus::AdapterOnly,
        recovered: &[
            "LeaderData mutual-minimum relation predicates",
            "victory/alliance state transitions",
        ],
        missing: &[
            "Leader::diplomacy strategy body",
            "set_diplo retargeting, shared-vision removal, chat and event side effects",
            "one walked declaration matrix shared by target/path/collision/victory consumers",
            "arena command/UI surface for declarations",
        ],
    },
    IntegrationItem {
        subsystem: Model6Subsystem::Attrition,
        status: IntegrationStatus::AdapterOnly,
        recovered: &[
            "calc_anti_attrition and get_attrition arithmetic",
            "period phase and suffer-attrition damage shape",
        ],
        missing: &[
            "arena per-unit attrition-period state and retail recomputation sites",
            "runtime type virtuals get_bonus(0x42), +0x10c and +0x308",
            "Object::take_damage fractional-damage denominator",
            "removal of arena combat's unconditional in_supply=true input",
        ],
    },
    IntegrationItem {
        subsystem: Model6Subsystem::Supply,
        status: IntegrationStatus::AdapterOnly,
        recovered: &[
            "ordered Unit::process_supply host-query adapter",
            "out-of-supply reload arithmetic and constants",
        ],
        missing: &[
            "Supplies::find_supply and source lifetime/index",
            "three building proximity queries in retail traversal order",
            "per-tick resupplied flag transaction in UnitData walked state",
            "located reload call site and supply healing",
        ],
    },
];

/// Bounded access to the exact water predicates already recovered in `don-sim`.
///
/// This borrows a [`WaterWorld`] because Arena does not yet own an authoritative water
/// view. It deliberately validates coordinates before calling retail-shaped helpers that
/// assume their callers already performed the engine's bounds checks. It never falls back
/// to Arena's land-only map or to the explicitly approximate water-route proxy.
pub struct ArenaWaterHost<'a> {
    water: &'a WaterWorld,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaterAdapterError {
    TileOutOfBounds { tx: i32, ty: i32 },
    WCellOutOfBounds { wx: i32, wy: i32 },
}

impl<'a> ArenaWaterHost<'a> {
    pub fn new(water: &'a WaterWorld) -> Self {
        Self { water }
    }

    /// Exact `UnitData::needs_transport` medium transition. Both endpoints are mandatory
    /// authoritative TCoords; an absent or off-map endpoint is an error, not dry land.
    pub fn transport_need(
        &self,
        from: (i32, i32),
        to: (i32, i32),
    ) -> Result<TransportNeed, WaterAdapterError> {
        for (tx, ty) in [from, to] {
            if !self.water.valid_t(tx, ty) {
                return Err(WaterAdapterError::TileOutOfBounds { tx, ty });
            }
        }
        Ok(naval::needs_transport(self.water, from, to))
    }

    /// Exact `BuildTypeData::is_dock_tile` shore/apron predicate. This is placement
    /// evidence only; it does not stand in for dock construction or object allocation.
    pub fn dock_site_allowed(&self, wx: i32, wy: i32) -> Result<bool, WaterAdapterError> {
        if !self.water.valid_w(wx, wy) {
            return Err(WaterAdapterError::WCellOutOfBounds { wx, wy });
        }
        Ok(naval::is_dock_tile(self.water, wx, wy))
    }
}

/// Explicit `LeaderData::diplos` state for an arena host. This is simulation state—not bot
/// memory. Callers can walk it through [`DiplomacyState::declarations`], but mutations stay
/// behind the explicitly state-only transaction below.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiplomacyState {
    declared: [[i32; NUM_LEADERS]; NUM_LEADERS],
}

impl DiplomacyState {
    /// Retail's leader reset initializes declarations to war. Self is still reported as an
    /// ally by `get_diplo`; the diagonal storage cells are not special-cased here.
    pub const fn at_war() -> Self {
        Self {
            declared: [[Diplo::War as i32; NUM_LEADERS]; NUM_LEADERS],
        }
    }

    pub const fn declarations(&self) -> &[[i32; NUM_LEADERS]; NUM_LEADERS] {
        &self.declared
    }

    /// Write only the directional `LeaderData::diplos[other]` cell and report its effective
    /// relation change. The name is intentionally explicit: this is not the complete
    /// `Leader::set_diplo` transaction and performs none of its retargeting, vision, event,
    /// or chat side effects.
    pub fn write_declaration_state_only(
        &mut self,
        who: usize,
        other: usize,
        state: Diplo,
    ) -> Result<DiplomacyStateReceipt, PlayerSlot> {
        self.check_pair(who, other)?;
        let declared_before = self.declared[who][other];
        let effective_before = self.relation(who, other)?;
        self.declared[who][other] = state as i32;
        Ok(DiplomacyStateReceipt {
            declared_before,
            declared_after: state as i32,
            effective_before,
            effective_after: self.relation(who, other)?,
        })
    }

    pub fn relation(&self, who: usize, other: usize) -> Result<Diplo, PlayerSlot> {
        self.check_pair(who, other)?;
        Ok(victory_score::effective_diplo(
            who == other,
            self.declared[who][other],
            self.declared[other][who],
        ))
    }

    pub fn is_enemy(&self, who: usize, other: usize) -> Result<bool, PlayerSlot> {
        self.relation(who, other).map(|d| d == Diplo::War)
    }

    pub fn is_ally(&self, who: usize, other: usize) -> Result<bool, PlayerSlot> {
        self.relation(who, other).map(|d| d == Diplo::Ally)
    }

    pub fn is_peace(&self, who: usize, other: usize) -> Result<bool, PlayerSlot> {
        self.relation(who, other).map(|d| d == Diplo::Peace)
    }

    fn check_pair(&self, who: usize, other: usize) -> Result<(), PlayerSlot> {
        if who >= NUM_LEADERS {
            return Err(PlayerSlot(who));
        }
        if other >= NUM_LEADERS {
            return Err(PlayerSlot(other));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerSlot(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiplomacyStateReceipt {
    pub declared_before: i32,
    pub declared_after: i32,
    pub effective_before: Diplo,
    pub effective_after: Diplo,
}

/// Arena-shaped anti-air call. The caller must pass the same main [`Random`] used by the
/// tick; the adapter deliberately owns no RNG and therefore cannot hide stream state.
#[derive(Clone, Copy, Debug)]
pub struct ArenaAntiAirShot<'a> {
    pub whom: i32,
    pub ox: i32,
    pub target_active: bool,
    pub target_is_unit: bool,
    pub target: &'a TypeRow,
    pub target_flying_low: bool,
    pub shooter_is_unit: bool,
    pub shooter_order: i32,
    pub shooter: &'a TypeRow,
}

pub fn anti_air_gate(shot: &ArenaAntiAirShot<'_>, rng: &mut Random) -> AntiAirGate {
    let target = shot.target.air_type_data();
    let shooter = shot.shooter.air_type_data();
    air::antiair_dud_gate(
        &AntiAirShot {
            whom: shot.whom,
            ox: shot.ox,
            target_active: shot.target_active,
            target_is_unit: shot.target_is_unit,
            target_type: &target,
            target_flying_low: shot.target_flying_low,
            shooter_is_unit: shot.shooter_is_unit,
            shooter_order: shot.shooter_order,
            shooter_type: &shooter,
        },
        rng,
    )
}

/// Completed queue facts for `ObjectData::num_aircraft_here`.
///
/// Carrier queues are stored on the carrier and are always counted. A non-carrier build
/// host consumes the two exact build-record call results recovered by `don-sim`; the
/// adapter will not accept an already-combined or zero-defaulted adjustment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirQueueFacts {
    None,
    Carrier {
        num_queued: i32,
    },
    Build {
        also_count_queue: bool,
        count_kind2_type0: i32,
        count_kind1_helicopter: i32,
    },
}

impl AirQueueFacts {
    fn retail(self) -> air::AircraftQueueAccounting {
        match self {
            Self::None => air::AircraftQueueAccounting::None,
            Self::Carrier { num_queued } => air::AircraftQueueAccounting::Carrier { num_queued },
            Self::Build {
                also_count_queue,
                count_kind2_type0,
                count_kind1_helicopter,
            } => air::AircraftQueueAccounting::Build {
                also_count_queue,
                count_kind2_type0,
                count_kind1_helicopter,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AirCapacityReceipt {
    pub host: air::AirHost,
    pub aircraft_here: i32,
    pub aircraft_limit: i32,
    pub may_launch: bool,
}

/// Capacity query over a completed owner-local hosted-object traversal. The adapter uses
/// the live Arena type row for strict host identity and the `'J'` Holds-Air bit. It owns no
/// object scan and invents no queue counters.
pub fn air_capacity(
    host_type: &TypeRow,
    host_o: i32,
    host_who: i32,
    hosted: &[air::HostedAircraft],
    queue: AirQueueFacts,
) -> Result<AirCapacityReceipt, AirAdapterError> {
    check_air_object(host_o, host_who)?;
    let host = air::air_host_of(host_type.id);
    let queue_matches_host = match (host, host_type.kind_building, queue) {
        (air::AirHost::Carrier, _, AirQueueFacts::Carrier { .. }) => true,
        (air::AirHost::Carrier, _, _) => false,
        (_, true, AirQueueFacts::Build { .. }) => true,
        (_, false, AirQueueFacts::None) => true,
        _ => false,
    };
    if !queue_matches_host {
        return Err(AirAdapterError::QueueHostMismatch {
            type_id: host_type.id,
        });
    }
    let aircraft_here = air::num_aircraft_here(host_o, host_who, hosted, queue.retail());
    let aircraft_limit = air::num_aircraft_limit(host);
    Ok(AirCapacityReceipt {
        host,
        aircraft_here,
        aircraft_limit,
        may_launch: air::airbase_may_launch(
            &host_type.air_type_data(),
            aircraft_here,
            aircraft_limit,
        ),
    })
}

/// Fuel transition over exact live type fields. Host discovery remains a mandatory caller
/// result; [`AirHostSearch::Exhausted`] means the retail scans completed and found nothing,
/// not “unmodeled”.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirHostSearch {
    Found { o: i32, who: i32 },
    Exhausted,
}

/// Outcome of validating the aircraft's currently assigned host before the nearest-host
/// scan. `Rejected` is distinct from no assignment so a caller cannot silently omit the
/// retail capacity test and pass `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurrentAirHost {
    Unassigned,
    Accepted { o: i32, who: i32 },
    Rejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirAdapterError {
    NotAirDomain { type_id: i32 },
    InvalidObject { o: i32, who: i32 },
    QueueHostMismatch { type_id: i32 },
}

fn check_air_object(o: i32, who: i32) -> Result<(), AirAdapterError> {
    if o < 0 || !(0..NUM_LEADERS as i32).contains(&who) {
        Err(AirAdapterError::InvalidObject { o, who })
    } else {
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
pub fn air_fuel_transition(
    ty: &TypeRow,
    mana_burn: i16,
    on_map: bool,
    has_space_program: bool,
    space_air_range_pct: i32,
    returning: i32,
    current_host: CurrentAirHost,
    nearest_host: AirHostSearch,
) -> Result<(i16, i32, FuelVerdict), AirAdapterError> {
    let type_id = ty.id;
    let ty = ty.air_type_data();
    if !ty.is_air_domain() {
        return Err(AirAdapterError::NotAirDomain { type_id });
    }
    let cap = air::mana_cap(&ty, has_space_program, space_air_range_pct);
    let burn = air::air_fuel_step(mana_burn, cap, on_map);
    let left = (cap - burn as i32).max(0);
    let (current_host_ok, current_host) = match current_host {
        CurrentAirHost::Unassigned | CurrentAirHost::Rejected => (false, None),
        CurrentAirHost::Accepted { o, who } => {
            check_air_object(o, who)?;
            (true, Some((o, who)))
        }
    };
    let nearest_host = match nearest_host {
        AirHostSearch::Found { o, who } => {
            check_air_object(o, who)?;
            Some((o, who))
        }
        AirHostSearch::Exhausted => None,
    };
    let (returning, verdict) = air::check_fuel(
        &ty,
        returning,
        left,
        current_host_ok,
        current_host,
        nearest_host,
    );
    Ok((burn, returning, verdict))
}

/// Which already-derived `Unit::process_attrition` period source the host selected. The
/// selection itself depends on object graph predicates not present in the arena, so it is
/// an enum—not a guessed priority order.
#[derive(Clone, Copy, Debug)]
pub enum AttritionPeriodSource {
    Disabled,
    Normal(AttritionInput),
    Peace { type_class: i32 },
    Assassin { type_class: i32 },
}

pub fn attrition_period(source: AttritionPeriodSource, rules: &AttritionRules) -> Option<i16> {
    match source {
        AttritionPeriodSource::Disabled => None,
        AttritionPeriodSource::Normal(input) => {
            borders_fog::attrition_period(borders_fog::get_attrition(&input, rules), rules)
        }
        AttritionPeriodSource::Peace { type_class } => Some(borders_fog::special_attrition_period(
            rules.peace_attrition,
            type_class,
        )),
        AttritionPeriodSource::Assassin { type_class } => Some(
            borders_fog::special_attrition_period(rules.assassin_attrition, type_class),
        ),
    }
}

/// Guards checked before `Unit::process_supply` performs any world query. They remain
/// separate from query results because all three make the retail predicate return false;
/// calling that “out of supply” would erase why the search was suppressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SupplyGuards {
    pub already_flagged: bool,
    pub always_supplied: bool,
    pub militia: bool,
}

/// Stable identity and de-obfuscated position of the unit whose owner-local supply queries
/// are being performed. `Supplies::find_supply` consumes `(x, y, who)`; the nearby-building
/// calls are instance methods, so `(who, o)` must stay attached to the same query receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SupplyUnitKey {
    pub who: i32,
    pub o: i32,
    pub x: i32,
    pub y: i32,
}

/// The first retail short-circuit that prevented a supply search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupplyGuard {
    AlreadyFlagged,
    AlwaysSupplied,
    Militia,
}

/// Identity returned by the first successful supply query. These object/source indices
/// are mandatory host evidence: the adapter does not reduce an unavailable world lookup
/// to `false`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupplySource {
    SuppliesIndex(i32),
    OwnedBuilding { type_id: i32, object_index: i32 },
}

/// Completed `Unit::process_supply` outcome, including why no later query ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupplyOutcome {
    Guarded(SupplyGuard),
    Supplied(SupplySource),
    Exhausted,
}

/// Unforgeable receipt from [`resolve_supply`]. Its private outcome forces an Arena caller
/// to run the ordered, fallible host queries before feeding the result to attrition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SupplyResolution {
    outcome: SupplyOutcome,
}

impl SupplyResolution {
    pub fn outcome(self) -> SupplyOutcome {
        self.outcome
    }

    fn process_supply_return(self) -> bool {
        matches!(self.outcome, SupplyOutcome::Supplied(_))
    }

    fn retail(self, guards: SupplyGuards) -> SupplyInput {
        let (near_supply_source, near_building_16b, near_building_176, near_building_16e) =
            match self.outcome {
                SupplyOutcome::Supplied(SupplySource::SuppliesIndex(_)) => {
                    (true, false, false, false)
                }
                SupplyOutcome::Supplied(SupplySource::OwnedBuilding { type_id: 0x16B, .. }) => {
                    (false, true, false, false)
                }
                SupplyOutcome::Supplied(SupplySource::OwnedBuilding { type_id: 0x176, .. }) => {
                    (false, false, true, false)
                }
                SupplyOutcome::Supplied(SupplySource::OwnedBuilding { type_id: 0x16E, .. }) => {
                    (false, false, false, true)
                }
                SupplyOutcome::Supplied(SupplySource::OwnedBuilding { .. })
                | SupplyOutcome::Guarded(_)
                | SupplyOutcome::Exhausted => (false, false, false, false),
            };
        SupplyInput {
            already_flagged: guards.already_flagged,
            always_supplied: guards.always_supplied,
            militia: guards.militia,
            near_supply_source,
            near_building_16b,
            near_building_176,
            near_building_16e,
        }
    }
}

/// Arena-owned world queries consumed by `Unit::process_supply`. Implementations must
/// perform the real owner-local searches and return stable object/source identities.
/// Methods are called in retail order and stop after the first hit.
pub trait ArenaSupplyQueries {
    type Error;

    fn find_supply(&mut self, unit: SupplyUnitKey) -> Result<Option<i32>, Self::Error>;
    fn find_owned_building_in_range(
        &mut self,
        unit: SupplyUnitKey,
        type_id: i32,
    ) -> Result<Option<i32>, Self::Error>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum SupplyAdapterError<E> {
    Host(E),
    InvalidUnit {
        who: i32,
        o: i32,
    },
    InvalidObjectIndex {
        query: SupplyQuery,
        object_index: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupplyQuery {
    Supplies,
    OwnedBuilding(i32),
}

/// Resolve `Unit::process_supply` without flattening ordered, fallible world queries into
/// convenient booleans. The synthetic `SupplyInput` cross-check at the end ensures this
/// adapter stays aligned with the recovered local predicate.
pub fn resolve_supply<Q: ArenaSupplyQueries>(
    unit: SupplyUnitKey,
    guards: SupplyGuards,
    queries: &mut Q,
) -> Result<SupplyResolution, SupplyAdapterError<Q::Error>> {
    if unit.o < 0 || !(0..NUM_LEADERS as i32).contains(&unit.who) {
        return Err(SupplyAdapterError::InvalidUnit {
            who: unit.who,
            o: unit.o,
        });
    }
    let guarded = if guards.already_flagged {
        Some(SupplyGuard::AlreadyFlagged)
    } else if guards.always_supplied {
        Some(SupplyGuard::AlwaysSupplied)
    } else if guards.militia {
        Some(SupplyGuard::Militia)
    } else {
        None
    };
    if let Some(guard) = guarded {
        return Ok(SupplyResolution {
            outcome: SupplyOutcome::Guarded(guard),
        });
    }

    if let Some(source) = queries
        .find_supply(unit)
        .map_err(SupplyAdapterError::Host)?
    {
        if source < 0 {
            return Err(SupplyAdapterError::InvalidObjectIndex {
                query: SupplyQuery::Supplies,
                object_index: source,
            });
        }
        let result = SupplyResolution {
            outcome: SupplyOutcome::Supplied(SupplySource::SuppliesIndex(source)),
        };
        debug_assert!(borders_fog::supply_state(&result.retail(guards)));
        return Ok(result);
    }

    for type_id in [0x16B, 0x176, 0x16E] {
        if let Some(object_index) = queries
            .find_owned_building_in_range(unit, type_id)
            .map_err(SupplyAdapterError::Host)?
        {
            if object_index < 0 {
                return Err(SupplyAdapterError::InvalidObjectIndex {
                    query: SupplyQuery::OwnedBuilding(type_id),
                    object_index,
                });
            }
            let result = SupplyResolution {
                outcome: SupplyOutcome::Supplied(SupplySource::OwnedBuilding {
                    type_id,
                    object_index,
                }),
            };
            debug_assert!(borders_fog::supply_state(&result.retail(guards)));
            return Ok(result);
        }
    }
    let result = SupplyResolution {
        outcome: SupplyOutcome::Exhausted,
    };
    debug_assert!(!borders_fog::supply_state(&result.retail(guards)));
    Ok(result)
}

#[derive(Clone, Copy, Debug)]
pub struct AttritionTickInput {
    pub frame: i32,
    pub unit_id: i16,
    pub period: i16,
    /// Exact completed result of the ordered supply adapter.
    pub supply: SupplyResolution,
    pub type_308: i32,
    pub curr_uber_size: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttritionTick {
    NotDue,
    SupplyFound,
    Damage(AttritionDamage),
}

pub fn attrition_tick(input: &AttritionTickInput) -> AttritionTick {
    if !borders_fog::attrition_due(input.frame, input.unit_id, input.period) {
        return AttritionTick::NotDue;
    }
    if input.supply.process_supply_return() {
        return AttritionTick::SupplyFound;
    }
    AttritionTick::Damage(borders_fog::attrition_damage(
        input.type_308,
        input.curr_uber_size,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> TypeRow {
        TypeRow {
            kind_unit: true,
            domain: air::DOMAIN_AIR,
            obj_masks: 0,
            mana: 3,
            ..TypeRow::default()
        }
    }

    #[test]
    fn inventory_has_one_literal_entry_per_model6_subsystem() {
        assert_eq!(MODEL6_INVENTORY.len(), 6);
        assert_eq!(MODEL6_INVENTORY[0].status, IntegrationStatus::AdapterOnly);
        assert_eq!(MODEL6_INVENTORY[1].status, IntegrationStatus::Blocked);
        for item in MODEL6_INVENTORY {
            assert!(!item.recovered.is_empty());
            assert!(
                !item.missing.is_empty(),
                "{:?} was falsely cleared",
                item.subsystem
            );
        }
    }

    #[test]
    fn lifecycle_inventory_stays_split_and_fail_closed() {
        assert_eq!(LIFECYCLE_INVENTORY.len(), 8);
        assert!(!construction::RUNTIME_FIDELITY_READY);
        for item in LIFECYCLE_INVENTORY {
            assert!(!item.recovered.is_empty());
            assert!(
                !item.missing.is_empty(),
                "{:?} was falsely cleared",
                item.subsystem
            );
        }
    }

    struct MissingConstructionHost;

    impl ConstructionEffects for MissingConstructionHost {
        type Error = &'static str;

        fn builder_gate(
            &mut self,
            _builder: construction::ObjectKey,
            _target: construction::ObjectKey,
        ) -> Result<construction::BuilderGateReceipt, Self::Error> {
            Err("authoritative builder gate unavailable")
        }

        fn blocked_site(
            &mut self,
            _site_key: construction::ObjectKey,
            _site: &BuildData,
        ) -> Result<construction::SiteCheckReceipt, Self::Error> {
            Err("authoritative placement unavailable")
        }

        fn start_site(
            &mut self,
            _site_key: construction::ObjectKey,
            _site: &mut BuildData,
            _notify: i32,
        ) -> Result<construction::SiteLifecycleReceipt, Self::Error> {
            Err("start transaction unavailable")
        }

        fn disband_site(
            &mut self,
            _site_key: construction::ObjectKey,
            _site: &mut BuildData,
            _mode: i32,
        ) -> Result<construction::SiteLifecycleReceipt, Self::Error> {
            Err("disband transaction unavailable")
        }

        fn activate_site(
            &mut self,
            _site_key: construction::ObjectKey,
            _site: &mut BuildData,
            _arg0: i32,
            _arg1: i32,
            _arg2: i32,
        ) -> Result<construction::SiteLifecycleReceipt, Self::Error> {
            Err("activate transaction unavailable")
        }

        fn finish_builder(
            &mut self,
            _builder: construction::ObjectKey,
            _target: construction::ObjectKey,
            _reason: construction::BuilderFinish,
        ) -> Result<construction::EffectReceipt, Self::Error> {
            Err("build_done transaction unavailable")
        }

        fn interrupt_builder(
            &mut self,
            _builder: construction::ObjectKey,
            _target: construction::ObjectKey,
            _reason: construction::BuilderFinish,
        ) -> Result<construction::EffectReceipt, Self::Error> {
            Err("cancel transaction unavailable")
        }
    }

    #[test]
    fn construction_adapter_propagates_a_missing_host_transaction() {
        let site_key = construction::ObjectKey {
            who: 1,
            o: 4,
            uid: 9,
        };
        let builder = construction::ObjectKey {
            who: 1,
            o: 7,
            uid: 2,
        };
        let mut site = BuildData {
            who: 1,
            uid: 9,
            ..BuildData::default()
        };
        let mut effects = MissingConstructionHost;
        let mut host = ArenaConstructionHost::new(&mut effects);
        assert_eq!(
            host.execute_builder(
                &mut site,
                site_key,
                builder,
                construction::BuildOrderTarget::new(site_key, false),
                construction::BuilderContribution::default(),
                &ProdRules::shipped(),
            ),
            Err(construction::ConstructionError::Effect(
                "authoritative builder gate unavailable"
            ))
        );
    }

    #[test]
    fn gather_adapter_preserves_uid_capacity_chain_and_atomic_reservations() {
        let mut site = GatherSite::new(2, 41);
        site.uid = 91;
        site.set_authoritative_capacity(1);
        let mut worker = GatherWorker::new(2, 7, 0x32);
        worker.assignment = Some(gathering::GatherAssignment {
            target_owner: 2,
            target_build: 41,
            target_uid: 91,
            been_there: true,
            inside_target: None,
        });
        let valid = GatherTile { tx: 3, ty: 5 };
        let invalid = GatherTile { tx: -1, ty: 0 };
        let mut host =
            ArenaGatherHost::from_authoritative_state(site, vec![worker], vec![valid, invalid]);
        let mut world = World::init_default_rules(4, 4);

        assert_eq!(host.set_tiles_reserved(&mut world, true), Err(invalid));
        assert_eq!(gathering::gather_tile_reserved(&world, valid), Some(false));
        host.ordered_tiles.pop();
        assert_eq!(host.set_tiles_reserved(&mut world, true), Ok(()));
        assert_eq!(host.attach_worker(7), gathering::AttachResult::Attached);
        assert_eq!(
            host.gross_from_evaluated_worker([160, 0, 0, 0, 0, 0], 0),
            Ok([160, 0, 0, 0, 0, 0])
        );
    }

    #[test]
    fn diplomacy_uses_the_retail_mutual_minimum_and_checks_slots() {
        let mut d = DiplomacyState::at_war();
        assert_eq!(d.relation(0, 0), Ok(Diplo::Ally));
        assert_eq!(d.relation(0, 1), Ok(Diplo::War));
        let first = d.write_declaration_state_only(0, 1, Diplo::Ally).unwrap();
        assert_eq!(
            first,
            DiplomacyStateReceipt {
                declared_before: Diplo::War as i32,
                declared_after: Diplo::Ally as i32,
                effective_before: Diplo::War,
                effective_after: Diplo::War,
            }
        );
        d.write_declaration_state_only(1, 0, Diplo::Peace).unwrap();
        assert_eq!(d.relation(0, 1), Ok(Diplo::Peace));
        assert_eq!(d.is_peace(1, 0), Ok(true));
        d.write_declaration_state_only(1, 0, Diplo::Ally).unwrap();
        assert_eq!(d.relation(0, 1), Ok(Diplo::Ally));
        assert_eq!(d.relation(NUM_LEADERS, 0), Err(PlayerSlot(NUM_LEADERS)));
    }

    #[test]
    fn water_adapter_requires_in_bounds_authoritative_tiles_and_reads_mutations() {
        let mut water = WaterWorld::new(2, 1);
        assert_eq!(
            ArenaWaterHost::new(&water).transport_need((0, 0), (4, 0)),
            Ok(TransportNeed::None)
        );
        water.set_tocean(4, 0);
        assert_eq!(
            ArenaWaterHost::new(&water).transport_need((0, 0), (4, 0)),
            Ok(TransportNeed::Embark)
        );
        water.set_tocean(0, 0);
        assert_eq!(
            ArenaWaterHost::new(&water).transport_need((0, 0), (4, 0)),
            Ok(TransportNeed::None)
        );
        *water.tmask_mut(4, 0) &= !naval::tmask::COVER;
        assert_eq!(
            ArenaWaterHost::new(&water).transport_need((0, 0), (4, 0)),
            Ok(TransportNeed::Disembark)
        );
        assert_eq!(
            ArenaWaterHost::new(&water).transport_need((8, 0), (4, 0)),
            Err(WaterAdapterError::TileOutOfBounds { tx: 8, ty: 0 })
        );
    }

    #[test]
    fn dock_adapter_reads_the_exact_shore_apron_instead_of_a_water_flag_only() {
        let mut water = WaterWorld::new(1, 2);
        water.wcell_mut(0, 0).land = naval::LAND_WATER_CODES[0];
        let host = ArenaWaterHost::new(&water);
        assert_eq!(host.dock_site_allowed(0, 0), Ok(true));

        water.set_building_at(0, 4, true);
        assert_eq!(
            ArenaWaterHost::new(&water).dock_site_allowed(0, 0),
            Ok(false)
        );
        assert_eq!(
            ArenaWaterHost::new(&water).dock_site_allowed(0, 2),
            Err(WaterAdapterError::WCellOutOfBounds { wx: 0, wy: 2 })
        );
    }

    #[test]
    fn fuel_adapter_owns_no_host_search_or_rng_state() {
        let ty = row();
        let (burn, returning, verdict) = air_fuel_transition(
            &ty,
            2,
            true,
            false,
            0,
            0,
            CurrentAirHost::Unassigned,
            AirHostSearch::Exhausted,
        )
        .unwrap();
        assert_eq!(burn, 3);
        assert_eq!(returning, 1);
        assert_eq!(verdict, FuelVerdict::Crash);

        let (_, _, verdict) = air_fuel_transition(
            &ty,
            2,
            true,
            false,
            0,
            0,
            CurrentAirHost::Rejected,
            AirHostSearch::Found { o: 14, who: 2 },
        )
        .unwrap();
        assert_eq!(verdict, FuelVerdict::ReturnTo { o: 14, who: 2 });
        assert_eq!(
            air_fuel_transition(
                &ty,
                2,
                true,
                false,
                0,
                0,
                CurrentAirHost::Unassigned,
                AirHostSearch::Found { o: -1, who: 2 },
            ),
            Err(AirAdapterError::InvalidObject { o: -1, who: 2 })
        );

        let mut ground = ty;
        ground.id = 50;
        ground.domain = air::DOMAIN_LAND;
        assert_eq!(
            air_fuel_transition(
                &ground,
                0,
                true,
                false,
                0,
                0,
                CurrentAirHost::Unassigned,
                AirHostSearch::Exhausted,
            ),
            Err(AirAdapterError::NotAirDomain { type_id: 50 })
        );
    }

    #[test]
    fn anti_air_adapter_advances_the_callers_main_stream() {
        let mut shooter = TypeRow {
            kind_unit: true,
            domain: air::DOMAIN_LAND,
            obj_masks: air::OBJ_ANTI_AIR,
            fly_low: 100,
            ..TypeRow::default()
        };
        shooter.id = 1;
        let target = row();
        let mut rng = Random::new(9);
        let before = rng.state();
        let gate = anti_air_gate(
            &ArenaAntiAirShot {
                whom: 1,
                ox: 1,
                target_active: true,
                target_is_unit: true,
                target: &target,
                target_flying_low: true,
                shooter_is_unit: true,
                shooter_order: 0,
                shooter: &shooter,
            },
            &mut rng,
        );
        assert_eq!(gate.draws, 1);
        assert_ne!(rng.state(), before);
    }

    #[test]
    fn anti_air_adapter_preserves_target_first_short_circuit_and_second_draw() {
        let shooter = TypeRow {
            kind_unit: true,
            domain: air::DOMAIN_LAND,
            fly_low: 100,
            ..TypeRow::default()
        };
        let mut target = row();
        target.fly_low = 0;
        let mut rng = Random::new(17);
        let dud = anti_air_gate(
            &ArenaAntiAirShot {
                whom: 1,
                ox: 2,
                target_active: true,
                target_is_unit: true,
                target: &target,
                target_flying_low: true,
                shooter_is_unit: true,
                shooter_order: 0,
                shooter: &shooter,
            },
            &mut rng,
        );
        assert_eq!((dud.draws, dud.dud), (1, true));

        target.fly_low = 100;
        let hit = anti_air_gate(
            &ArenaAntiAirShot {
                whom: 1,
                ox: 2,
                target_active: true,
                target_is_unit: true,
                target: &target,
                target_flying_low: true,
                shooter_is_unit: true,
                shooter_order: 0,
                shooter: &shooter,
            },
            &mut rng,
        );
        assert_eq!((hit.draws, hit.dud), (2, false));
    }

    #[test]
    fn aircraft_capacity_depends_on_exact_home_identity_and_queue_fact() {
        let host_type = TypeRow {
            kind_building: true,
            id: air::type_ids::AIRBASE,
            obj_masks: air::OBJ_HOLDS_AIR,
            ..TypeRow::default()
        };
        let hosted = (0..10)
            .map(|_| air::HostedAircraft {
                active: true,
                domain: air::DOMAIN_AIR,
                home_base_o: 20,
                home_base_who: 1,
            })
            .collect::<Vec<_>>();
        let base = air_capacity(
            &host_type,
            20,
            1,
            &hosted,
            AirQueueFacts::Build {
                also_count_queue: false,
                count_kind2_type0: 0,
                count_kind1_helicopter: 0,
            },
        )
        .unwrap();
        assert_eq!((base.aircraft_here, base.aircraft_limit), (10, 10));
        assert!(base.may_launch);
        assert_eq!(
            air_capacity(&host_type, 20, 1, &hosted, AirQueueFacts::None),
            Err(AirAdapterError::QueueHostMismatch {
                type_id: air::type_ids::AIRBASE,
            })
        );

        let queued = air_capacity(
            &host_type,
            20,
            1,
            &hosted,
            AirQueueFacts::Build {
                also_count_queue: true,
                count_kind2_type0: 4,
                count_kind1_helicopter: 3,
            },
        )
        .unwrap();
        assert_eq!(queued.aircraft_here, 11);
        assert!(!queued.may_launch);

        let other_owner = air_capacity(
            &host_type,
            20,
            2,
            &hosted,
            AirQueueFacts::Build {
                also_count_queue: false,
                count_kind2_type0: 0,
                count_kind1_helicopter: 0,
            },
        )
        .unwrap();
        assert_eq!(other_owner.aircraft_here, 0);
    }

    #[derive(Default)]
    struct ScriptedSupplyQueries {
        calls: Vec<i32>,
        units: Vec<SupplyUnitKey>,
        supply: Option<i32>,
        building_16b: Option<i32>,
        building_176: Option<i32>,
        building_16e: Option<i32>,
        fail_on: Option<i32>,
    }

    impl ArenaSupplyQueries for ScriptedSupplyQueries {
        type Error = &'static str;

        fn find_supply(&mut self, unit: SupplyUnitKey) -> Result<Option<i32>, Self::Error> {
            self.calls.push(-1);
            self.units.push(unit);
            if self.fail_on == Some(-1) {
                Err("supply index unavailable")
            } else {
                Ok(self.supply)
            }
        }

        fn find_owned_building_in_range(
            &mut self,
            unit: SupplyUnitKey,
            type_id: i32,
        ) -> Result<Option<i32>, Self::Error> {
            self.calls.push(type_id);
            self.units.push(unit);
            if self.fail_on == Some(type_id) {
                return Err("building traversal unavailable");
            }
            Ok(match type_id {
                0x16B => self.building_16b,
                0x176 => self.building_176,
                0x16E => self.building_16e,
                _ => unreachable!("adapter requested an underived supply building"),
            })
        }
    }

    #[test]
    fn supply_adapter_short_circuits_in_retail_order_and_keeps_identities() {
        let unit = SupplyUnitKey {
            who: 2,
            o: 18,
            x: 9_216,
            y: 7_680,
        };
        let open = SupplyGuards {
            already_flagged: false,
            always_supplied: false,
            militia: false,
        };
        let mut queries = ScriptedSupplyQueries {
            building_176: Some(73),
            ..ScriptedSupplyQueries::default()
        };
        assert_eq!(
            resolve_supply(unit, open, &mut queries).unwrap().outcome(),
            SupplyOutcome::Supplied(SupplySource::OwnedBuilding {
                type_id: 0x176,
                object_index: 73,
            })
        );
        assert_eq!(queries.calls, [-1, 0x16B, 0x176]);
        assert_eq!(queries.units, [unit, unit, unit]);

        queries.calls.clear();
        queries.units.clear();
        queries.supply = Some(11);
        assert_eq!(
            resolve_supply(unit, open, &mut queries).unwrap().outcome(),
            SupplyOutcome::Supplied(SupplySource::SuppliesIndex(11))
        );
        assert_eq!(queries.calls, [-1]);

        queries.calls.clear();
        queries.units.clear();
        queries.supply = None;
        queries.building_176 = None;
        assert_eq!(
            resolve_supply(unit, open, &mut queries).unwrap().outcome(),
            SupplyOutcome::Exhausted
        );
        assert_eq!(queries.calls, [-1, 0x16B, 0x176, 0x16E]);

        queries.calls.clear();
        queries.units.clear();
        let guarded = SupplyGuards {
            already_flagged: true,
            ..open
        };
        assert_eq!(
            resolve_supply(unit, guarded, &mut queries)
                .unwrap()
                .outcome(),
            SupplyOutcome::Guarded(SupplyGuard::AlreadyFlagged)
        );
        assert!(queries.calls.is_empty());
    }

    #[test]
    fn supply_adapter_propagates_missing_or_malformed_host_evidence() {
        let unit = SupplyUnitKey {
            who: 2,
            o: 18,
            x: 9_216,
            y: 7_680,
        };
        let open = SupplyGuards {
            already_flagged: false,
            always_supplied: false,
            militia: false,
        };
        let mut missing = ScriptedSupplyQueries {
            fail_on: Some(0x16B),
            ..ScriptedSupplyQueries::default()
        };
        assert_eq!(
            resolve_supply(unit, open, &mut missing),
            Err(SupplyAdapterError::Host("building traversal unavailable"))
        );
        assert_eq!(missing.calls, [-1, 0x16B]);

        let mut malformed = ScriptedSupplyQueries {
            supply: Some(-4),
            ..ScriptedSupplyQueries::default()
        };
        assert_eq!(
            resolve_supply(unit, open, &mut malformed),
            Err(SupplyAdapterError::InvalidObjectIndex {
                query: SupplyQuery::Supplies,
                object_index: -4,
            })
        );
        assert_eq!(
            resolve_supply(
                SupplyUnitKey { o: -1, ..unit },
                open,
                &mut ScriptedSupplyQueries::default(),
            ),
            Err(SupplyAdapterError::InvalidUnit { who: 2, o: -1 })
        );
    }

    #[test]
    fn attrition_period_adapter_preserves_rate_and_special_source_mutations() {
        let rules = AttritionRules::default();
        let mut normal = AttritionInput {
            attacker_attrition: 1,
            victim_anti_att: borders_fog::ANTI_ATT_BASE,
            siege_class: false,
            militia: false,
            type_id: 50,
            type_class: 0,
            age_diff: 0,
        };
        assert_eq!(
            attrition_period(AttritionPeriodSource::Normal(normal), &rules),
            Some(48)
        );
        normal.attacker_attrition = 2;
        assert_eq!(
            attrition_period(AttritionPeriodSource::Normal(normal), &rules),
            Some(24)
        );
        assert_eq!(
            attrition_period(AttritionPeriodSource::Peace { type_class: 2 }, &rules),
            Some(4)
        );
        assert_eq!(
            attrition_period(AttritionPeriodSource::Disabled, &rules),
            None
        );
    }

    #[test]
    fn attrition_adapter_distinguishes_timing_supply_and_damage() {
        let mut input = AttritionTickInput {
            frame: 47,
            unit_id: 0,
            period: 48,
            supply: SupplyResolution {
                outcome: SupplyOutcome::Supplied(SupplySource::SuppliesIndex(3)),
            },
            type_308: 1,
            curr_uber_size: 4,
        };
        assert_eq!(attrition_tick(&input), AttritionTick::NotDue);
        input.frame = 48;
        assert_eq!(attrition_tick(&input), AttritionTick::SupplyFound);
        input.supply = SupplyResolution {
            outcome: SupplyOutcome::Exhausted,
        };
        assert_eq!(
            attrition_tick(&input),
            AttritionTick::Damage(AttritionDamage {
                flat: 1,
                fractional: 0,
            })
        );
        input.unit_id = 1;
        assert_eq!(attrition_tick(&input), AttritionTick::NotDue);
        input.frame = 47;
        input.type_308 = 0;
        input.curr_uber_size = 4;
        assert_eq!(
            attrition_tick(&input),
            AttritionTick::Damage(AttritionDamage {
                flat: 0,
                fractional: 4,
            })
        );
    }
}
