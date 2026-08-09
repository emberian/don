//! Fail-closed adapters for Arena MODEL 6 systems.
//!
//! This module does not make the arena naval or airborne by declaration. It names the
//! recovered retail kernels that are safe to call, keeps every world-query prerequisite
//! explicit, and inventories the missing host systems. `arena::world` may consume these
//! adapters only when it owns the corresponding state; callers cannot obtain a convenient
//! default for water reachability, supply coverage, air target search, or diplomacy AI.

use crate::arena::types::TypeRow;
use don_sim::rng::Random;
use don_sim::systems::air::{self, AntiAirGate, AntiAirShot, FuelVerdict};
use don_sim::systems::borders_fog::{
    self, AttritionDamage, AttritionInput, AttritionRules, SupplyInput,
};
use don_sim::systems::victory_score::{self, Diplo, NUM_LEADERS};

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
/// result; passing `None` means the retail scans completed and found nothing, not “unmodeled”.
#[allow(clippy::too_many_arguments)]
pub fn air_fuel_transition(
    ty: &TypeRow,
    mana_burn: i16,
    on_map: bool,
    has_space_program: bool,
    space_air_range_pct: i32,
    returning: i32,
    current_host_ok: bool,
    current_host: Option<(i32, i32)>,
    nearest_host: Option<(i32, i32)>,
) -> (i16, i32, FuelVerdict) {
    let ty = ty.air_type_data();
    let cap = air::mana_cap(&ty, has_space_program, space_air_range_pct);
    let burn = air::air_fuel_step(mana_burn, cap, on_map);
    let left = (cap - burn as i32).max(0);
    let (returning, verdict) = air::check_fuel(
        &ty,
        returning,
        left,
        current_host_ok,
        current_host,
        nearest_host,
    );
    (burn, returning, verdict)
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
pub struct AttritionTickInput {
    pub frame: i32,
    pub unit_id: i16,
    pub period: i16,
    /// Every world query consumed by `Unit::process_supply`, already resolved by the host.
    pub supply: SupplyInput,
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
    if borders_fog::supply_state(&input.supply) {
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
            air_fuel_transition(&ty, 2, true, false, 0, 0, false, None, None);
        assert_eq!(burn, 3);
        assert_eq!(returning, 1);
        assert_eq!(verdict, FuelVerdict::Crash);
    }

    #[test]
    fn attrition_adapter_distinguishes_timing_supply_and_damage() {
        let mut input = AttritionTickInput {
            frame: 47,
            unit_id: 0,
            period: 48,
            supply: SupplyInput {
                near_supply_source: true,
                ..SupplyInput::default()
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
