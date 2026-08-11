//! Exact shipped gather-enhancer identities and Roman city arithmetic.
//!
//! This module owns no terrain yield and no construction lifecycle.  It validates the
//! fourteen live source rows and the `rules.xml` BonusType prerequisites needed by the
//! bounded Arena tranche, delegates the shipped level tables to
//! `tech_cities::calc_gather_enhancers`, and preserves `CityData::enhancer_amount`'s
//! multiply-then-divide ordering.  The caller decides which completed buildings belong to
//! a city and which admitted gather source is being scaled.

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
pub const CARPENTRY_TYPE: i32 = 609;
pub const LOGGING_INDUSTRY_TYPE: i32 = 610;
pub const PAPERMILL_TYPE: i32 = 611;
pub const AGRICULTURE_TYPE: i32 = 612;
pub const CROP_ROTATION_TYPE: i32 = 613;
pub const FOOD_INDUSTRY_TYPE: i32 = 614;
pub const METAL_ALLOYS_TYPE: i32 = 620;
pub const COLD_CASTING_TYPE: i32 = 621;
pub const STEEL_TYPE: i32 = 622;

pub const GATHER_RESEARCH_TYPES: [i32; 9] = [
    CARPENTRY_TYPE,
    LOGGING_INDUSTRY_TYPE,
    PAPERMILL_TYPE,
    AGRICULTURE_TYPE,
    CROP_ROTATION_TYPE,
    FOOD_INDUSTRY_TYPE,
    METAL_ALLOYS_TYPE,
    COLD_CASTING_TYPE,
    STEEL_TYPE,
];

const CLASSICAL_AGE_TYPE: i32 = 544;
const LIBRARY_TYPE: i32 = 435;
const LAWS_OF_NATURE_TYPE: i32 = 554;
const ELECTRICITY_TYPE: i32 = 555;
const ELECTRONICS_TYPE: i32 = 556;
const FIRST_BONUS_TYPE: usize = 684;

