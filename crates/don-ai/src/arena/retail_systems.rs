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
use don_sim::systems::combat::DamageOutcome;
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
    /// The recovered transaction is called by the live Arena world path. The inventory's
    /// explicit residuals still prevent treating the whole subsystem as complete.
    WorldIntegrated,
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
        status: IntegrationStatus::WorldIntegrated,
        recovered: &[
            "calc_anti_attrition and get_attrition arithmetic",
            "period phase and suffer-attrition damage shape",
            "32-frame live recomputation call site and universal reset prefix",
            "friendly-territory recomputation return",
            "object-backed due-tick supply/attrition mutation transaction",
            "live Arena unit-band host, singleton damage mutation and death close path",
        ],
        missing: &[
            "non-friendly attrition-period selection requiring diplomacy, leader and object graphs",
            "multi-slot captain damage cascade for ObjectType uber_size greater than one",
        ],
    },
    IntegrationItem {
        subsystem: Model6Subsystem::Supply,
        status: IntegrationStatus::WorldIntegrated,
        recovered: &[
            "ordered Unit::process_supply host-query adapter",
            "walked SupplyData and HeroData registry traversal with exact range metric",
            "due-tick UnitData unit_masks2 resupplied write",
            "live Arena support registries, object-table host and unit-band call site",
            "live UnitData::in_supply query at the post-volley siege recharge call site",
            "French and completed-Versailles supply-healing arm with full repair postlude",
            "same-owner land-worker healing, marker clock and singleton repair mutation",
            "same-owner Iroquois ordinary-unit healing with live age and composition preflight",
            "Antipater/Wellington singleton healing through the live hero registry and radius",
            "Senator/President/CEO singleton healing with live relations, masks and ordered composition",
        ],
        missing: &[
            "foreign/allied worker and Iroquois healing do not yet consume the live diplomacy matrix",
            "caravan, merchant and captain healing-family composition",
            "multi-slot captain repair for ObjectType uber_size greater than one",
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
    pub supply_type: bool,
    pub militia: bool,
}

/// Stable identity and de-obfuscated position of the unit whose owner-local supply queries
/// are being performed. `Supplies::find_supply` consumes `(x, y, who)`; the subsequent
/// `ObjectData::has_general` calls are instance methods, so `(who, o)` must stay attached
/// to the same query receipt.
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
    SupplyType,
    Militia,
}

/// Identity returned by the first successful supply query. These object/source indices
/// are mandatory host evidence: the adapter does not reduce an unavailable world lookup
/// to `false`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupplySource {
    SuppliesIndex(i32),
    HeroObject { type_id: i32, object_index: i32 },
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
                SupplyOutcome::Supplied(SupplySource::HeroObject { type_id: 0x16B, .. }) => {
                    (false, true, false, false)
                }
                SupplyOutcome::Supplied(SupplySource::HeroObject { type_id: 0x176, .. }) => {
                    (false, false, true, false)
                }
                SupplyOutcome::Supplied(SupplySource::HeroObject { type_id: 0x16E, .. }) => {
                    (false, false, false, true)
                }
                SupplyOutcome::Supplied(SupplySource::HeroObject { .. })
                | SupplyOutcome::Guarded(_)
                | SupplyOutcome::Exhausted => (false, false, false, false),
            };
        SupplyInput {
            already_flagged: guards.already_flagged,
            always_supplied: guards.supply_type,
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
    fn find_owned_hero_in_range(
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
    HeroType(i32),
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
    } else if guards.supply_type {
        Some(SupplyGuard::SupplyType)
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
            .find_owned_hero_in_range(unit, type_id)
            .map_err(SupplyAdapterError::Host)?
        {
            if object_index < 0 {
                return Err(SupplyAdapterError::InvalidObjectIndex {
                    query: SupplyQuery::HeroType(type_id),
                    object_index,
                });
            }
            let result = SupplyResolution {
                outcome: SupplyOutcome::Supplied(SupplySource::HeroObject {
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

/// `SupplyData::supply_flags` / `HeroData::hero_flags` bit checked by the two retail
/// registry traversals.
pub const SUPPORT_REGISTRY_ACTIVE: u8 = 1;
/// `UnitData::unit_masks2` bit written by `Unit::process` after a due supply hit.
pub const RESUPPLIED_THIS_TICK: u32 = 0x4_0000;
/// Both `SupplyData::get_radius` and `HeroData::get_radius` return tiles; their callers
/// multiply by this many fine world units before comparing `vector_dist`.
pub const SUPPORT_RANGE_UNITS_PER_TILE: i32 = 0xC0;

/// The six bytes walked by `Supply::walk_data` (`0x0073B540`) and consumed in owner-local
/// list order by `Supplies::find_supply` (`0x0073ABA0`). The list slot, not `supply`, is
/// the identity returned by the latter function, so both remain visible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SupplyRegistryRecord {
    pub supply: i16,
    pub o: i16,
    pub supply_flags: u8,
    pub who: i8,
}

/// The identity fields at `HeroData +0x24..+0x29` consumed by
/// `HeroesData::find_hero` (`0x0073A1B0`). The rest of the 48-byte walked record owns the
/// aura-radius calculation and is deliberately accessed through the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeroRegistryRecord {
    pub hero: i16,
    pub o: i16,
    pub hero_flags: u8,
    pub who: i8,
}

/// Object fields and virtual results read by both registry traversals. This is an object
/// snapshot, not a pre-combined "in supply" answer: the adapter still resolves every
/// record, checks liveness and `is_unit`, computes distance, and preserves the first
/// source identity itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SupplySearchObject {
    pub active: bool,
    pub is_unit: bool,
    pub x: i32,
    pub y: i32,
}

/// Exact inputs to `SupplyData::get_radius` (`0x0073B560`). The Terra Cotta Army is
/// TypeIndex `0x211`; `terra_cotta_range` is `Constants +0x45C`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SupplyRadiusFacts {
    pub supply_radius: i32,
    pub supply_radius_upgrade: i32,
    pub supply_upgrade: i32,
    pub has_terra_cotta: bool,
    pub terra_cotta_range: i32,
}

impl SupplyRadiusFacts {
    pub fn radius_tiles(self) -> i32 {
        let radius = self
            .supply_radius
            .wrapping_add(self.supply_radius_upgrade.wrapping_mul(self.supply_upgrade));
        if self.has_terra_cotta {
            radius.wrapping_add(self.terra_cotta_range)
        } else {
            radius
        }
    }
}

/// Scalar leader/rules inputs to `HeroData::get_radius` (`0x00739E50`). Object type
/// predicates are deliberately not flattened into this value; the adapter asks the host's
/// `ObjectTypeData::is` implementation in the retail order and with the retail relation
/// selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeroRadiusFacts {
    pub general_upgrade: i32,
    pub general_radius: i32,
    pub parmenio_radius_adjust_256: i32,
    pub wellington_radius_percent: i32,
    pub kutosov_radius_percent: i32,
    pub has_terra_cotta: bool,
    pub terra_cotta_range: i32,
    pub military_patriot_radius_bonus: i32,
    pub economic_patriot_radius_bonus: i32,
}

/// UnitData fields and virtual results needed for one `Unit::process` supply/attrition
/// branch. A host loads this from one live object immediately before the transaction, so
/// the position, phase, walked mask and damage shape cannot be assembled from unrelated
/// objects by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SupplyAttritionUnitState {
    pub unit: SupplyUnitKey,
    pub unit_id: i16,
    pub type_id: i32,
    pub type_category: i32,
    pub attrition_period: i16,
    pub damage: i32,
    pub healing: i16,
    pub unit_masks: u32,
    pub unit_masks2: u32,
    pub is_supply: bool,
    pub is_hero: bool,
    pub militia: bool,
    pub domain: i32,
    pub type_308: i32,
    pub curr_uber_size: i32,
}

/// Real host boundary for the complete sim-state portion of the due attrition branch.
///
/// The two returned slices are walked registry state, not arbitrary candidates. Object
/// lookup is owner-local exactly as in retail: even though each registry record also
/// stores `who`, its `o` is resolved through the unit being processed's owner table.
/// `object_is` must implement `ObjectTypeData::is(type_id, relation_set)`, including its
/// equivalence lists, rather than compare display names. `hero_radius_facts` exposes the
/// leader/rules scalars used by `HeroData::get_radius`; the adapter owns the ordered type
/// predicates and arithmetic.
///
/// The two mutation methods are intentionally scalar/full-transaction writes. In
/// particular, `take_attrition_damage` must execute the ordinary `Object::take_damage`
/// path (including death/captain cascades) with the recovered flat/fractional shape; it
/// may not update a detached hit-point copy.
pub trait ArenaSupplyAttritionHost {
    type Error;

