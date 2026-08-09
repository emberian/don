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
        status: IntegrationStatus::Blocked,
        recovered: &[
            "map_terrain water/WATERHALF storage and predicates",
            "naval WaterWorld coordinate and coast predicates",
        ],
        missing: &[
            "retail map water generation and continent construction",
            "tile/water astar_path domains and calc_cost",
            "water-region territory propagation",
        ],
    },
    IntegrationItem {
        subsystem: Model6Subsystem::Naval,
        status: IntegrationStatus::Blocked,
        recovered: &[
            "naval shipped roster and static predicates",
            "dock registry bookkeeping and conditional gull RNG fragment",
            "fish candidate scan and fixed retail move tables",
        ],
        missing: &[
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
            "flight band, fuel, capacity and air-order local transitions",
        ],
        missing: &[
            "Unit::do_air_physics",
            "retail air target/host scans and stable traversal order",
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
        ],
    },
    IntegrationItem {
        subsystem: Model6Subsystem::Supply,
        status: IntegrationStatus::AdapterOnly,
        recovered: &[
            "Unit::process_supply short-circuit predicate",
            "out-of-supply reload arithmetic and constants",
        ],
        missing: &[
            "Supplies::find_supply and source lifetime/index",
            "three building proximity queries in retail traversal order",
            "located reload call site and supply healing",
        ],
    },
];

/// Explicit `LeaderData::diplos` state for an arena host. This is simulation state—not bot
/// memory—and remains public so serialization/checksum work cannot accidentally omit it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiplomacyState {
    pub declared: [[i32; NUM_LEADERS]; NUM_LEADERS],
}

impl DiplomacyState {
    /// Retail's leader reset initializes declarations to war. Self is still reported as an
    /// ally by `get_diplo`; the diagonal storage cells are not special-cased here.
    pub const fn at_war() -> Self {
        Self {
            declared: [[Diplo::War as i32; NUM_LEADERS]; NUM_LEADERS],
        }
    }

    pub fn declare(&mut self, who: usize, other: usize, state: Diplo) -> Result<(), PlayerSlot> {
        if who >= NUM_LEADERS {
            return Err(PlayerSlot(who));
        }
        if other >= NUM_LEADERS {
            return Err(PlayerSlot(other));
        }
        self.declared[who][other] = state as i32;
        Ok(())
    }

    pub fn relation(&self, who: usize, other: usize) -> Result<Diplo, PlayerSlot> {
        if who >= NUM_LEADERS {
            return Err(PlayerSlot(who));
        }
        if other >= NUM_LEADERS {
            return Err(PlayerSlot(other));
        }
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerSlot(pub usize);

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

/// Fuel transition over exact live type fields. Host discovery remains a mandatory caller
/// result; [`AirHostSearch::Exhausted`] means the retail scans completed and found nothing,
/// not “unmodeled”.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirHostSearch {
    Found { o: i32, who: i32 },
    Exhausted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirAdapterError {
    NotAirDomain { type_id: i32 },
}

#[allow(clippy::too_many_arguments)]
pub fn air_fuel_transition(
    ty: &TypeRow,
    mana_burn: i16,
    on_map: bool,
    has_space_program: bool,
    space_air_range_pct: i32,
    returning: i32,
    // A currently assigned host only when the caller's retail capacity test passed.
    current_host: Option<(i32, i32)>,
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
    let nearest_host = match nearest_host {
        AirHostSearch::Found { o, who } => Some((o, who)),
        AirHostSearch::Exhausted => None,
    };
    let (returning, verdict) = air::check_fuel(
        &ty,
        returning,
        left,
        current_host.is_some(),
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

#[derive(Clone, Copy, Debug)]
pub struct SupplyFacts {
    pub already_flagged: bool,
    pub always_supplied: bool,
    pub militia: bool,
    /// Result of a completed `Supplies::find_supply` query.
    pub near_supply_source: bool,
    /// Results of the three ordered building proximity queries.
    pub near_building_16b: bool,
    pub near_building_176: bool,
    pub near_building_16e: bool,
}

impl SupplyFacts {
    fn retail(self) -> SupplyInput {
        SupplyInput {
            already_flagged: self.already_flagged,
            always_supplied: self.always_supplied,
            militia: self.militia,
            near_supply_source: self.near_supply_source,
            near_building_16b: self.near_building_16b,
            near_building_176: self.near_building_176,
            near_building_16e: self.near_building_16e,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AttritionTickInput {
    pub frame: i32,
    pub unit_id: i16,
    pub period: i16,
    /// Every world query consumed by `Unit::process_supply`, already resolved by the host.
    pub supply: SupplyFacts,
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
    if borders_fog::supply_state(&input.supply.retail()) {
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
        d.declare(0, 1, Diplo::Ally).unwrap();
        d.declare(1, 0, Diplo::Peace).unwrap();
        assert_eq!(d.relation(0, 1), Ok(Diplo::Peace));
        d.declare(1, 0, Diplo::Ally).unwrap();
        assert_eq!(d.relation(0, 1), Ok(Diplo::Ally));
        assert_eq!(d.relation(NUM_LEADERS, 0), Err(PlayerSlot(NUM_LEADERS)));
    }

    #[test]
    fn fuel_adapter_owns_no_host_search_or_rng_state() {
        let ty = row();
        let (burn, returning, verdict) =
            air_fuel_transition(&ty, 2, true, false, 0, 0, None, AirHostSearch::Exhausted).unwrap();
        assert_eq!(burn, 3);
        assert_eq!(returning, 1);
        assert_eq!(verdict, FuelVerdict::Crash);

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
                None,
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
    fn attrition_adapter_distinguishes_timing_supply_and_damage() {
        let mut input = AttritionTickInput {
            frame: 47,
            unit_id: 0,
            period: 48,
            supply: SupplyFacts {
                already_flagged: false,
                always_supplied: false,
                militia: false,
                near_supply_source: true,
                near_building_16b: false,
                near_building_176: false,
                near_building_16e: false,
            },
            type_308: 1,
            curr_uber_size: 4,
        };
        assert_eq!(attrition_tick(&input), AttritionTick::NotDue);
        input.frame = 48;
        assert_eq!(attrition_tick(&input), AttritionTick::SupplyFound);
        input.supply.near_supply_source = false;
        assert_eq!(
            attrition_tick(&input),
            AttritionTick::Damage(AttritionDamage {
                flat: 1,
                fractional: 0,
            })
        );
    }
}