/// The deterministic opening trace uses the shipped Roman roster. Greek research
/// cost/speed and Egyptian/French early enhancer properties are not hosted by Arena.
pub const AUTHORITATIVE_RESEARCH_TRIBE: u8 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherUpgradeSources {
    pub granary: i32,
    pub lumber_mill: i32,
    pub smelter: i32,
    pub mathematics: i32,
    pub chemistry: i32,
    pub research: [i32; 9],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GatherUpgradeSourceError {
    Missing(i32),
    Mismatch { type_id: i32, field: &'static str },
    MissingRulesSection(&'static str),
    RulesMismatch(&'static str),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompletedBaseEnhancers {
    pub granary: bool,
    pub lumber_mill: bool,
    pub smelter: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GatherEnhancerLevels {
    pub granary: usize,
    pub lumber_mill: usize,
    pub smelter: usize,
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
        LIBRARY_TYPE,
    )?;
    validate_tech(
        row(CHEMISTRY_TYPE).ok_or(GatherUpgradeSourceError::Missing(CHEMISTRY_TYPE))?,
        CHEMISTRY_TYPE,
        "Chemistry",
        350,
        [0, 0, 200, 160, 0, 0],
        &[MATHEMATICS_TYPE],
        2,
        LIBRARY_TYPE,
    )?;

    for (type_id, name, job_time, cost, preq, age, where_) in [
        (
            CARPENTRY_TYPE,
            "Carpentry",
            300,
            [150, 0, 0, 0, 150, 0],
            &[CHEMISTRY_TYPE][..],
            2,
            LUMBER_MILL_TYPE,
        ),
        (
            LOGGING_INDUSTRY_TYPE,
            "Logging Industry",
            350,
            [250, 0, 0, 0, 250, 0],
            &[LAWS_OF_NATURE_TYPE, CARPENTRY_TYPE][..],
            3,
            LUMBER_MILL_TYPE,
        ),
        (
            PAPERMILL_TYPE,
            "Papermill",
            450,
            [450, 0, 0, 0, 450, 0],
            &[ELECTRONICS_TYPE, LOGGING_INDUSTRY_TYPE][..],
            5,
            LUMBER_MILL_TYPE,
        ),
        (
            AGRICULTURE_TYPE,
            "Agriculture",
            300,
            [0, 150, 0, 0, 150, 0],
            &[CHEMISTRY_TYPE][..],
            2,
            GRANARY_TYPE,
        ),
        (
            CROP_ROTATION_TYPE,
            "Crop Rotation",
            350,
            [0, 250, 0, 0, 250, 0],
            &[LAWS_OF_NATURE_TYPE, AGRICULTURE_TYPE][..],
            3,
            GRANARY_TYPE,
        ),
        (
            FOOD_INDUSTRY_TYPE,
            "Food Industry",
            450,
            [0, 450, 0, 0, 450, 0],
            &[ELECTRONICS_TYPE, CROP_ROTATION_TYPE][..],
            5,
            GRANARY_TYPE,
        ),
        (
            METAL_ALLOYS_TYPE,
            "Metal Alloys",
            350,
            [250, 250, 0, 0, 0, 0],
            &[LAWS_OF_NATURE_TYPE][..],
            3,
            SMELTER_TYPE,
        ),
        (
            COLD_CASTING_TYPE,
            "Cold Casting",
            400,
            [350, 350, 0, 0, 0, 0],
            &[ELECTRICITY_TYPE, METAL_ALLOYS_TYPE][..],
            4,
            SMELTER_TYPE,
        ),
        (
            STEEL_TYPE,
            "Steel",
            450,
            [450, 450, 0, 0, 0, 0],
            &[ELECTRONICS_TYPE, COLD_CASTING_TYPE][..],
            5,
            SMELTER_TYPE,
        ),
    ] {
        validate_tech(
            row(type_id).ok_or(GatherUpgradeSourceError::Missing(type_id))?,
            type_id,
            name,
            job_time,
            cost,
            preq,
            age,
            where_,
        )?;
    }

    Ok(GatherUpgradeSources {
        granary: GRANARY_TYPE,
        lumber_mill: LUMBER_MILL_TYPE,
        smelter: SMELTER_TYPE,
        mathematics: MATHEMATICS_TYPE,
        chemistry: CHEMISTRY_TYPE,
        research: GATHER_RESEARCH_TYPES,
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
    where_: i32,
) -> Result<(), GatherUpgradeSourceError> {
    check(row.id == expected_id, row.id, "type_id")?;
    check(!row.kind_building && !row.kind_unit, row.id, "kind")?;
    check(row.name == name, row.id, "name_display")?;
    check(row.job_time == job_time, row.id, "job_time")?;
    check(row.cost == cost, row.id, "cost")?;
    check(row.preq == preq, row.id, "preq")?;
    check(row.where_ == where_, row.id, "where")?;
    check(row.from == -1, row.id, "from")?;
    check(row.age == age, row.id, "age")?;
    check(row.tribe_mask == 16_777_215, row.id, "tribe_mask")?;
    Ok(())
}

/// Validate the shipped BonusType-to-TechType query mapping and the nation-power
/// descriptors that bound this Roman-only adapter.
///
/// `LeaderData::get_granary`, `CityData::lumber_level`, and
/// `LeaderData::get_smelter` query BonusTypes 699..711. Their `rules.xml` ordering is the
/// only shipped data link from those properties to the nine TechType rows above.
pub fn validate_shipped_rule_text(xml: &str) -> Result<(), GatherUpgradeSourceError> {
    let bonuses = xml
        .split_once("<TECHBONUSES>")
        .and_then(|(_, rest)| rest.split_once("</TECHBONUSES>").map(|(body, _)| body))
        .ok_or(GatherUpgradeSourceError::MissingRulesSection("TECHBONUSES"))?;
    let preqs: Vec<&str> = bonuses
        .split("<BONUS>")
        .skip(1)
        .map(|block| {
            block
                .split_once("preq0=\"")
                .and_then(|(_, rest)| rest.split_once('"').map(|(value, _)| value))
                .unwrap_or("")
        })
        .collect();
    for (bonus_type, expected) in [
        (699, "Agriculture"),
        (700, "Crop Rotation"),
        (701, "Food Industry"),
        (702, "disable"),
        (706, "Carpentry"),
        (707, "Logging Industry"),
        (708, "Papermill"),
        (709, "Metal Alloys"),
        (710, "Cold Casting"),
        (711, "Steel"),
    ] {
        if preqs.get(bonus_type - FIRST_BONUS_TYPE).copied() != Some(expected) {
            return Err(GatherUpgradeSourceError::RulesMismatch(
                "BonusType prerequisite",
            ));
        }
    }

    for (tag, expected) in [
        ("GREEK_RESEARCH_SPEED", "100% bonus"),
        ("GREEK_RESEARCH_COST", "10% cost reduction"),
        (
            "EGYPTIAN_GRANARY_EARLY",
            "2 (2 = enabled and start with one, 1 = enabled, 0 = disabled)",
        ),
        ("EGYPTIAN_GRANARY_UPGRADES", "1 (1 = enabled, 0 = disabled)"),
        ("FRENCH_LUMBERMILL_EARLY", "2"),
        ("FRENCH_LUMBERMILL_UPGRADES", "1"),
    ] {
        let needle = format!("<{tag} value=\"");
        let actual = xml
            .split_once(&needle)
            .and_then(|(_, rest)| rest.split_once('"').map(|(value, _)| value));
        if actual != Some(expected) {
            return Err(GatherUpgradeSourceError::RulesMismatch(
                "nation-power descriptor",
            ));
        }
    }
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

/// Exact completed-building levels for the default Roman Arena cohort.
///
/// Returning `None` is deliberate for every other tribe: Arena has no Greek research
/// discount/speed host and no Egyptian/French early-building property/grant host, so this
/// adapter does not silently generalize a Roman query result to nation-power games.
pub fn roman_enhancer_levels(
    tribe: u8,
    completed: CompletedBaseEnhancers,
    has_tech: impl Fn(i32) -> bool,
) -> Option<GatherEnhancerLevels> {
    if tribe != AUTHORITATIVE_RESEARCH_TRIBE {
        return None;
    }
    Some(GatherEnhancerLevels {
        granary: if !completed.granary {
            0
        } else if has_tech(FOOD_INDUSTRY_TYPE) {
            4
        } else if has_tech(CROP_ROTATION_TYPE) {
            3
        } else if has_tech(AGRICULTURE_TYPE) {
            2
        } else {
            1
        },
        lumber_mill: if !completed.lumber_mill {
            0
        } else if has_tech(PAPERMILL_TYPE) {
            4
        } else if has_tech(LOGGING_INDUSTRY_TYPE) {
            3
        } else if has_tech(CARPENTRY_TYPE) {
            2
        } else {
            1
        },
        smelter: if !completed.smelter {
            0
        } else if has_tech(STEEL_TYPE) {
            4
        } else if has_tech(COLD_CASTING_TYPE) {
            3
        } else if has_tech(METAL_ALLOYS_TYPE) {
            2
        } else {
            1
        },
    })
}

pub fn researched_enhancers(levels: GatherEnhancerLevels) -> GatherEnhancers {
    calc_gather_enhancers(
        &CityRules::RETAIL,
        levels.granary,
        levels.lumber_mill,
        levels.smelter,
    )
}

/// Exact `CityData::enhancer_amount` arithmetic at a resolved shipped research level.
pub fn researched_enhancer_amount(
    levels: GatherEnhancerLevels,
    resource: usize,
    amount: i32,
) -> i32 {
    let enhancers = researched_enhancers(levels);
    let bonus = match resource {
        RES_FOOD => enhancers.granary,
        RES_TIMBER => enhancers.lumber_mill,
        RES_METAL => enhancers.smelter,
        _ => 0,
    };
    i32::from(bonus).wrapping_add(100).wrapping_mul(amount) / 100
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