    fn unit_state(&self, who: i32, o: i32)
        -> Result<Option<SupplyAttritionUnitState>, Self::Error>;
    fn supply_records(&self, who: i32) -> Result<&[SupplyRegistryRecord], Self::Error>;
    fn hero_records(&self, who: i32) -> Result<&[HeroRegistryRecord], Self::Error>;
    fn support_object(&self, who: i32, o: i32) -> Result<Option<SupplySearchObject>, Self::Error>;
    fn supply_radius_facts(&self, who: i32) -> Result<SupplyRadiusFacts, Self::Error>;
    fn owned_type_count(&self, who: i32, type_id: i32) -> Result<i32, Self::Error>;
    fn object_is(
        &self,
        who: i32,
        o: i32,
        type_id: i32,
        relation_set: i32,
    ) -> Result<bool, Self::Error>;
    fn hero_radius_facts(&self, who: i32) -> Result<HeroRadiusFacts, Self::Error>;

    fn write_unit_masks2(
        &mut self,
        who: i32,
        o: i32,
        before: u32,
        after: u32,
    ) -> Result<(), Self::Error>;
    fn take_attrition_damage(
        &mut self,
        who: i32,
        o: i32,
        damage: AttritionDamage,
    ) -> Result<DamageOutcome, Self::Error>;
    fn suffer_graphic_attrition(&mut self, who: i32, o: i32) -> Result<(), Self::Error>;
}

/// Additional live world reads used by `UnitData::in_supply` (`0x00609EF0`), the query
/// performed from `UnitData::recharge` (`0x0060FDF0`). This predicate is deliberately
/// separate from `Unit::process_supply`: non-land and friendly-territory units return in
/// supply immediately, and only the walked Supplies registry is consulted afterward.
pub trait ArenaReloadSupplyHost: ArenaSupplyAttritionHost {
    fn territory_owner_at(&self, x: i32, y: i32) -> Result<i32, Self::Error>;
}

/// Supply-healing facts that belong to the leader/world rather than the unit. The active
/// healing arm at `0x005E0F1A..0x005E0FFC` uses only these two gates after the base rate.
pub trait ArenaSupplyHealingHost: ArenaSupplyAttritionHost {
    fn french_supply_bonus(&self, who: i32) -> Result<bool, Self::Error>;
    fn completed_versailles(&self, who: i32) -> Result<bool, Self::Error>;
    fn unsupported_prior_healing_source(&self, who: i32, o: i32) -> Result<bool, Self::Error>;
    fn repair_supply_damage(
        &mut self,
        who: i32,
        o: i32,
        damage_before: i32,
        healing_before: i16,
        unit_masks_before: u32,
        amount: i32,
        healing_rate: i32,
    ) -> Result<HealingRepairMutation, Self::Error>;
}

/// Live facts and the atomic repair write for the Antipater/Wellington hero-aura arm at
/// `0x005E09ED..0x005E0AE4`. Hero identity and radius are deliberately resolved through
/// the walked `HeroesData` registry inherited from [`ArenaSupplyAttritionHost`].
pub trait ArenaHeroAuraHealingHost: ArenaSupplyHealingHost + ArenaReloadSupplyHost {
    fn repair_hero_aura_damage(
        &mut self,
        who: i32,
        o: i32,
        damage_before: i32,
        healing_before: i16,
        unit_masks_before: u32,
        amount: i32,
        healing_rate: i32,
    ) -> Result<HealingRepairMutation, Self::Error>;
}

/// Live facts and the atomic repair write for the Iroquois healing arm at
/// `0x005E0B49..0x005E0C90`. The optional scenario TypeIndex is a mandatory game-mode
/// fact: `None` means the retail scenario flag is disabled, not that its type lookup was
/// unavailable.
pub trait ArenaIroquoisHealingHost: ArenaHeroAuraHealingHost {
    fn iroquois_healing_bonus(&self, who: i32) -> Result<bool, Self::Error>;
    fn healing_age(&self, who: i32) -> Result<usize, Self::Error>;
    fn scenario_healing_type(&self) -> Result<Option<i32>, Self::Error>;
    fn repair_iroquois_damage(
        &mut self,
        who: i32,
        o: i32,
        damage_before: i32,
        healing_before: i16,
        unit_masks_before: u32,
        amount: i32,
        healing_rate: i32,
    ) -> Result<HealingRepairMutation, Self::Error>;
}

/// Live relations and the atomic repair write for the consecutive Senator, President and
/// CEO aura arms at `0x005E0C90..0x005E0F1A`. `is_allied` must expose the mutual
/// `LeaderData::is_ally` result; owner inequality is not sufficient.
pub trait ArenaPatriotHealingHost: ArenaIroquoisHealingHost {
    fn is_allied(&self, who: i32, other: i32) -> Result<bool, Self::Error>;
    fn repair_patriot_damage(
        &mut self,
        who: i32,
        o: i32,
        damage_before: i32,
        healing_before: i16,
        unit_masks_before: u32,
        amount: i32,
        healing_rate: i32,
    ) -> Result<HealingRepairMutation, Self::Error>;
}

/// Live facts and the atomic repair write for the final worker arm of
/// `Unit::process_healing` (`0x005E1000..0x005E110D`). The worker predicate is recovered
/// as the four literal TypeIndexes `0x32..=0x35`; caravan and merchant virtual predicates
/// deliberately remain outside this boundary.
pub trait ArenaWorkerHealingHost: ArenaPatriotHealingHost {
    fn repair_worker_damage(
        &mut self,
        who: i32,
        o: i32,
        damage_before: i32,
        healing_before: i16,
        unit_masks_before: u32,
        amount: i32,
        healing_rate: i32,
    ) -> Result<HealingRepairMutation, Self::Error>;
}

