//! Exact shipped gather-enhancer identities and base-level city arithmetic.
//!
//! This module owns no terrain yield and no construction lifecycle.  It validates the
//! five live source rows needed by the bounded Arena tranche, delegates the shipped
//! `20 / 20 / 50` tables to `tech_cities::calc_gather_enhancers`, and preserves
//! `CityData::enhancer_amount`'s multiply-then-divide ordering.  The caller decides which
//! completed buildings belong to a city and which admitted gather source is being scaled.

use super::types::TypeRow;
use don_sim::systems::economy::{NUM_RESOURCES, RES_FOOD, RES_METAL, RES_TIMBER};
use don_sim::systems::tech_cities::{
    calc_gather_enhancers, city_taxes, CityRules, GatherEnhancers,
};

pub const GRANARY_TYPE: i32 = 423;
pub const LUMBER_MILL_TYPE: i32 = 424;
pub const SMELTER_TYPE: i32 = 425;
pub const MATHEMATICS_TYPE: i32 = 552;
pub const CHEMISTRY_TYPE: i32 = 553;

const CLASSICAL_AGE_TYPE: i32 = 544;
const LIBRARY_TYPE: i32 = 435;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherUpgradeSources {
    pub granary: i32,
    pub lumber_mill: i32,
    pub smelter: i32,
    pub mathematics: i32,
    pub chemistry: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GatherUpgradeSourceError {
    Missing(i32),
    Mismatch { type_id: i32, field: &'static str },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompletedBaseEnhancers {
    pub granary: bool,
    pub lumber_mill: bool,
    pub smelter: bool,
}

/// Validate the shipped live rows before Arena can name or apply this tranche.
///
/// The closure shape lets an observation-only policy validate through `Obs::ty` without
/// acquiring a second, richer rules view.  A renamed, renumbered, re-costed or re-gated
/// row fails closed rather than silently becoming a different policy.
pub fn validate_shipped_sources<'a>(
    mut row: impl FnMut(i32) -> Option<&'a TypeRow>,
) -> Result<GatherUpgradeSources, GatherUpgradeSourceError> {
    validate_building(
        row(GRANARY_TYPE).ok_or(GatherUpgradeSourceError::Missing(GRANARY_TYPE))?,
        "Granary",
        "GRANARY",
        [0, 60, 40, 0, 0, 0],
        &[MATHEMATICS_TYPE, CLASSICAL_AGE_TYPE],
        1,
        16_252_927,
    )?;
    validate_building(
        row(LUMBER_MILL_TYPE).ok_or(GatherUpgradeSourceError::Missing(LUMBER_MILL_TYPE))?,
        "Lumber Mill",
        "LUMBERMILL",
        [60, 0, 0, 0, 40, 0],
        &[MATHEMATICS_TYPE, CLASSICAL_AGE_TYPE],
        1,
        16_777_215,
    )?;
    validate_building(
        row(SMELTER_TYPE).ok_or(GatherUpgradeSourceError::Missing(SMELTER_TYPE))?,
        "Smelter",
        "SMELTER",
        [0, 70, 50, 0, 0, 0],
        &[CHEMISTRY_TYPE, CLASSICAL_AGE_TYPE],
        2,
        16_777_215,
    )?;
    validate_tech(
        row(MATHEMATICS_TYPE).ok_or(GatherUpgradeSourceError::Missing(MATHEMATICS_TYPE))?,
        MATHEMATICS_TYPE,
        "Mathematics",
        275,
        [0, 0, 120, 80, 0, 0],
        &[551],
        1,
    )?;
    validate_tech(
        row(CHEMISTRY_TYPE).ok_or(GatherUpgradeSourceError::Missing(CHEMISTRY_TYPE))?,
        CHEMISTRY_TYPE,
        "Chemistry",
        350,
        [0, 0, 200, 160, 0, 0],
        &[MATHEMATICS_TYPE],
        2,
    )?;

    Ok(GatherUpgradeSources {
        granary: GRANARY_TYPE,
        lumber_mill: LUMBER_MILL_TYPE,
        smelter: SMELTER_TYPE,
        mathematics: MATHEMATICS_TYPE,
        chemistry: CHEMISTRY_TYPE,
    })
}

fn validate_building(
    row: &TypeRow,
    name: &str,
    internal: &str,
    cost: [i32; NUM_RESOURCES],
    preq: &[i32],
    age: i32,
    tribe_mask: u32,
) -> Result<(), GatherUpgradeSourceError> {
    let expected_id = match name {
        "Granary" => GRANARY_TYPE,
        "Lumber Mill" => LUMBER_MILL_TYPE,
        "Smelter" => SMELTER_TYPE,
        _ => unreachable!("validator only admits the shipped base enhancers"),
    };
    check(row.id == expected_id, row.id, "type_id")?;
    check(row.kind_building && !row.kind_unit, row.id, "kind")?;
    check(row.name == name, row.id, "name_display")?;
    check(row.internal == internal, row.id, "name_internal")?;
    check(row.job_time == 1000, row.id, "job_time")?;
    check(row.cost == cost, row.id, "cost")?;
    check(row.preq == preq, row.id, "preq")?;
    check(row.where_ == -1, row.id, "where")?;
    check(row.from == -1, row.id, "from")?;
    check(row.age == age, row.id, "age")?;
    check(row.tribe_mask == tribe_mask, row.id, "tribe_mask")?;
    check(row.build_flags == 0x0800_0201, row.id, "build_flags")?;
    check(row.x_size == 5 && row.y_size == 5, row.id, "footprint")?;
    Ok(())
}

fn validate_tech(
    row: &TypeRow,
    expected_id: i32,
    name: &str,
    job_time: i32,
    cost: [i32; NUM_RESOURCES],
    preq: &[i32],
    age: i32,
) -> Result<(), GatherUpgradeSourceError> {
    check(row.id == expected_id, row.id, "type_id")?;
    check(!row.kind_building && !row.kind_unit, row.id, "kind")?;
    check(row.name == name, row.id, "name_display")?;
    check(row.job_time == job_time, row.id, "job_time")?;
    check(row.cost == cost, row.id, "cost")?;
    check(row.preq == preq, row.id, "preq")?;
    check(row.where_ == LIBRARY_TYPE, row.id, "where")?;
    check(row.from == -1, row.id, "from")?;
    check(row.age == age, row.id, "age")?;
    check(row.tribe_mask == 16_777_215, row.id, "tribe_mask")?;
    Ok(())
}

fn check(
    condition: bool,
    type_id: i32,
    field: &'static str,
) -> Result<(), GatherUpgradeSourceError> {
    condition
        .then_some(())
        .ok_or(GatherUpgradeSourceError::Mismatch { type_id, field })
}

/// The exact base-level bytes written by `City::calc_gather` for completed enhancers.
///
/// Higher levels require the runtime BonusType/property queries used by
/// `LeaderData::get_granary`, `CityData::lumber_level` and `LeaderData::get_smelter`.
/// Arena does not expose those properties, so this bounded tranche supplies only level 1.
pub fn base_enhancers(completed: CompletedBaseEnhancers) -> GatherEnhancers {
    calc_gather_enhancers(
        &CityRules::RETAIL,
        usize::from(completed.granary),
        usize::from(completed.lumber_mill),
        usize::from(completed.smelter),
    )
}

/// Exact `CityData::enhancer_amount(resource, amount)` arithmetic for the supported slots.
pub fn enhancer_amount(completed: CompletedBaseEnhancers, resource: usize, amount: i32) -> i32 {
    let enhancers = base_enhancers(completed);
    let bonus = match resource {
        RES_FOOD => enhancers.granary,
        RES_TIMBER => enhancers.lumber_mill,
        RES_METAL => enhancers.smelter,
        _ => 0,
    };
    i32::from(bonus).wrapping_add(100).wrapping_mul(amount) / 100
}

/// Exact city percentage passed to `BuildTypeData::calc_gather` for one resource.
#[inline]
pub fn enhancer_percent(completed: CompletedBaseEnhancers, resource: usize) -> i32 {
    enhancer_amount(completed, resource, 100)
}

/// Exact sixteenths-scale wealth gross from one completed city census.
///
/// The caller owns the live same-city census. Porcelain Tower is deliberately false: its
/// Leader/property gate is outside this base tranche. Passing Temple preserves the exact
/// recovered `CityData::get_taxes` call even though shipped `TEMPLE_TAXES` is zero.
pub fn base_city_tax_gross(completed_buildings: i32, has_market: bool, has_temple: bool) -> i32 {
    city_taxes(
        &CityRules::RETAIL,
        completed_buildings,
        has_market,
        false,
        0,
        has_temple,
    )
    .wrapping_mul(16)
}