/// The universal live-world prefix of `Unit::process_attrition` plus the territory read
/// needed by its first exactly-closable branch. The host write is compare-and-swap shaped:
/// callers cannot clear a period or mask value that changed after [`unit_state`].
pub trait ArenaAttritionRecomputeHost: ArenaReloadSupplyHost {
    #[allow(clippy::too_many_arguments)]
    fn write_attrition_recompute_prefix(
        &mut self,
        who: i32,
        o: i32,
        unit_masks_before: u32,
        unit_masks_after: u32,
        unit_masks2_before: u32,
        unit_masks2_after: u32,
        period_before: i16,
        period_after: i16,
    ) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupportRegistry {
    Supplies,
    Heroes,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SupplyAttritionTransactionError<E> {
    Host(E),
    InvalidUnit {
        who: i32,
        o: i32,
    },
    MissingUnit {
        who: i32,
        o: i32,
    },
    UnitIdentityChanged {
        requested_who: i32,
        requested_o: i32,
        found_who: i32,
        found_o: i32,
    },
    UnsupportedPriorHealingSource {
        who: i32,
        o: i32,
    },
    MissingRegistryObject {
        registry: SupportRegistry,
        slot: usize,
        who: i32,
        o: i16,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReloadSupplyState {
    NonLand,
    FriendlyTerritory,
    SuppliesIndex(i32),
    OutOfSupply,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupplyHealingTransaction {
    NoDamage,
    NotLand,
    SupplyUnit,
    Disabled,
    NotDue {
        rate: i32,
    },
    OutOfSupply {
        rate: i32,
    },
    Healed {
        rate: i32,
        source_index: i32,
        repair: HealingRepairMutation,
    },
}

/// Compare-and-swap-shaped receipt for `Unit::repair_damage` plus the caller's exact
/// `ObjectData::healing = max(healing, rate)` postlude. The supply, hero/patriot-aura and
/// supported Iroquois arms clear `unit_masks & 0x4000` after a root unit reaches zero
/// damage; the worker arm retains it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HealingRepairMutation {
    pub damage_before: i32,
    pub damage_after: i32,
    pub healing_before: i16,
    pub healing_after: i16,
    pub unit_masks_before: u32,
    pub unit_masks_after: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerHealingBlocker {
    IroquoisHealing,
    SupplyHealing,
    MultiSlot { type_id: i32, uber_size: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerHealingTransaction {
    NoDamage,
    NotDue {
        rate: i32,
    },
    UnderAttack {
        rate: i32,
    },
    NotWorker {
        rate: i32,
    },
    NotLand {
        rate: i32,
    },
    UnownedTerritory {
        rate: i32,
    },
    BlockedForeignTerritory {
        rate: i32,
        territory_owner: i32,
    },
    BlockedPriorFamily {
        rate: i32,
        blocker: WorkerHealingBlocker,
    },
    Healed {
        rate: i32,
        repair: HealingRepairMutation,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeroAuraHealingBlocker {
    MultiSlot { type_id: i32, uber_size: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeroAuraHealingTransaction {
    NoDamage,
    SeaUnit,
    SupplyUnit,
    Disabled,
    NotDue {
        rate: i32,
    },
    OutsideAura {
        rate: i32,
    },
    BlockedComposition {
        rate: i32,
        blocker: HeroAuraHealingBlocker,
    },
    Healed {
        rate: i32,
        hero_type_id: i32,
        hero_object_index: i32,
        repair: HealingRepairMutation,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatriotHealingBlocker {
    MultiSlot { type_id: i32, uber_size: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatriotHealingArmTransaction {
    NoSource,
    UnownedTerritory,
    NonAlliedTerritory {
        territory_owner: i32,
    },
    GuardedCeoTarget,
    OutsideAura,
    Healed {
        hero_object_index: i32,
        repair: HealingRepairMutation,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatriotHealingTransaction {
    NoDamage,
    SeaUnit,
    SupplyUnit,
    Disabled,
    NotDue {
        rate: i32,
    },
    BlockedComposition {
        rate: i32,
        blocker: PatriotHealingBlocker,
    },
    Processed {
        rate: i32,
        senator: PatriotHealingArmTransaction,
        president: PatriotHealingArmTransaction,
        ceo: PatriotHealingArmTransaction,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IroquoisHealingBlocker {
    CivilianOrMerchant { type_id: i32, type_category: i32 },
    MultiSlot { type_id: i32, uber_size: i32 },
    ScenarioHealing { scenario_type_id: i32 },
    SupplyHealing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IroquoisHealingTransaction {
    NoDamage,
    OtherNation,
    Suppressed,
    HeroUnit,
    NotLand,
    InvalidAge {
        age: usize,
    },
    NotDue {
        rate: i32,
    },
    UnownedTerritory {
        rate: i32,
    },
    BlockedForeignTerritory {
        rate: i32,
        territory_owner: i32,
    },
    BlockedComposition {
        rate: i32,
        blocker: IroquoisHealingBlocker,
    },
    Healed {
        rate: i32,
        repair: HealingRepairMutation,
    },
}

/// Receipt for the 32-frame `Unit::process_attrition` call site in `Unit::process`.
/// `BlockedNonFriendlyTerritory` still proves the universal retail prefix was applied;
/// selecting a replacement period needs the unrecovered diplomacy/leader/object graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttritionRecomputeTransaction {
    NotDue,
    FriendlyTerritoryReset {
        unit_masks_before: u32,
        unit_masks_after: u32,
        unit_masks2_before: u32,
        unit_masks2_after: u32,
        period_before: i16,
    },
    BlockedNonFriendlyTerritory {
        territory_owner: i32,
        unit_masks_before: u32,
        unit_masks_after: u32,
        unit_masks2_before: u32,
        unit_masks2_after: u32,
        period_before: i16,
    },
}

/// Mutation receipt for one `Unit::process` due-check. `Resupplied` proves the walked
/// `unit_masks2` write happened; `Damaged` proves the full host damage call happened and
/// records whether retail's surviving-unit graphic callback ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupplyAttritionTransaction {
    NotDue,
    Resupplied {
        source: SupplySource,
        unit_masks2_before: u32,
        unit_masks2_after: u32,
    },
    Damaged {
        supply: SupplyOutcome,
        damage: AttritionDamage,
        outcome: DamageOutcome,
        graphic_attrition: bool,
    },
}

fn support_distance(unit: SupplyUnitKey, object: SupplySearchObject) -> i32 {
    don_sim::systems::movement::vector_dist(
        unit.x.wrapping_sub(object.x),
        unit.y.wrapping_sub(object.y),
    )
}

fn signed_div_256(value: i32) -> i32 {
    value.wrapping_add((value >> 31) & 0xFF) >> 8
}

fn hero_radius_tiles<H: ArenaSupplyAttritionHost>(
    host: &H,
    object_who: i32,
    object_o: i32,
    radius_who: i32,
) -> Result<i32, SupplyAttritionTransactionError<H::Error>> {
    let facts = host
        .hero_radius_facts(radius_who)
        .map_err(SupplyAttritionTransactionError::Host)?;
    let mut radius = facts
        .general_radius
        .wrapping_mul(facts.general_upgrade.wrapping_add(3))
        / 2;
    if host
        .object_is(object_who, object_o, 0x168, 0)
        .map_err(SupplyAttritionTransactionError::Host)?
    {
        radius = signed_div_256(facts.parmenio_radius_adjust_256.wrapping_mul(radius));
    }
    if host
        .object_is(object_who, object_o, 0x170, 0)
        .map_err(SupplyAttritionTransactionError::Host)?
    {
        radius = facts.wellington_radius_percent.wrapping_mul(radius) / 100;
    }
    if host
        .object_is(object_who, object_o, 0x176, 0)
        .map_err(SupplyAttritionTransactionError::Host)?
    {
        radius = facts.kutosov_radius_percent.wrapping_mul(radius) / 100;
    }
    if facts.has_terra_cotta {
        radius = radius.wrapping_add(facts.terra_cotta_range);
    }
    if host
        .object_is(object_who, object_o, 0x160, 1)
        .map_err(SupplyAttritionTransactionError::Host)?
        || host
            .object_is(object_who, object_o, 0x162, 1)
            .map_err(SupplyAttritionTransactionError::Host)?
        || host
            .object_is(object_who, object_o, 0x164, 1)
            .map_err(SupplyAttritionTransactionError::Host)?
    {
        radius = radius.wrapping_add(facts.military_patriot_radius_bonus);
    }
    if host
        .object_is(object_who, object_o, 0x161, 1)
        .map_err(SupplyAttritionTransactionError::Host)?
        || host
            .object_is(object_who, object_o, 0x163, 1)
            .map_err(SupplyAttritionTransactionError::Host)?
        || host
            .object_is(object_who, object_o, 0x165, 1)
            .map_err(SupplyAttritionTransactionError::Host)?
    {
        radius = radius.wrapping_add(facts.economic_patriot_radius_bonus);
    }
    Ok(radius)
}

fn find_registered_supply<H: ArenaSupplyAttritionHost>(
    state: SupplyAttritionUnitState,
    host: &H,
) -> Result<Option<i32>, SupplyAttritionTransactionError<H::Error>> {
    let supplies = host
        .supply_records(state.unit.who)
        .map_err(SupplyAttritionTransactionError::Host)?;
    for (slot, record) in supplies.iter().copied().enumerate() {
        if record.supply_flags & SUPPORT_REGISTRY_ACTIVE == 0 {
            continue;
        }
        let object = host
            .support_object(state.unit.who, record.o as i32)
            .map_err(SupplyAttritionTransactionError::Host)?
            .ok_or(SupplyAttritionTransactionError::MissingRegistryObject {
                registry: SupportRegistry::Supplies,
                slot,
                who: state.unit.who,
                o: record.o,
            })?;
        if !object.active || !object.is_unit {
            continue;
        }
        let radius = host
            .supply_radius_facts(record.who as i32)
            .map_err(SupplyAttritionTransactionError::Host)?
            .radius_tiles()
            .wrapping_mul(SUPPORT_RANGE_UNITS_PER_TILE);
        if support_distance(state.unit, object) <= radius {
            return Ok(Some(slot as i32));
        }
    }
    Ok(None)
}

/// `ObjectData::has_general(0, type_id)` for Arena's live unit object shape. Retail tests
/// the object itself before walking its owner's `HeroesData` records, then uses each
/// record's owner for `HeroData::get_radius`.
fn find_registered_hero_aura<H: ArenaSupplyAttritionHost>(
    state: SupplyAttritionUnitState,
    type_id: i32,
    host: &H,
) -> Result<Option<i32>, SupplyAttritionTransactionError<H::Error>> {
    if host
        .object_is(state.unit.who, state.unit.o, type_id, 0)
        .map_err(SupplyAttritionTransactionError::Host)?
    {
        return Ok(Some(state.unit.o));
    }

    let heroes = host
        .hero_records(state.unit.who)
        .map_err(SupplyAttritionTransactionError::Host)?;
    for (slot, record) in heroes.iter().copied().enumerate() {
        if record.hero_flags & SUPPORT_REGISTRY_ACTIVE == 0 {
            continue;
        }
        let object = host
            .support_object(state.unit.who, record.o as i32)
            .map_err(SupplyAttritionTransactionError::Host)?
            .ok_or(SupplyAttritionTransactionError::MissingRegistryObject {
                registry: SupportRegistry::Heroes,
                slot,
                who: state.unit.who,
                o: record.o,
            })?;
        if !object.active
            || !object.is_unit
            || !host
                .object_is(state.unit.who, record.o as i32, type_id, 0)
                .map_err(SupplyAttritionTransactionError::Host)?
        {
            continue;
        }
        let radius = hero_radius_tiles(host, state.unit.who, record.o as i32, record.who as i32)?
            .wrapping_mul(SUPPORT_RANGE_UNITS_PER_TILE);
        if support_distance(state.unit, object) <= radius {
            return Ok(Some(record.o as i32));
        }
    }
    Ok(None)
}

fn resolve_registered_supply<H: ArenaSupplyAttritionHost>(
    state: SupplyAttritionUnitState,
    host: &H,
) -> Result<SupplyResolution, SupplyAttritionTransactionError<H::Error>> {
    let guards = SupplyGuards {
        already_flagged: state.unit_masks & 0x40_0000 != 0,
        supply_type: state.is_supply,
        militia: state.militia,
    };
    let guarded = if guards.already_flagged {
        Some(SupplyGuard::AlreadyFlagged)
    } else if guards.supply_type {
        Some(SupplyGuard::SupplyType)
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

    if let Some(slot) = find_registered_supply(state, host)? {
        return Ok(SupplyResolution {
            outcome: SupplyOutcome::Supplied(SupplySource::SuppliesIndex(slot)),
        });
    }

    for type_id in [0x16B, 0x176, 0x16E] {
        if host
            .owned_type_count(state.unit.who, type_id)
            .map_err(SupplyAttritionTransactionError::Host)?
            == 0
        {
            continue;
        }
        let heroes = host
            .hero_records(state.unit.who)
            .map_err(SupplyAttritionTransactionError::Host)?;
        for (slot, record) in heroes.iter().copied().enumerate() {
            if record.hero_flags & SUPPORT_REGISTRY_ACTIVE == 0 {
                continue;
            }
            let object = host
                .support_object(state.unit.who, record.o as i32)
                .map_err(SupplyAttritionTransactionError::Host)?
                .ok_or(SupplyAttritionTransactionError::MissingRegistryObject {
                    registry: SupportRegistry::Heroes,
                    slot,
                    who: state.unit.who,
                    o: record.o,
                })?;
            if !object.active
                || !object.is_unit
                || !host
                    .object_is(state.unit.who, record.o as i32, type_id, 0)
                    .map_err(SupplyAttritionTransactionError::Host)?
            {
                continue;
            }
            let radius =
                hero_radius_tiles(host, state.unit.who, record.o as i32, record.who as i32)?
                    .wrapping_mul(SUPPORT_RANGE_UNITS_PER_TILE);
            if support_distance(state.unit, object) <= radius {
                return Ok(SupplyResolution {
                    outcome: SupplyOutcome::Supplied(SupplySource::HeroObject {
                        type_id,
                        object_index: record.o as i32,
                    }),
                });
            }
        }
    }

    Ok(SupplyResolution {
        outcome: SupplyOutcome::Exhausted,
    })
}

/// Execute the exact `UnitData::in_supply` predicate used by `UnitData::recharge`.
/// Unlike `Unit::process_supply`, this returns true immediately for non-land domains and
/// friendly territory, then walks only `Supplies::find_supply`.
pub fn resolve_reload_supply<H: ArenaReloadSupplyHost>(
    who: i32,
    o: i32,
    host: &H,
) -> Result<ReloadSupplyState, SupplyAttritionTransactionError<H::Error>> {
    if o < 0 || !(0..NUM_LEADERS as i32).contains(&who) {
        return Err(SupplyAttritionTransactionError::InvalidUnit { who, o });
    }
    let state = host
        .unit_state(who, o)
        .map_err(SupplyAttritionTransactionError::Host)?
        .ok_or(SupplyAttritionTransactionError::MissingUnit { who, o })?;
    if state.unit.who != who || state.unit.o != o {
        return Err(SupplyAttritionTransactionError::UnitIdentityChanged {
            requested_who: who,
            requested_o: o,
            found_who: state.unit.who,
            found_o: state.unit.o,
        });
    }
    if state.domain != 0 {
        return Ok(ReloadSupplyState::NonLand);
    }
    let territory_who = host
        .territory_owner_at(state.unit.x, state.unit.y)
        .map_err(SupplyAttritionTransactionError::Host)?;
    if territory_who == who {
        return Ok(ReloadSupplyState::FriendlyTerritory);
    }
    Ok(match find_registered_supply(state, host)? {
        Some(slot) => ReloadSupplyState::SuppliesIndex(slot),
        None => ReloadSupplyState::OutOfSupply,
    })
}

/// Execute the supply-specific healing arm of `Unit::process_healing`
/// (`0x005E0F1A..0x005E0FFC`) for a live, isolated land object.
///
/// The shipped base rate is zero. French supply adds 20 frames; completed Versailles adds
/// 20 when it is the only source, or combines as `(rate + 20) / 4`. On a due phase the
/// engine walks `Supplies::find_supply` and calls `Unit::repair_damage(1,1,1)`.
pub fn execute_supply_healing<H: ArenaSupplyHealingHost>(
    frame: i32,
    who: i32,
    o: i32,
    host: &mut H,
) -> Result<SupplyHealingTransaction, SupplyAttritionTransactionError<H::Error>> {
    if o < 0 || !(0..NUM_LEADERS as i32).contains(&who) {
        return Err(SupplyAttritionTransactionError::InvalidUnit { who, o });
    }
    let state = host
        .unit_state(who, o)
        .map_err(SupplyAttritionTransactionError::Host)?
        .ok_or(SupplyAttritionTransactionError::MissingUnit { who, o })?;
    if state.unit.who != who || state.unit.o != o {
        return Err(SupplyAttritionTransactionError::UnitIdentityChanged {
            requested_who: who,
            requested_o: o,
            found_who: state.unit.who,
            found_o: state.unit.o,
        });
    }
    if state.damage <= 0 {
        return Ok(SupplyHealingTransaction::NoDamage);
    }
    if state.domain != 0 {
        return Ok(SupplyHealingTransaction::NotLand);
    }
    if state.is_supply {
        return Ok(SupplyHealingTransaction::SupplyUnit);
    }
    let mut rate = AttritionRules::default().supply_heal_rate;
    if host
        .french_supply_bonus(who)
        .map_err(SupplyAttritionTransactionError::Host)?
    {
        rate = rate.wrapping_add(20);
    }
    if host
        .completed_versailles(who)
        .map_err(SupplyAttritionTransactionError::Host)?
    {
        rate = if rate == 0 {
            20
        } else {
            rate.wrapping_add(20) / 4
        };
    }
    if rate == 0 {
        return Ok(SupplyHealingTransaction::Disabled);
    }
    if (frame.wrapping_add(i32::from(state.unit_id))) % rate != 0 {
        return Ok(SupplyHealingTransaction::NotDue { rate });
    }
    let Some(source_index) = find_registered_supply(state, host)? else {
        return Ok(SupplyHealingTransaction::OutOfSupply { rate });
    };
    if host
        .unsupported_prior_healing_source(who, o)
        .map_err(SupplyAttritionTransactionError::Host)?
    {
        return Err(SupplyAttritionTransactionError::UnsupportedPriorHealingSource { who, o });
    }
    let repair = host
        .repair_supply_damage(
            who,
            o,
            state.damage,
            state.healing,
            state.unit_masks,
            1,
            rate,
        )
        .map_err(SupplyAttritionTransactionError::Host)?;
    Ok(SupplyHealingTransaction::Healed {
        rate,
        source_index,
        repair,
    })
}

/// Execute the complete singleton Antipater/Wellington hero-aura healing arm of
/// `Unit::process_healing` (`0x005E09ED..0x005E0AE4`).
///
/// The optimized `LeaderData::num_units[0x137/0x13E]` loads are **unit-table slots**, not
/// TypeIndexes: the live table starts at TypeIndex `0x32`, so they identify Antipater
/// (`0x169`) and Wellington (`0x170`). Either enables the shipped 20-frame
/// `antipater_heal_rate`; `ObjectData::has_general(0, type)` then resolves the target
/// itself or the first in-radius live hero from the owner-local registry.
pub fn execute_hero_aura_healing<H: ArenaHeroAuraHealingHost>(
    frame: i32,
    who: i32,
    o: i32,
    host: &mut H,
) -> Result<HeroAuraHealingTransaction, SupplyAttritionTransactionError<H::Error>> {
    const ANTIPATER_HEAL_RATE: i32 = 20;
    const HERO_TYPES: [i32; 2] = [0x169, 0x170];

    if o < 0 || !(0..NUM_LEADERS as i32).contains(&who) {
        return Err(SupplyAttritionTransactionError::InvalidUnit { who, o });
    }
    let state = host
        .unit_state(who, o)
        .map_err(SupplyAttritionTransactionError::Host)?
        .ok_or(SupplyAttritionTransactionError::MissingUnit { who, o })?;
    if state.unit.who != who || state.unit.o != o {
        return Err(SupplyAttritionTransactionError::UnitIdentityChanged {
            requested_who: who,
            requested_o: o,
            found_who: state.unit.who,
            found_o: state.unit.o,
        });
    }
    if state.damage <= 0 {
        return Ok(HeroAuraHealingTransaction::NoDamage);
    }
    // The domain-one arm returns before this family. Air and land continue.
    if state.domain == 1 {
        return Ok(HeroAuraHealingTransaction::SeaUnit);
    }
    if state.is_supply {
        return Ok(HeroAuraHealingTransaction::SupplyUnit);
    }

    let mut owned = [false; HERO_TYPES.len()];
    for (slot, type_id) in HERO_TYPES.into_iter().enumerate() {
        owned[slot] = host
            .owned_type_count(who, type_id)
            .map_err(SupplyAttritionTransactionError::Host)?
            > 0;
    }
    if !owned.into_iter().any(|present| present) {
        return Ok(HeroAuraHealingTransaction::Disabled);
    }
    if frame.wrapping_add(i32::from(state.unit_id)) % ANTIPATER_HEAL_RATE != 0 {
        return Ok(HeroAuraHealingTransaction::NotDue {
            rate: ANTIPATER_HEAL_RATE,
        });
    }
    if state.type_308 != 1 || state.curr_uber_size != 1 {
        return Ok(HeroAuraHealingTransaction::BlockedComposition {
            rate: ANTIPATER_HEAL_RATE,
            blocker: HeroAuraHealingBlocker::MultiSlot {
                type_id: state.type_id,
                uber_size: state.type_308,
            },
        });
    }

    for (slot, hero_type_id) in HERO_TYPES.into_iter().enumerate() {
        if !owned[slot] {
            continue;
        }
        let Some(hero_object_index) = find_registered_hero_aura(state, hero_type_id, host)? else {
            continue;
        };
        let repair = host
            .repair_hero_aura_damage(
                who,
                o,
                state.damage,
                state.healing,
                state.unit_masks,
                1,
                ANTIPATER_HEAL_RATE,
            )
            .map_err(SupplyAttritionTransactionError::Host)?;
        return Ok(HeroAuraHealingTransaction::Healed {
            rate: ANTIPATER_HEAL_RATE,
            hero_type_id,
            hero_object_index,
            repair,
        });
    }

    Ok(HeroAuraHealingTransaction::OutsideAura {
        rate: ANTIPATER_HEAL_RATE,
    })
}

/// Execute the exact same-owner, ordinary-unit subdomain of the Iroquois healing arm in
/// `Unit::process_healing` (`0x005E0B49..0x005E0C90`).
///
/// `LeaderData::get_age` selects the shipped `{20,15,10,5}` frame rate. The exact
/// Antipater/Wellington family is sequenced by the live World before this call and the
/// exact patriot family after it. Civilian/merchant and multi-slot shapes remain typed
/// blockers; foreign territory consumes the live World's mutual diplomacy matrix outside
/// this bounded same-owner transaction.
pub fn execute_iroquois_healing<H: ArenaIroquoisHealingHost>(
    frame: i32,
    who: i32,
    o: i32,
    host: &mut H,
) -> Result<IroquoisHealingTransaction, SupplyAttritionTransactionError<H::Error>> {
    const RATES: [i32; 4] = [20, 15, 10, 5];

    if o < 0 || !(0..NUM_LEADERS as i32).contains(&who) {
        return Err(SupplyAttritionTransactionError::InvalidUnit { who, o });
    }
    let state = host
        .unit_state(who, o)
        .map_err(SupplyAttritionTransactionError::Host)?
        .ok_or(SupplyAttritionTransactionError::MissingUnit { who, o })?;
    if state.unit.who != who || state.unit.o != o {
        return Err(SupplyAttritionTransactionError::UnitIdentityChanged {
            requested_who: who,
            requested_o: o,
            found_who: state.unit.who,
            found_o: state.unit.o,
        });
    }
    if state.damage <= 0 {
        return Ok(IroquoisHealingTransaction::NoDamage);
    }
    if !host
        .iroquois_healing_bonus(who)
        .map_err(SupplyAttritionTransactionError::Host)?
    {
        return Ok(IroquoisHealingTransaction::OtherNation);
    }
    if state.unit_masks & 0x1000 != 0 {
        return Ok(IroquoisHealingTransaction::Suppressed);
    }
    if state.is_hero {
        return Ok(IroquoisHealingTransaction::HeroUnit);
    }
    if state.domain != 0 {
        return Ok(IroquoisHealingTransaction::NotLand);
    }

    let age = host
        .healing_age(who)
        .map_err(SupplyAttritionTransactionError::Host)?;
    let Some(&rate) = RATES.get(age) else {
        return Ok(IroquoisHealingTransaction::InvalidAge { age });
    };
    if frame.wrapping_add(i32::from(state.unit_id)) % rate != 0 {
        return Ok(IroquoisHealingTransaction::NotDue { rate });
    }

    if matches!(state.type_category, 5 | 8) || state.type_id == 0x13D {
        return Ok(IroquoisHealingTransaction::BlockedComposition {
            rate,
            blocker: IroquoisHealingBlocker::CivilianOrMerchant {
                type_id: state.type_id,
                type_category: state.type_category,
            },
        });
    }
    if state.type_308 != 1 || state.curr_uber_size != 1 {
        return Ok(IroquoisHealingTransaction::BlockedComposition {
            rate,
            blocker: IroquoisHealingBlocker::MultiSlot {
                type_id: state.type_id,
                uber_size: state.type_308,
            },
        });
    }
    if let Some(scenario_type_id) = host
        .scenario_healing_type()
        .map_err(SupplyAttritionTransactionError::Host)?
    {
        if host
            .object_is(who, o, scenario_type_id, 0)
            .map_err(SupplyAttritionTransactionError::Host)?
        {
            return Ok(IroquoisHealingTransaction::BlockedComposition {
                rate,
                blocker: IroquoisHealingBlocker::ScenarioHealing { scenario_type_id },
            });
        }
    }
    if host
        .completed_versailles(who)
        .map_err(SupplyAttritionTransactionError::Host)?
    {
        return Ok(IroquoisHealingTransaction::BlockedComposition {
            rate,
            blocker: IroquoisHealingBlocker::SupplyHealing,
        });
    }

    let territory_owner = host
        .territory_owner_at(state.unit.x, state.unit.y)
        .map_err(SupplyAttritionTransactionError::Host)?;
    if territory_owner < 0 {
        return Ok(IroquoisHealingTransaction::UnownedTerritory { rate });
    }
    if territory_owner != who {
        return Ok(IroquoisHealingTransaction::BlockedForeignTerritory {
            rate,
            territory_owner,
        });
    }

    let repair = host
        .repair_iroquois_damage(
            who,
            o,
            state.damage,
            state.healing,
            state.unit_masks,
            1,
            rate,
        )
        .map_err(SupplyAttritionTransactionError::Host)?;
    Ok(IroquoisHealingTransaction::Healed { rate, repair })
}

#[derive(Clone, Copy)]
enum PatriotArmPreflight {
    NoSource,
    UnownedTerritory,
    NonAlliedTerritory(i32),
    GuardedCeoTarget,
    OutsideAura,
    Aura(i32),
}

fn commit_patriot_arm<H: ArenaPatriotHealingHost>(
    preflight: PatriotArmPreflight,
    rate: i32,
    who: i32,
    o: i32,
    host: &mut H,
) -> Result<PatriotHealingArmTransaction, SupplyAttritionTransactionError<H::Error>> {
    let hero_object_index = match preflight {
        PatriotArmPreflight::NoSource => return Ok(PatriotHealingArmTransaction::NoSource),
        PatriotArmPreflight::UnownedTerritory => {
            return Ok(PatriotHealingArmTransaction::UnownedTerritory);
        }
        PatriotArmPreflight::NonAlliedTerritory(territory_owner) => {
            return Ok(PatriotHealingArmTransaction::NonAlliedTerritory { territory_owner });
        }
        PatriotArmPreflight::GuardedCeoTarget => {
            return Ok(PatriotHealingArmTransaction::GuardedCeoTarget);
        }
        PatriotArmPreflight::OutsideAura => return Ok(PatriotHealingArmTransaction::OutsideAura),
        PatriotArmPreflight::Aura(hero_object_index) => hero_object_index,
    };

    // Every arm preflights before the first write. Reload only the target mutation fields
    // here so consecutive qualifying patriots compose exactly rather than using a stale
    // damage/healing/mask snapshot from the beginning of the family.
    let state = host
        .unit_state(who, o)
        .map_err(SupplyAttritionTransactionError::Host)?
        .ok_or(SupplyAttritionTransactionError::MissingUnit { who, o })?;
    if state.unit.who != who || state.unit.o != o {
        return Err(SupplyAttritionTransactionError::UnitIdentityChanged {
            requested_who: who,
            requested_o: o,
            found_who: state.unit.who,
            found_o: state.unit.o,
        });
    }
    let repair = host
        .repair_patriot_damage(
            who,
            o,
            state.damage,
            state.healing,
            state.unit_masks,
            1,
            rate,
        )
        .map_err(SupplyAttritionTransactionError::Host)?;
    Ok(PatriotHealingArmTransaction::Healed {
        hero_object_index,
        repair,
    })
}

/// Execute the complete singleton Senator/President/CEO aura family at
/// `Unit::process_healing` `0x005E0C90..0x005E0F1A`.
///
/// The three optimized count slots `0x12F/0x131/0x133` map through the live UnitType-table
/// base `0x32` to TypeIndexes `0x161/0x163/0x165`. Senator requires owned allied
/// territory; President accepts unowned or allied territory; CEO has no territory read
/// and instead applies the recovered `unit_masks`/`unit_masks2` guard. All three use the
/// 20-frame scalar at `Rules +0x698`, walk `ObjectData::has_general(0, type)` in branch
/// order, and may each repair one point in the same call. `process_entered_with_damage`
/// retains the single damage gate at the top of `Unit::process_healing`; an earlier family
/// reaching zero does not suppress these later branches in retail.
pub fn execute_patriot_healing<H: ArenaPatriotHealingHost>(
    frame: i32,
    who: i32,
    o: i32,
    process_entered_with_damage: bool,
    host: &mut H,
) -> Result<PatriotHealingTransaction, SupplyAttritionTransactionError<H::Error>> {
    const PATRIOT_HEAL_RATE: i32 = 20;
    const SENATOR: i32 = 0x161;
    const PRESIDENT: i32 = 0x163;
    const CEO: i32 = 0x165;

    if o < 0 || !(0..NUM_LEADERS as i32).contains(&who) {
        return Err(SupplyAttritionTransactionError::InvalidUnit { who, o });
    }
    if !process_entered_with_damage {
        return Ok(PatriotHealingTransaction::NoDamage);
    }
    let state = host
        .unit_state(who, o)
        .map_err(SupplyAttritionTransactionError::Host)?
        .ok_or(SupplyAttritionTransactionError::MissingUnit { who, o })?;
    if state.unit.who != who || state.unit.o != o {
        return Err(SupplyAttritionTransactionError::UnitIdentityChanged {
            requested_who: who,
            requested_o: o,
            found_who: state.unit.who,
            found_o: state.unit.o,
        });
    }
    if state.domain == 1 {
        return Ok(PatriotHealingTransaction::SeaUnit);
    }
    if state.is_supply {
        return Ok(PatriotHealingTransaction::SupplyUnit);
    }
    if PATRIOT_HEAL_RATE == 0 {
        return Ok(PatriotHealingTransaction::Disabled);
    }
    if frame.wrapping_add(i32::from(state.unit_id)) % PATRIOT_HEAL_RATE != 0 {
        return Ok(PatriotHealingTransaction::NotDue {
            rate: PATRIOT_HEAL_RATE,
        });
    }
    if state.type_308 != 1 || state.curr_uber_size != 1 {
        return Ok(PatriotHealingTransaction::BlockedComposition {
            rate: PATRIOT_HEAL_RATE,
            blocker: PatriotHealingBlocker::MultiSlot {
                type_id: state.type_id,
                uber_size: state.type_308,
            },
        });
    }

    let senator = if host
        .owned_type_count(who, SENATOR)
        .map_err(SupplyAttritionTransactionError::Host)?
        <= 0
    {
        PatriotArmPreflight::NoSource
    } else {
        let territory_owner = host
            .territory_owner_at(state.unit.x, state.unit.y)
            .map_err(SupplyAttritionTransactionError::Host)?;
        if territory_owner < 0 {
            PatriotArmPreflight::UnownedTerritory
        } else if !host
            .is_allied(who, territory_owner)
            .map_err(SupplyAttritionTransactionError::Host)?
        {
            PatriotArmPreflight::NonAlliedTerritory(territory_owner)
        } else {
            match find_registered_hero_aura(state, SENATOR, host)? {
                Some(source) => PatriotArmPreflight::Aura(source),
                None => PatriotArmPreflight::OutsideAura,
            }
        }
    };

    let president = if host
        .owned_type_count(who, PRESIDENT)
        .map_err(SupplyAttritionTransactionError::Host)?
        <= 0
    {
        PatriotArmPreflight::NoSource
    } else {
        let territory_owner = host
            .territory_owner_at(state.unit.x, state.unit.y)
            .map_err(SupplyAttritionTransactionError::Host)?;
        if territory_owner >= 0
            && !host
                .is_allied(who, territory_owner)
                .map_err(SupplyAttritionTransactionError::Host)?
        {
            PatriotArmPreflight::NonAlliedTerritory(territory_owner)
        } else {
            match find_registered_hero_aura(state, PRESIDENT, host)? {
                Some(source) => PatriotArmPreflight::Aura(source),
                None => PatriotArmPreflight::OutsideAura,
            }
        }
    };

    let ceo = if host
        .owned_type_count(who, CEO)
        .map_err(SupplyAttritionTransactionError::Host)?
        <= 0
    {
        PatriotArmPreflight::NoSource
    } else if state.unit_masks & 0x80 != 0
        && state.unit_masks2 & RESUPPLIED_THIS_TICK == 0
        && !state.is_supply
    {
        PatriotArmPreflight::GuardedCeoTarget
    } else {
        match find_registered_hero_aura(state, CEO, host)? {
            Some(source) => PatriotArmPreflight::Aura(source),
            None => PatriotArmPreflight::OutsideAura,
        }
    };

    let senator = commit_patriot_arm(senator, PATRIOT_HEAL_RATE, who, o, host)?;
    let president = commit_patriot_arm(president, PATRIOT_HEAL_RATE, who, o, host)?;
    let ceo = commit_patriot_arm(ceo, PATRIOT_HEAL_RATE, who, o, host)?;
    Ok(PatriotHealingTransaction::Processed {
        rate: PATRIOT_HEAL_RATE,
        senator,
        president,
        ceo,
    })
}

/// Execute the exact friendly-land worker subdomain of the final civilian arm in
/// `Unit::process_healing` (`0x005E1000..0x005E110D`).
///
/// The shipped rate is 45 frames. Retail rejects the due call while under attack, accepts
/// four literal worker TypeIndexes, then repairs on sea or allied territory. Arena has no
/// diplomacy matrix, so same-owner land is exact, unowned land is an exact no-op, and a
/// foreign owner is a typed authority boundary. The live World sequences the supported
/// hero and patriot auras first; Iroquois/supply and multi-slot composition remain explicit
/// instead of being silently combined here.
pub fn execute_worker_healing<H: ArenaWorkerHealingHost>(
    frame: i32,
    who: i32,
    o: i32,
    host: &mut H,
) -> Result<WorkerHealingTransaction, SupplyAttritionTransactionError<H::Error>> {
    const CIVILIAN_HEAL_RATE: i32 = 45;

    if o < 0 || !(0..NUM_LEADERS as i32).contains(&who) {
        return Err(SupplyAttritionTransactionError::InvalidUnit { who, o });
    }
    let state = host
        .unit_state(who, o)
        .map_err(SupplyAttritionTransactionError::Host)?
        .ok_or(SupplyAttritionTransactionError::MissingUnit { who, o })?;
    if state.unit.who != who || state.unit.o != o {
        return Err(SupplyAttritionTransactionError::UnitIdentityChanged {
            requested_who: who,
            requested_o: o,
            found_who: state.unit.who,
            found_o: state.unit.o,
        });
    }
    if state.damage <= 0 {
        return Ok(WorkerHealingTransaction::NoDamage);
    }
    if frame.wrapping_add(i32::from(state.unit_id)) % CIVILIAN_HEAL_RATE != 0 {
        return Ok(WorkerHealingTransaction::NotDue {
            rate: CIVILIAN_HEAL_RATE,
        });
    }
    if state.unit_masks2 & 1 != 0 {
        return Ok(WorkerHealingTransaction::UnderAttack {
            rate: CIVILIAN_HEAL_RATE,
        });
    }
    if !matches!(state.type_id, 0x32..=0x35) {
        return Ok(WorkerHealingTransaction::NotWorker {
            rate: CIVILIAN_HEAL_RATE,
        });
    }
    if state.domain != 0 {
        return Ok(WorkerHealingTransaction::NotLand {
            rate: CIVILIAN_HEAL_RATE,
        });
    }
    if state.type_308 != 1 || state.curr_uber_size != 1 {
        return Ok(WorkerHealingTransaction::BlockedPriorFamily {
            rate: CIVILIAN_HEAL_RATE,
            blocker: WorkerHealingBlocker::MultiSlot {
                type_id: state.type_id,
                uber_size: state.type_308,
            },
        });
    }
    if host
        .iroquois_healing_bonus(who)
        .map_err(SupplyAttritionTransactionError::Host)?
    {
        return Ok(WorkerHealingTransaction::BlockedPriorFamily {
            rate: CIVILIAN_HEAL_RATE,
            blocker: WorkerHealingBlocker::IroquoisHealing,
        });
    }
    if host
        .french_supply_bonus(who)
        .map_err(SupplyAttritionTransactionError::Host)?
        || host
            .completed_versailles(who)
            .map_err(SupplyAttritionTransactionError::Host)?
    {
        return Ok(WorkerHealingTransaction::BlockedPriorFamily {
            rate: CIVILIAN_HEAL_RATE,
            blocker: WorkerHealingBlocker::SupplyHealing,
        });
    }

    let territory_owner = host
        .territory_owner_at(state.unit.x, state.unit.y)
        .map_err(SupplyAttritionTransactionError::Host)?;
    if territory_owner < 0 {
        return Ok(WorkerHealingTransaction::UnownedTerritory {
            rate: CIVILIAN_HEAL_RATE,
        });
    }
    if territory_owner != who {
        return Ok(WorkerHealingTransaction::BlockedForeignTerritory {
            rate: CIVILIAN_HEAL_RATE,
            territory_owner,
        });
    }

    let repair = host
        .repair_worker_damage(
            who,
            o,
            state.damage,
            state.healing,
            state.unit_masks,
            1,
            CIVILIAN_HEAL_RATE,
        )
        .map_err(SupplyAttritionTransactionError::Host)?;
    Ok(WorkerHealingTransaction::Healed {
        rate: CIVILIAN_HEAL_RATE,
        repair,
    })
}

/// Execute the exact universal prefix and friendly-territory return of
/// `Unit::process_attrition` (`0x005E11A0`) from its 32-frame `Unit::process` call site.
///
/// Retail first clears `RESUPPLIED_THIS_TICK`, then `process_attrition` clears
/// `unit_masks & 0x400080` and resets the signed attrition period to zero. Friendly land
/// returns with that state. Non-friendly territory reaches predicates which require the
/// diplomacy/leader/object graph, so the transaction reports that boundary without
/// inventing a replacement period.
pub fn execute_attrition_recompute<H: ArenaAttritionRecomputeHost>(
    frame: i32,
    who: i32,
    o: i32,
    host: &mut H,
) -> Result<AttritionRecomputeTransaction, SupplyAttritionTransactionError<H::Error>> {
    if o < 0 || !(0..NUM_LEADERS as i32).contains(&who) {
        return Err(SupplyAttritionTransactionError::InvalidUnit { who, o });
    }
    let state = host
        .unit_state(who, o)
        .map_err(SupplyAttritionTransactionError::Host)?
        .ok_or(SupplyAttritionTransactionError::MissingUnit { who, o })?;
    if state.unit.who != who || state.unit.o != o {
        return Err(SupplyAttritionTransactionError::UnitIdentityChanged {
            requested_who: who,
            requested_o: o,
            found_who: state.unit.who,
            found_o: state.unit.o,
        });
    }
    if frame.wrapping_add(i32::from(state.unit_id)) % 32 != 0 {
        return Ok(AttritionRecomputeTransaction::NotDue);
    }

    let unit_masks_after = state.unit_masks & !0x40_0080;
    let unit_masks2_after = state.unit_masks2 & !RESUPPLIED_THIS_TICK;
    host.write_attrition_recompute_prefix(
        who,
        o,
        state.unit_masks,
        unit_masks_after,
        state.unit_masks2,
        unit_masks2_after,
        state.attrition_period,
        0,
    )
    .map_err(SupplyAttritionTransactionError::Host)?;

    let territory_owner = host
        .territory_owner_at(state.unit.x, state.unit.y)
        .map_err(SupplyAttritionTransactionError::Host)?;
    if territory_owner == who {
        Ok(AttritionRecomputeTransaction::FriendlyTerritoryReset {
            unit_masks_before: state.unit_masks,
            unit_masks_after,
            unit_masks2_before: state.unit_masks2,
            unit_masks2_after,
            period_before: state.attrition_period,
        })
    } else {
        Ok(AttritionRecomputeTransaction::BlockedNonFriendlyTerritory {
            territory_owner,
            unit_masks_before: state.unit_masks,
            unit_masks_after,
            unit_masks2_before: state.unit_masks2,
            unit_masks2_after,
            period_before: state.attrition_period,
        })
    }
}

/// Execute the sim-state part of retail's supply/attrition branch for one live unit.
///
/// The transaction performs no registry read before the exact `(frame + id) % period`
/// due check. When due, it traverses the walked `Supplies` list, then the three
/// count-gated `Heroes` searches in retail order. A hit ORs `0x40000` into the live walked
/// `unit_masks2`; otherwise it executes the recovered attrition damage shape and, when
/// the object survives, the same graphic callback selected by `suffer_attrition(1)`.
pub fn execute_supply_attrition<H: ArenaSupplyAttritionHost>(
    frame: i32,
    who: i32,
    o: i32,
    host: &mut H,
) -> Result<SupplyAttritionTransaction, SupplyAttritionTransactionError<H::Error>> {
    if o < 0 || !(0..NUM_LEADERS as i32).contains(&who) {
        return Err(SupplyAttritionTransactionError::InvalidUnit { who, o });
    }
    let state = host
        .unit_state(who, o)
        .map_err(SupplyAttritionTransactionError::Host)?
        .ok_or(SupplyAttritionTransactionError::MissingUnit { who, o })?;
    if state.unit.who != who || state.unit.o != o {
        return Err(SupplyAttritionTransactionError::UnitIdentityChanged {
            requested_who: who,
            requested_o: o,
            found_who: state.unit.who,
            found_o: state.unit.o,
        });
    }
    if !borders_fog::attrition_due(frame, state.unit_id, state.attrition_period) {
        return Ok(SupplyAttritionTransaction::NotDue);
    }

    let supply = resolve_registered_supply(state, host)?;
    if let SupplyOutcome::Supplied(source) = supply.outcome() {
        let before = state.unit_masks2;
        let after = before | RESUPPLIED_THIS_TICK;
        host.write_unit_masks2(who, o, before, after)
            .map_err(SupplyAttritionTransactionError::Host)?;
        return Ok(SupplyAttritionTransaction::Resupplied {
            source,
            unit_masks2_before: before,
            unit_masks2_after: after,
        });
    }

    let damage = borders_fog::attrition_damage(state.type_308, state.curr_uber_size);
    let outcome = host
        .take_attrition_damage(who, o, damage)
        .map_err(SupplyAttritionTransactionError::Host)?;
    let graphic_attrition = outcome == DamageOutcome::Survived;
    if graphic_attrition {
        host.suffer_graphic_attrition(who, o)
            .map_err(SupplyAttritionTransactionError::Host)?;
    }
    Ok(SupplyAttritionTransaction::Damaged {
        supply: supply.outcome(),
        damage,
        outcome,
        graphic_attrition,
    })
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
        hero_16b: Option<i32>,
        hero_176: Option<i32>,
        hero_16e: Option<i32>,
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

        fn find_owned_hero_in_range(
            &mut self,
            unit: SupplyUnitKey,
            type_id: i32,
        ) -> Result<Option<i32>, Self::Error> {
            self.calls.push(type_id);
            self.units.push(unit);
            if self.fail_on == Some(type_id) {
                return Err("hero traversal unavailable");
            }
            Ok(match type_id {
                0x16B => self.hero_16b,
                0x176 => self.hero_176,
                0x16E => self.hero_16e,
                _ => unreachable!("adapter requested an underived supply hero"),
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
            supply_type: false,
            militia: false,
        };
        let mut queries = ScriptedSupplyQueries {
            hero_176: Some(73),
            ..ScriptedSupplyQueries::default()
        };
        assert_eq!(
            resolve_supply(unit, open, &mut queries).unwrap().outcome(),
            SupplyOutcome::Supplied(SupplySource::HeroObject {
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
        queries.hero_176 = None;
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
            supply_type: false,
            militia: false,
        };
        let mut missing = ScriptedSupplyQueries {
            fail_on: Some(0x16B),
            ..ScriptedSupplyQueries::default()
        };
        assert_eq!(
            resolve_supply(unit, open, &mut missing),
            Err(SupplyAdapterError::Host("hero traversal unavailable"))
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

    #[test]
    fn supply_attrition_transaction_traverses_walked_sources_and_commits_one_branch() {
        use std::cell::RefCell;

        struct Host {
            unit: SupplyAttritionUnitState,
            supplies: Vec<SupplyRegistryRecord>,
            heroes: Vec<HeroRegistryRecord>,
            objects: Vec<Option<SupplySearchObject>>,
            object_types: Vec<i32>,
            counts: [i32; 3],
            reads: RefCell<Vec<String>>,
            writes: Vec<(u32, u32)>,
            damages: Vec<AttritionDamage>,
            graphics: usize,
        }

        impl ArenaSupplyAttritionHost for Host {
            type Error = &'static str;

            fn unit_state(
                &self,
                who: i32,
                o: i32,
            ) -> Result<Option<SupplyAttritionUnitState>, Self::Error> {
                self.reads.borrow_mut().push(format!("unit:{who}:{o}"));
                Ok(Some(self.unit))
            }

            fn supply_records(&self, who: i32) -> Result<&[SupplyRegistryRecord], Self::Error> {
                self.reads.borrow_mut().push(format!("supplies:{who}"));
                Ok(&self.supplies)
            }

            fn hero_records(&self, who: i32) -> Result<&[HeroRegistryRecord], Self::Error> {
                self.reads.borrow_mut().push(format!("heroes:{who}"));
                Ok(&self.heroes)
            }

            fn support_object(
                &self,
                who: i32,
                o: i32,
            ) -> Result<Option<SupplySearchObject>, Self::Error> {
                self.reads.borrow_mut().push(format!("object:{who}:{o}"));
                Ok(self.objects.get(o as usize).copied().flatten())
            }

            fn supply_radius_facts(&self, who: i32) -> Result<SupplyRadiusFacts, Self::Error> {
                self.reads.borrow_mut().push(format!("supply-radius:{who}"));
                Ok(SupplyRadiusFacts {
                    supply_radius: 14,
                    supply_radius_upgrade: 2,
                    supply_upgrade: 0,
                    has_terra_cotta: false,
                    terra_cotta_range: 3,
                })
            }

            fn owned_type_count(&self, _who: i32, type_id: i32) -> Result<i32, Self::Error> {
                self.reads.borrow_mut().push(format!("count:{type_id}"));
                Ok(self.counts[match type_id {
                    0x16B => 0,
                    0x176 => 1,
                    0x16E => 2,
                    _ => return Err("unexpected type count"),
                }])
            }

            fn object_is(
                &self,
                _who: i32,
                o: i32,
                type_id: i32,
                relation_set: i32,
            ) -> Result<bool, Self::Error> {
                self.reads
                    .borrow_mut()
                    .push(format!("object-is:{o}:{type_id}:{relation_set}"));
                Ok(self.object_types[o as usize] == type_id)
            }

            fn hero_radius_facts(&self, who: i32) -> Result<HeroRadiusFacts, Self::Error> {
                self.reads.borrow_mut().push(format!("hero-radius:{who}"));
                Ok(HeroRadiusFacts {
                    general_upgrade: 1,
                    general_radius: 4,
                    parmenio_radius_adjust_256: 256,
                    wellington_radius_percent: 100,
                    kutosov_radius_percent: 100,
                    has_terra_cotta: false,
                    terra_cotta_range: 3,
                    military_patriot_radius_bonus: 0,
                    economic_patriot_radius_bonus: 0,
                })
            }

            fn write_unit_masks2(
                &mut self,
                who: i32,
                o: i32,
                before: u32,
                after: u32,
            ) -> Result<(), Self::Error> {
                if (who, o, before) != (self.unit.unit.who, self.unit.unit.o, self.unit.unit_masks2)
                {
                    return Err("stale unit_masks2 write");
                }
                self.unit.unit_masks2 = after;
                self.writes.push((before, after));
                Ok(())
            }

            fn take_attrition_damage(
                &mut self,
                who: i32,
                o: i32,
                damage: AttritionDamage,
            ) -> Result<DamageOutcome, Self::Error> {
                if (who, o) != (self.unit.unit.who, self.unit.unit.o) {
                    return Err("damage identity changed");
                }
                self.damages.push(damage);
                Ok(DamageOutcome::Survived)
            }

            fn suffer_graphic_attrition(&mut self, who: i32, o: i32) -> Result<(), Self::Error> {
                if (who, o) != (self.unit.unit.who, self.unit.unit.o) {
                    return Err("graphic identity changed");
                }
                self.graphics += 1;
                Ok(())
            }
        }

        let unit = SupplyUnitKey {
            who: 2,
            o: 18,
            x: 10_000,
            y: 5_000,
        };
        let mut objects = vec![None; 19];
        // Exact axis boundary: vector_dist is 14 * 0xC0, so this source is included.
        objects[10] = Some(SupplySearchObject {
            active: true,
            is_unit: true,
            x: unit.x + 14 * SUPPORT_RANGE_UNITS_PER_TILE,
            y: unit.y,
        });
        objects[11] = Some(SupplySearchObject {
            active: true,
            is_unit: true,
            x: unit.x + 8 * SUPPORT_RANGE_UNITS_PER_TILE,
            y: unit.y,
        });
        let mut object_types = vec![-1; 19];
        object_types[11] = 0x176;
        let mut host = Host {
            unit: SupplyAttritionUnitState {
                unit,
                unit_id: 0,
                type_id: 0x32,
                type_category: 5,
                attrition_period: 48,
                damage: 0,
                healing: 0,
                unit_masks: 0,
                unit_masks2: 0x20,
                is_supply: false,
                is_hero: false,
                militia: false,
                domain: 0,
                type_308: 0,
                curr_uber_size: 4,
            },
            supplies: vec![
                SupplyRegistryRecord {
                    supply: 0,
                    o: 9,
                    supply_flags: 0,
                    who: 2,
                },
                SupplyRegistryRecord {
                    supply: 41,
                    o: 10,
                    supply_flags: SUPPORT_REGISTRY_ACTIVE,
                    who: 2,
                },
            ],
            heroes: vec![HeroRegistryRecord {
                hero: 0,
                o: 11,
                hero_flags: SUPPORT_REGISTRY_ACTIVE,
                who: 2,
            }],
            objects,
            object_types,
            counts: [0, 1, 0],
            reads: RefCell::default(),
            writes: Vec::new(),
            damages: Vec::new(),
            graphics: 0,
        };

        assert_eq!(
            execute_supply_attrition(47, 2, 18, &mut host).unwrap(),
            SupplyAttritionTransaction::NotDue
        );
        assert_eq!(&*host.reads.borrow(), &["unit:2:18"]);

        host.reads.borrow_mut().clear();
        assert_eq!(
            execute_supply_attrition(48, 2, 18, &mut host).unwrap(),
            SupplyAttritionTransaction::Resupplied {
                source: SupplySource::SuppliesIndex(1),
                unit_masks2_before: 0x20,
                unit_masks2_after: 0x20 | RESUPPLIED_THIS_TICK,
            }
        );
        assert_eq!(host.writes, [(0x20, 0x20 | RESUPPLIED_THIS_TICK)]);
        assert!(host
            .reads
            .borrow()
            .iter()
            .all(|read| !read.starts_with("count:") && !read.starts_with("heroes:")));

        // One fine unit outside the supply radius forces the ordered 16B -> 176 fallback;
        // the walked HeroData source lies exactly on its own inclusive boundary.
        host.unit.unit_masks2 = 0x40;
        host.objects[10].as_mut().unwrap().x += 1;
        host.reads.borrow_mut().clear();
        assert_eq!(
            execute_supply_attrition(48, 2, 18, &mut host).unwrap(),
            SupplyAttritionTransaction::Resupplied {
                source: SupplySource::HeroObject {
                    type_id: 0x176,
                    object_index: 11,
                },
                unit_masks2_before: 0x40,
                unit_masks2_after: 0x40 | RESUPPLIED_THIS_TICK,
            }
        );
        let reads = host.reads.borrow();
        let count_16b = reads.iter().position(|v| v == "count:363").unwrap();
        let count_176 = reads.iter().position(|v| v == "count:374").unwrap();
        let heroes = reads.iter().position(|v| v == "heroes:2").unwrap();
        assert!(count_16b < count_176 && count_176 < heroes);
        drop(reads);

        // The same one-unit radius mutation now exhausts every source and commits damage,
        // never a mask write. A surviving take_damage result triggers the graphic hook.
        host.unit.unit_masks2 = 0x80;
        host.objects[11].as_mut().unwrap().x += 1;
        let writes_before = host.writes.len();
        assert_eq!(
            execute_supply_attrition(48, 2, 18, &mut host).unwrap(),
            SupplyAttritionTransaction::Damaged {
                supply: SupplyOutcome::Exhausted,
                damage: AttritionDamage {
                    flat: 0,
                    fractional: 4,
                },
                outcome: DamageOutcome::Survived,
                graphic_attrition: true,
            }
        );
        assert_eq!(host.writes.len(), writes_before);
        assert_eq!(
            host.damages,
            [AttritionDamage {
                flat: 0,
                fractional: 4,
            }]
        );
        assert_eq!(host.graphics, 1);
    }
}
