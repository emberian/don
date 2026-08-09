//! Tech tree and cities — the `cities` checksum channel and the tech/age rule layer.
//!
//! Everything here is derived from `ron-bin/riseofnations.exe` + `ron-bin/sbl/rise.pdb` and
//! the shipped `ron-data/*.xml`. Each item carries its source address. The derivation notes
//! live in `docs/mechanics/tech-cities.md`.
//!
//! # Fidelity discipline
//!
//! * `[measured]` — read out of this binary / these data files in this session.
//! * `[structure]` — control flow taken from `re/decomp-all/<ea>.c` (Ghidra output is a
//!   hypothesis about *structure*; integer values in it are trustworthy, ordering of
//!   int-vs-float ops is not — none of this subsystem is float except `plunder_scale`).
//! * `[unverified]` — reproduced from structure but not yet differentially tested.
//!
//! Nothing in this file has been run against the oracle. Nothing here is Tier A or B.
//!
//! # What this module owns
//!
//! `CheckSums::check_cities` (`0x00937600`) — channel 9 of the 15 in `CheckSums::check_all`
//! (`0x00936560`). The state it walks is the `Cities` pool: a **160-slot** pool, 8 players
//! x 20 `City` records, `Cities` at `0x00C09960`, `sizeof(City) == 192`.

#![allow(clippy::needless_range_loop)]

// ---------------------------------------------------------------------------------------
// 0. Type indices used by the city/tech code.
// ---------------------------------------------------------------------------------------

/// `enum TypeIndex` values this subsystem branches on. [measured, PDB TPI `TypeIndex`]
pub mod ty {
    pub const PEASANTS: i32 = 50;
    pub const PEASANTS_KOREAN: i32 = 51;

    /// First build type; also `VILLAGE`, the level-1 city center.
    pub const VILLAGE: i32 = 414; // 0x19e
    pub const TOWN: i32 = 415; // 0x19f  ("Large City")
    pub const METROPOLIS: i32 = 416; // 0x1a0 ("Major City")
    pub const FARM: i32 = 417; // 0x1a1
    pub const WOODCUTTER: i32 = 418; // 0x1a2
    pub const MINE: i32 = 419; // 0x1a3
    pub const UNIVERSITY: i32 = 420; // 0x1a4
    pub const OIL_WELL: i32 = 421; // 0x1a5
    pub const OIL_PLATFORM: i32 = 422; // 0x1a6
    pub const SMELTER_AGE0: i32 = 423; // 0x1a7
    pub const SMELTER_AGE1: i32 = 424; // 0x1a8
    pub const SMELTER_AGE4: i32 = 425; // 0x1a9
    pub const SMELTER_AGE5: i32 = 426; // 0x1aa
    pub const LIBRARY: i32 = 435; // 0x1b3
    pub const MARKET: i32 = 436; // 0x1b4
    pub const TEMPLE: i32 = 437; // 0x1b5
    pub const CAPITOL: i32 = 438; // 0x1b6  (the capital city center marker)

    pub const PYRAMIDS: i32 = 526; // 0x20e
    pub const FORBIDDEN_CITY: i32 = 531; // 0x213 — behaves as a level-3 city center

    /// `BASE_AGETYPES` / `CLASSICAL_AGE`.
    pub const CLASSICAL_AGE: i32 = 544; // 0x220
    pub const INFORMATION_AGE: i32 = 550; // 0x226
    /// `BASE_EPOCHTYPES` == `END_AGETYPES` == `WRITTEN_WORD`.
    pub const BASE_EPOCHTYPES: i32 = 551; // 0x227
    /// `END_EPOCHTYPES` (exclusive upper bound of the 28 library techs).
    pub const END_EPOCHTYPES: i32 = 579; // 0x243

    /// The four mutually-exclusive government pairs occupy `[0x26f, 0x274]`.
    pub const GOV_PAIR_BASE: i32 = 623; // 0x26f

    /// Highest tech-ish index the age/epoch revalidation sweeps reach (exclusive).
    pub const REVALIDATE_END: i32 = 629; // 0x275

    /// Number of bits in `LeaderData::tech : BitMask<806>`.
    pub const NUM_TYPES: usize = 806;
}

/// Research categories, as assigned by `TechType::set_research` `0x0066cba0`. [measured]
///
/// `cat = f((type - BASE_EPOCHTYPES) / 7)`: quotient 0 -> 3, 1 -> 2, 2 -> 1, 3 -> 0.
/// Anything that is not an epoch tech also lands on `SCIENCE` (the fallback store).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum ResearchCat {
    Military = 0,
    Civic = 1,
    Commerce = 2,
    Science = 3,
}

/// `TechType::set_research` `0x0066cba0`. [measured]
pub fn research_cat(type_index: i32) -> ResearchCat {
    if (ty::BASE_EPOCHTYPES..ty::END_EPOCHTYPES).contains(&type_index) {
        match (type_index - ty::BASE_EPOCHTYPES) / 7 {
            0 => ResearchCat::Science,
            1 => ResearchCat::Commerce,
            2 => ResearchCat::Civic,
            _ => ResearchCat::Military,
        }
    } else {
        ResearchCat::Science
    }
}

/// Resource slot order for `TypeData::costs[6]`, `LeaderData::econ[6]`,
/// `Constants::city_gather[6]`. [measured, `enum TypeIndex` 0..5 and
/// `ron-data/resourcerules.xml` `<ABBREV>`]
pub const RES_FOOD: usize = 0;
pub const RES_TIMBER: usize = 1;
pub const RES_WEALTH: usize = 2;
pub const RES_KNOWLEDGE: usize = 3;
pub const RES_METAL: usize = 4;
pub const RES_OIL: usize = 5;
pub const NUM_RES: usize = 6;

/// `<ABBREV>` letters for the six basic resources, in slot order.
/// [measured, `ron-data/resourcerules.xml`]
pub const RES_ABBREV: [char; NUM_RES] = ['f', 't', 'g', 'k', 'm', 'o'];

// ---------------------------------------------------------------------------------------
// 1. Rule constants — offsets into the `Constants` singleton, values from ron-data/rules.xml.
// ---------------------------------------------------------------------------------------

/// The subset of `Constants` this subsystem reads. Offsets are byte offsets into the
/// `Constants` object reached through `GameAccess::constants` `[0x00C061F0]`
/// (== `GameAccessConst::constantsc` `[0x00C061E4]`, the same object).
///
/// Values are the retail `ron-data/rules.xml` ones. [measured, offsets from
/// `docs/derivation/rules-constants.json`; values from the shipped rules.xml]
#[derive(Clone, Copy, Debug)]
pub struct CityRules {
    /// `+108` `RECAPTURE_CITY_MODIFIER`, parsed by `String::fraction(256)` -> `512`.
    pub recapture_city_modifier: i32,
    /// `+272` `CITY_TERRITORY_MULTIPLIER`.
    pub city_territory_multiplier: i32,
    /// `+288` `TERRITORY_LIMIT_CITY`, tiles.
    pub territory_limit_city: i32,
    /// `+300` `CITY_CENTER_RADIUS`, tiles.
    pub city_center_radius: i32,
    /// `+304` `CITY_CENTER_POP_RADIUS`, tiles — the *per level* radius increment.
    pub city_center_pop_radius: i32,
    /// `+308` `CITY_CAPTURE_RADIUS`, tiles.
    pub city_capture_radius: i32,
    /// `+312` `CITY_SPACING`, tiles.
    pub city_spacing: i32,
    /// `+316` `RELAX_CITY_SPACING`, tiles subtracted when the owner dominates the region.
    pub relax_city_spacing: i32,
    /// `+320` `FIRST_CITY_NEAR_COAST`, tiles.
    pub first_city_near_coast: i32,
    /// `+328` `FORT_TO_ENEMY_CITY_SPACING`, tiles.
    pub fort_to_enemy_city_spacing: i32,
    /// `+332` `CITY_BUILDINGS` — distinct kinds for VILLAGE -> TOWN is this **+ 1**.
    pub city_buildings: i32,
    /// `+336` `METRO_BUILDINGS` — distinct kinds for TOWN -> METROPOLIS is this **+ 1**.
    pub metro_buildings: i32,
    /// `+672` `FARMS_PER_CITY_BASE`.
    pub farms_per_city_base: i32,
    /// `+676` `FARMS_PER_CITY_LEVEL`.
    pub farms_per_city_level: i32,
    /// `+700..716` `GRANARY_BONUS[5]`, percent.
    pub granary_bonus: [i32; 5],
    /// `+720..736` `LUMBERMILL_BONUS[5]`, percent.
    pub lumbermill_bonus: [i32; 5],
    /// `+740..756` `SMELTER_BONUS[5]`, percent.
    pub smelter_bonus: [i32; 5],
    /// `+760` `REFINERY_BONUS`, percent.
    pub refinery_bonus: i32,
    /// `+772` `CITY_PLUNDER_PER_LEVEL`.
    pub city_plunder_per_level: i32,
    /// `+776` `CAPITAL_PLUNDER`.
    pub capital_plunder: i32,
    /// `+804` `VILLAGE_TAXES`.
    pub village_taxes: i32,
    /// `+808` `BUILDING_TAXES` (per building in the city).
    pub building_taxes: i32,
    /// `+812` `MARKET_TAXES`.
    pub market_taxes: i32,
    /// `+816` `TEMPLE_TAXES`.
    pub temple_taxes: i32,
    /// `+840` `VILLAGE_LITERACY`.
    pub village_literacy: i32,
    /// `+844` `UNIVERSITY_LITERACY`.
    pub university_literacy: i32,
    /// `+848` `LIBRARY_LITERACY`.
    pub library_literacy: i32,
    /// `+896` `DISBAND_CITY_RATE`, percent of normal raze time.
    pub disband_city_rate: i32,
    /// `+1000` `VILLAGE_POP` — pop cap granted per city, scaled by level.
    pub village_pop: i32,
    /// `+1068` `PYRAMIDS_CITY_LIMIT`.
    pub pyramids_city_limit: i32,
    /// `+1444` `BANTU_CITY_LIMIT`.
    pub bantu_city_limit: i32,
    /// `+1288` `TAJ_FARMS`.
    pub taj_farms: i32,
    /// `+1304` `KREMLIN_FARMS`.
    pub kremlin_farms: i32,
    /// `+1616` `EGYPTIAN_FARMS_PER_CITY_BASE`.
    pub egyptian_farms_per_city_base: i32,
    /// `+2408` `OLIVE_FARMS`.
    pub olive_farms: i32,
    /// `+2188` `INDIANS_CITY_RADIUS`.
    pub indians_city_radius: i32,
}

impl CityRules {
    /// Retail `ron-data/rules.xml`. [measured]
    pub const RETAIL: CityRules = CityRules {
        recapture_city_modifier: 512,
        city_territory_multiplier: 4,
        territory_limit_city: 4,
        city_center_radius: 20,
        city_center_pop_radius: 4,
        city_capture_radius: 10,
        city_spacing: 24,
        relax_city_spacing: 6,
        first_city_near_coast: 16,
        fort_to_enemy_city_spacing: 32,
        city_buildings: 5,
        metro_buildings: 9,
        farms_per_city_base: 5,
        farms_per_city_level: 0,
        granary_bonus: [20, 50, 100, 200, 250],
        lumbermill_bonus: [20, 50, 100, 200, 250],
        smelter_bonus: [50, 100, 150, 200, 250],
        refinery_bonus: 33,
        city_plunder_per_level: 100,
        capital_plunder: 500,
        village_taxes: 0,
        building_taxes: 0,
        market_taxes: 10,
        temple_taxes: 0,
        village_literacy: 0,
        university_literacy: 10,
        library_literacy: 0,
        disband_city_rate: 400,
        village_pop: 0,
        pyramids_city_limit: 1,
        bantu_city_limit: 1,
        taj_farms: 0,
        kremlin_farms: 0,
        egyptian_farms_per_city_base: 7,
        olive_farms: 1,
        indians_city_radius: 4,
    };
}

// ---------------------------------------------------------------------------------------
// 2. Primitives the checksum and the placement rule need, ported exactly.
// ---------------------------------------------------------------------------------------

/// `adler32` `0x00A46830` (`main/basic/misc.cpp:464`, BHG's own copy, not zlib's).
/// [measured]
///
/// `NMAX = 0x15B0 = 5552`, `BASE = 65521`, and a null buffer returns `1` rather than
/// leaving the accumulator alone — this module is the one that needs the null arm, so it
/// re-exports [`crate::checksum::adler32_or_null`] under the local name.
///
/// Correction: the header above says "BHG's own copy, not zlib's". Both exist —
/// `_adler32` `0x005089d0` is BHG's `main/basic` copy, `adler32` `0x00a46830` is the zlib
/// one, and it is `0x00a46830` that `CheckSum::walk_function` `0x00936ff0` calls
/// [measured, `call 0xa46830` at `0x0093700a`].
pub use crate::checksum::adler32_or_null as adler32;

/// `int vector_dist(int dx, int dy)` `0x0046CFF0` — the integer distance approximation
/// used by every proximity rule in the game, including city spacing. [measured]
///
/// `hi + lo*lo/(2*hi)`, with an overflow guard at 59999 that falls back to `hi + lo/2`
/// (written in the binary as `(lo + 2*hi) >> 1`).
pub fn vector_dist(dx: i32, dy: i32) -> u32 {
    let a = dx.unsigned_abs();
    let b = dy.unsigned_abs();
    let (lo, hi) = if b < a { (b, a) } else { (a, b) };
    if hi == 0 {
        return 0;
    }
    if lo > 59999 {
        return lo.wrapping_add(hi.wrapping_mul(2)) >> 1;
    }
    lo.wrapping_mul(lo) / hi.wrapping_mul(2) + hi
}

/// `CheckSum` `0x00B3F920`, the `DataWalk` implementation used by `CheckSums::check_all`.
///
/// `CheckSum::walk_function(void* begin, void* end)` `0x00936FF0` is exactly
/// `size += end - begin; accum = adler32(accum, begin, end - begin)`. [measured]
///
/// `check_all` builds the walker on its stack as
/// `{ vftable, input = 0, checksum = 1, flags = -1 }` and then resets `accum = 1;
/// size = 0` **before every channel**, so each channel is an independent adler32 seeded
/// at 1, and `check_all` returns the *sum* of the 15 channel accumulators. [measured]
#[derive(Clone, Copy, Debug)]
pub struct CheckSumWalk {
    pub accum: u32,
    pub size: u32,
    /// `DataWalk::checksum` — nonzero for the sync checksum. Gates out `String` members.
    pub checksum: i32,
}

impl Default for CheckSumWalk {
    fn default() -> Self {
        Self::new_channel()
    }
}

impl CheckSumWalk {
    /// The per-channel initial state used by `CheckSums::check_all`. [measured]
    pub fn new_channel() -> Self {
        CheckSumWalk {
            accum: 1,
            size: 0,
            checksum: 1,
        }
    }
    /// `CheckSum::walk_function` `0x00936FF0`.
    pub fn walk(&mut self, bytes: &[u8]) {
        self.size = self.size.wrapping_add(bytes.len() as u32);
        self.accum = adler32(self.accum, Some(bytes));
    }
}

// ---------------------------------------------------------------------------------------
// 3. City state — byte-exact for the fields the checksum walks.
// ---------------------------------------------------------------------------------------

/// City center level. `CityData::get_level` `0x00739340`. [measured]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CityLevel {
    /// `VILLAGE` (414) and anything unrecognised.
    Village = 1,
    /// `TOWN` (415).
    Town = 2,
    /// `METROPOLIS` (416) or `FORBIDDENCITY` (531).
    Metropolis = 3,
}

/// `CityData::get_level` `0x00739340`. [measured]
pub fn city_level(center_type: i32) -> CityLevel {
    if center_type == ty::TOWN {
        CityLevel::Town
    } else if center_type == ty::METROPOLIS || center_type == ty::FORBIDDEN_CITY {
        CityLevel::Metropolis
    } else {
        CityLevel::Village
    }
}

/// `CityData::upgrades_to` `0x007364F0`. `None` at level 3. [measured]
pub fn upgrades_to(center_type: i32) -> Option<i32> {
    if center_type == ty::METROPOLIS || center_type == ty::FORBIDDEN_CITY {
        None
    } else if center_type == ty::VILLAGE {
        Some(ty::TOWN)
    } else {
        Some(ty::METROPOLIS)
    }
}

/// `CityData::get_pop_value` `0x00738450` — the city's contribution to
/// `LeaderData::pop` / `Game::pop`. 1 / 3 / 5 by level. [measured]
pub fn pop_value(center_type: i32) -> i32 {
    match city_level(center_type) {
        CityLevel::Village => 1,
        CityLevel::Town => 3,
        CityLevel::Metropolis => 5,
    }
}

/// `CityData::pop_cap` `0x00737D40` — `VILLAGE_POP * level`. In retail rules
/// `VILLAGE_POP == 0`, so shipped cities grant **no** population cap. [measured]
pub fn pop_cap(rules: &CityRules, center_type: i32) -> i32 {
    rules.village_pop * (city_level(center_type) as i32)
}

/// `LeaderData::get_radius` `0x006DB790` (via `CityData::get_radius` `0x00738410`,
/// which re-applies the same clamp). [measured]
///
/// `city_center_radius + (level - 1) * city_center_pop_radius`, `+ INDIANS_CITY_RADIUS`
/// for tribe bonus 0x15, clamped to 64 tiles.
pub fn city_radius(rules: &CityRules, center_type: i32, indian_bonus: bool) -> i32 {
    let level = city_level(center_type) as i32;
    let mut r = (level - 1) * rules.city_center_pop_radius;
    if indian_bonus {
        r += rules.indians_city_radius;
    }
    r += rules.city_center_radius;
    r.min(64)
}

/// `LeaderData::get_farm_limit` `0x006DB270`, the per-leader base. [measured]
///
/// Egyptian (`has_tribe_bonus(7)`) swaps the base constant. `taj_farms` (+1288) and
/// `kremlin_farms` (+1304) are both 0 in retail; `olive_farms` (+2408) is 1 and is gated
/// on two rare-resource flags in `LeaderData`.
pub fn leader_farm_limit(
    rules: &CityRules,
    egyptian: bool,
    taj: bool,
    kremlin: bool,
    olive_oil: bool,
) -> i32 {
    let mut n = if egyptian {
        rules.egyptian_farms_per_city_base
    } else {
        rules.farms_per_city_base
    };
    // taj_farms / kremlin_farms are 0 in retail; kept so a modded ruleset still works.
    if taj {
        n += rules.taj_farms;
    }
    if kremlin {
        n += rules.kremlin_farms;
    }
    if olive_oil {
        n += rules.olive_farms;
    }
    n
}

/// `CityData::get_farm_limit` `0x00738230`. [measured]
pub fn city_farm_limit(rules: &CityRules, center_type: i32, leader_base: i32) -> i32 {
    let level = city_level(center_type) as i32;
    leader_base + (level - 1) * rules.farms_per_city_level
}

/// `CityData::num_kinds_needed` `0x00736640` — distinct building kinds required for the
/// next upgrade; 0 at level 3 or when the upgrade target is not researchable. [measured]
pub fn num_kinds_needed(rules: &CityRules, center_type: i32, target_available: bool) -> i32 {
    match upgrades_to(center_type) {
        None => 0,
        Some(target) => {
            if !target_available {
                0
            } else if target == ty::TOWN {
                rules.city_buildings + 1
            } else {
                rules.metro_buildings + 1
            }
        }
    }
}

/// `CityData::can_upgrade` `0x007383C0`. `distinct_kinds` is `CityData::num_kinds`
/// `0x007366C0` — the count of *distinct* `TypeIndex` among the city's buildings that are
/// active and pass the two virtual filters. [measured]
pub fn can_upgrade(rules: &CityRules, target_type: i32, distinct_kinds: i32) -> bool {
    if target_type == ty::VILLAGE {
        return true;
    }
    let need = if target_type == ty::TOWN {
        rules.city_buildings + 1
    } else {
        rules.metro_buildings + 1
    };
    // `CityData::enough_kinds(max)` caps its scan at 24 and returns 1 once it reaches
    // `min(max, 24)`. [measured, 0x00736540]
    distinct_kinds >= need.min(24)
}

/// `CityData::ready_to_upgrade` `0x00736480`. [measured]
pub fn ready_to_upgrade(
    rules: &CityRules,
    center_type: i32,
    target_available: bool,
    distinct_kinds: i32,
) -> bool {
    match upgrades_to(center_type) {
        None => false,
        Some(target) => target_available && can_upgrade(rules, target, distinct_kinds),
    }
}

/// `CityData::get_taxes` `0x00737B50`. [measured]
pub fn city_taxes(
    rules: &CityRules,
    num_buildings: i32,
    has_market: bool,
    porcelain_tower: bool,
    porcelain_market_pct: i32,
    has_temple: bool,
) -> i32 {
    let mut v = rules.village_taxes + num_buildings * rules.building_taxes;
    if has_market {
        let mut m = rules.market_taxes;
        if porcelain_tower {
            m = ((porcelain_market_pct + 100) * m) / 100;
        }
        v += m;
    }
    if has_temple {
        v += rules.temple_taxes;
    }
    v
}

/// `CityData::get_literacy` `0x00737C00`. [measured]
pub fn city_literacy(rules: &CityRules, has_university: bool, has_library: bool) -> i32 {
    let mut v = rules.village_literacy;
    if has_university {
        v += rules.university_literacy;
    }
    if has_library {
        v += rules.library_literacy;
    }
    v
}

/// `CityData::get_trade_value` `0x007363F0` — `num_buildings + {0, 2, 4}` by level.
/// [measured]
pub fn city_trade_value(center_type: i32, num_buildings: i32) -> i32 {
    match city_level(center_type) {
        CityLevel::Village => num_buildings,
        CityLevel::Town => num_buildings + 2,
        CityLevel::Metropolis => num_buildings + 4,
    }
}

/// `CityData::lumber_level` `0x00736820` — 0 without a lumber mill, else 1..=4 from the
/// upgrade ladder at type indices 0x2C2 / 0x2C3 / 0x2C4. [measured]
pub fn lumber_level(has_lumber_mill: bool, up1: bool, up2: bool, up3: bool) -> usize {
    if !has_lumber_mill {
        return 0;
    }
    if up3 {
        4
    } else if up2 {
        3
    } else if up1 {
        2
    } else {
        1
    }
}

/// The enhancer bonus tables are indexed **`level - 1`** — the engine writes the base
/// address one slot short (`constants + 0x2B8 + level*4` where `granary_bonus` starts at
/// `+0x2BC = 700`) so level 0 would read the neighbouring constant and is instead
/// short-circuited to 0 by the `city_flags` test. [measured]
///
/// `City::calc_gather` `0x00737C60` fills `CityData::{granary, lumber_mill, smelter,
/// refinery}` from these; **`refinery` is stored as a literal 0** in this build.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GatherEnhancers {
    /// `CityData::granary` `+86`.
    pub granary: u8,
    /// `CityData::lumber_mill` `+87`.
    pub lumber_mill: u8,
    /// `CityData::smelter` `+88`.
    pub smelter: u8,
    /// `CityData::refinery` `+89` — always 0 in this build.
    pub refinery: u8,
}

/// `City::calc_gather` `0x00737C60`, the part that fills the four enhancer bytes.
/// [measured]
///
/// `granary_level` / `lumber_level` / `smelter_level` are 1-based; 0 means "no such
/// building in this city", which stores a 0 bonus.
pub fn calc_gather_enhancers(
    rules: &CityRules,
    granary_level: usize,
    lumber_level: usize,
    smelter_level: usize,
) -> GatherEnhancers {
    let pick = |table: &[i32; 5], level: usize| -> u8 {
        if level == 0 || level > 5 {
            0
        } else {
            table[level - 1] as u8
        }
    };
    GatherEnhancers {
        granary: pick(&rules.granary_bonus, granary_level),
        lumber_mill: pick(&rules.lumbermill_bonus, lumber_level),
        smelter: pick(&rules.smelter_bonus, smelter_level),
        refinery: 0,
    }
}

/// `City::count_gather_slots` `0x00737DC0` — the auto-gather slot census. [measured]
///
/// Walks the city's building list (`BuildData::city_down` at `+116`) and maps each
/// gather-capable building type to a resource slot:
/// `FARM -> FOOD`, `WOODCUTTER -> TIMBER`, `MINE -> METAL`, `UNIVERSITY -> KNOWLEDGE`,
/// `OIL_WELL`/`OIL_PLATFORM -> OIL`. Slots come from `BuildData::gather_max` (`+128`).
///
/// The returned total **excludes** knowledge (`iVar7 == 3` skips the accumulate), which is
/// why universities do not count toward a city's gatherer total.
pub fn gather_slot_resource(build_type: i32) -> Option<usize> {
    match build_type {
        ty::FARM => Some(RES_FOOD),
        ty::WOODCUTTER => Some(RES_TIMBER),
        ty::MINE => Some(RES_METAL),
        ty::UNIVERSITY => Some(RES_KNOWLEDGE),
        ty::OIL_WELL | ty::OIL_PLATFORM => Some(RES_OIL),
        _ => None,
    }
}

/// One gather-capable building inside a city, for [`count_gather_slots`].
#[derive(Clone, Copy, Debug)]
pub struct CityGatherBuilding {
    /// `BuildData` type index.
    pub build_type: i32,
    /// `BuildData::gather_max` `+128`.
    pub gather_max: i32,
    /// `BuildData::num_gatherers()` `0x00630450` — currently assigned gatherers.
    pub num_gatherers: i32,
}

/// Result of `City::count_gather_slots(int* slots, int* free)` `0x00737DC0`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GatherSlots {
    pub slots: [i32; NUM_RES],
    pub free: [i32; NUM_RES],
    /// The function's return value: total slots **excluding** knowledge.
    pub total: i32,
}

/// `City::count_gather_slots` `0x00737DC0`. [structure] [unverified]
pub fn count_gather_slots(buildings: &[CityGatherBuilding]) -> GatherSlots {
    let mut out = GatherSlots::default();
    for b in buildings {
        let Some(res) = gather_slot_resource(b.build_type) else {
            continue;
        };
        out.slots[res] += b.gather_max;
        if res != RES_KNOWLEDGE {
            out.total += b.gather_max;
        }
        out.free[res] += b.gather_max - b.num_gatherers;
    }
    out
}

// ---------------------------------------------------------------------------------------
// 4. The `City` record, laid out so the checksum can be taken over its real bytes.
// ---------------------------------------------------------------------------------------

/// `CityData` `+4 .. +114`, the exact window `CheckSums::check_cities` walks.
///
/// Field names and offsets are the compiler's own, from the PDB TPI stream. `sizeof(City)
/// == 192`; `CityData` occupies `+0..+184` under a `vfptr` at `+0`. The checksum walks
/// `[+4, +6)` then `[+6, +114)` then `Array<CaravanLink> vans` (`+116`); the two `String`
/// members (`name` `+144`, `id` `+164`) are gated on `DataWalk::checksum == 0` and are
/// therefore **not** part of the sync checksum. [measured, `0x00937600` / `0x00489220`]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CityRecord {
    /// `+4` — bit 0 is "slot in use". Bits seen: 0x10 capital, 0x100 set on a cross-team
    /// capture (survives the `& 0xF13F` keep-mask, so only `City::assimilate` `0x00738E90`
    /// clears it), 0x200 has granary, 0x400 has lumber mill, 0x4000 / 0x8000 capital
    /// history.
    pub city_flags: u16,
    /// `+6` — index of this city in its owner's `PtrArray<City>`.
    pub city: i16,
    /// `+8` — object index of the city-center `Build`.
    pub o: i16,
    /// `+10` — region.
    pub reg: i16,
    /// `+12` `Coord x` (world units; 1 tile == 768).
    pub x: i32,
    /// `+16` `Coord y`.
    pub y: i32,
    /// `+20`
    pub attack_stamp: i32,
    /// `+24`
    pub raid_stamp: i32,
    /// `+28`
    pub reduce_stamp: i32,
    /// `+32`
    pub capture_stamp: i32,
    /// `+36`
    pub assimilation_timer: i32,
    /// `+40`
    pub capture_strength: i32,
    /// `+44` `int[8]`
    pub traded_with: [i32; 8],
    /// `+76`
    pub scouted: i16,
    /// `+78`
    pub in_port: i16,
    /// `+80`
    pub peasant_dist: i16,
    /// `+82`
    pub trade_val: i16,
    /// `+84`
    pub conquest_node: i16,
    /// `+86` granary gather bonus, percent
    pub granary: u8,
    /// `+87` lumber-mill gather bonus, percent
    pub lumber_mill: u8,
    /// `+88` smelter gather bonus, percent
    pub smelter: u8,
    /// `+89` refinery gather bonus — literal 0 in this build
    pub refinery: u8,
    /// `+90`
    pub free: u8,
    /// `+91`
    pub busy: u8,
    /// `+92`
    pub gatherers: u8,
    /// `+93`
    pub pop: u8,
    /// `+94` current owner
    pub who: i8,
    /// `+95` the nation the city counts as its own — drives the recapture damage bonus
    pub race: i8,
    /// `+96` the player who founded it
    pub founder: i8,
    /// `+97`
    pub plundered: u8,
    /// `+98`
    pub ocean: u8,
    /// `+99`
    pub land: u8,
    /// `+100`
    pub filled: u8,
    /// `+101`
    pub bordering: u8,
    /// `+102`
    pub ocean_filled: u8,
    /// `+103`
    pub dock_tile: u8,
    /// `+104` bitmask of players for whom this was once a capital
    pub was_capital_flags: u8,
    /// `+105` `unsigned char[3]`
    pub space: [u8; 3],
    /// `+108` `unsigned char[6]`
    pub ter: [u8; 6],
    // --- past the checksum window ---
    /// `+116` `Array<CaravanLink>` — walked by the checksum after the POD window.
    pub vans: CaravanLinkArray,
    /// `+144` `String name` — **not** in the sync checksum.
    pub name: String,
    /// `+164` `String id` — **not** in the sync checksum.
    pub id: String,
}

/// `CaravanLink` (`{ int cara; int who; }`, 8 bytes). [measured, PDB]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CaravanLink {
    pub cara: i32,
    pub who: i32,
}

/// `Array<CaravanLink>` — the checksum walks its **capacity and growth hint too**, not
/// just its contents, so a reimplementation must track them to match bit for bit.
/// [measured, `Array<CaravanLink>::walk_data` `0x00489040`]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CaravanLinkArray {
    /// `+4 length`
    pub items: Vec<CaravanLink>,
    /// `+8 size` — allocated capacity, **checksummed**.
    pub capacity: i32,
    /// `+12 grow` (`short`), **checksummed** as 2 bytes.
    pub grow: i16,
    /// `+20 flags`, masked with `0xBF` before walking, **checksummed** as 1 byte.
    pub flags: u8,
}

impl CaravanLinkArray {
    /// `Array<CaravanLink>::walk_data` `0x00489040`, checksum direction. [measured]
    ///
    /// Emits `length:i32`; if length != 0 also `capacity:i32`, `grow:i16`,
    /// `flags & 0xBF : u8`, then 8 bytes per element.
    pub fn walk(&self, w: &mut CheckSumWalk) {
        let len = self.items.len() as i32;
        w.walk(&len.to_le_bytes());
        if len == 0 {
            return;
        }
        w.walk(&self.capacity.to_le_bytes());
        w.walk(&self.grow.to_le_bytes());
        w.walk(&[self.flags & 0xBF]);
        for it in &self.items {
            let mut b = [0u8; 8];
            b[0..4].copy_from_slice(&it.cara.to_le_bytes());
            b[4..8].copy_from_slice(&it.who.to_le_bytes());
            w.walk(&b);
        }
    }
}

/// Byte length of the second `City` walk window, `[+6, +114)`.
pub const CITY_POD_LEN: usize = 108;

impl Default for CityRecord {
    fn default() -> Self {
        CityRecord {
            city_flags: 0,
            city: 0,
            o: -1,
            reg: -1,
            x: 0,
            y: 0,
            attack_stamp: 0,
            raid_stamp: 0,
            reduce_stamp: 0,
            capture_stamp: 0,
            assimilation_timer: 0,
            capture_strength: 0,
            traded_with: [0; 8],
            scouted: 0,
            in_port: 0,
            peasant_dist: 0,
            trade_val: 0,
            conquest_node: 0,
            granary: 0,
            lumber_mill: 0,
            smelter: 0,
            refinery: 0,
            free: 0,
            busy: 0,
            gatherers: 0,
            pop: 0,
            who: -1,
            race: -1,
            founder: -1,
            plundered: 0,
            ocean: 0,
            land: 0,
            filled: 0,
            bordering: 0,
            ocean_filled: 0,
            dock_tile: 0,
            was_capital_flags: 0,
            space: [0; 3],
            ter: [0; 6],
            vans: CaravanLinkArray::default(),
            name: String::new(),
            id: String::new(),
        }
    }
}

impl CityRecord {
    /// Slot in use — `city_flags & 1`. Every walk and nearly every query gates on it.
    pub fn active(&self) -> bool {
        self.city_flags & 1 != 0
    }

    /// The `[+6, +114)` image, little-endian, in declaration order. 108 bytes.
    pub fn pod_bytes(&self) -> [u8; CITY_POD_LEN] {
        let mut b = [0u8; CITY_POD_LEN];
        // offsets below are relative to City+6
        macro_rules! put {
            ($off:expr, $bytes:expr) => {{
                let s: &[u8] = &$bytes;
                b[$off..$off + s.len()].copy_from_slice(s);
            }};
        }
        put!(0, self.city.to_le_bytes()); // +6
        put!(2, self.o.to_le_bytes()); // +8
        put!(4, self.reg.to_le_bytes()); // +10
        put!(6, self.x.to_le_bytes()); // +12
        put!(10, self.y.to_le_bytes()); // +16
        put!(14, self.attack_stamp.to_le_bytes()); // +20
        put!(18, self.raid_stamp.to_le_bytes()); // +24
        put!(22, self.reduce_stamp.to_le_bytes()); // +28
        put!(26, self.capture_stamp.to_le_bytes()); // +32
        put!(30, self.assimilation_timer.to_le_bytes()); // +36
        put!(34, self.capture_strength.to_le_bytes()); // +40
        for (i, v) in self.traded_with.iter().enumerate() {
            put!(38 + i * 4, v.to_le_bytes()); // +44..+76
        }
        put!(70, self.scouted.to_le_bytes()); // +76
        put!(72, self.in_port.to_le_bytes()); // +78
        put!(74, self.peasant_dist.to_le_bytes()); // +80
        put!(76, self.trade_val.to_le_bytes()); // +82
        put!(78, self.conquest_node.to_le_bytes()); // +84
        b[80] = self.granary; // +86
        b[81] = self.lumber_mill; // +87
        b[82] = self.smelter; // +88
        b[83] = self.refinery; // +89
        b[84] = self.free; // +90
        b[85] = self.busy; // +91
        b[86] = self.gatherers; // +92
        b[87] = self.pop; // +93
        b[88] = self.who as u8; // +94
        b[89] = self.race as u8; // +95
        b[90] = self.founder as u8; // +96
        b[91] = self.plundered; // +97
        b[92] = self.ocean; // +98
        b[93] = self.land; // +99
        b[94] = self.filled; // +100
        b[95] = self.bordering; // +101
        b[96] = self.ocean_filled; // +102
        b[97] = self.dock_tile; // +103
        b[98] = self.was_capital_flags; // +104
        b[99..102].copy_from_slice(&self.space); // +105..+108
        b[102..108].copy_from_slice(&self.ter); // +108..+114
        b
    }

    /// `City::walk_data` `0x00489220`, checksum direction. [measured]
    pub fn walk(&self, w: &mut CheckSumWalk) {
        w.walk(&self.city_flags.to_le_bytes()); // [+4, +6)
        if !self.active() {
            return;
        }
        w.walk(&self.pod_bytes()); // [+6, +114)
        if w.checksum == 0 {
            // String::walk_data x2 — save/load path only.
            // Not implemented here: names are not sync-critical.
        }
        self.vans.walk(w);
    }
}

// ---------------------------------------------------------------------------------------
// 5. The city pool — `Cities` `0x00C09960`, 8 x `PtrArray<City>` x 20.
// ---------------------------------------------------------------------------------------

/// Number of player slots in the `Cities` pool. [measured, `Cities::init` `0x007358C0`
/// walks `0x00C09960 .. 0x00C09A50` in 28-byte strides.]
pub const NUM_PLAYERS: usize = 8;
/// Initial cities reserved per player. [measured, the `0x14` in `Cities::init`.]
pub const CITIES_PER_PLAYER: usize = 20;
/// The pool the lane brief calls the "160-slot pool".
pub const CITY_POOL_SLOTS: usize = NUM_PLAYERS * CITIES_PER_PLAYER;

/// `Cities` (`0x00C09960`) plus the per-leader `city_mark` (`LeaderData +1032`) that
/// bounds slot reuse.
#[derive(Clone, Debug)]
pub struct CityPool {
    /// `PtrArray<City>` per player. Grows past 20 only if every slot is live.
    pub slots: Vec<Vec<CityRecord>>,
    /// `LeaderData::city_mark` `+1032` — high-water mark of *used* slots per player.
    pub city_mark: [i32; NUM_PLAYERS],
}

impl Default for CityPool {
    fn default() -> Self {
        Self::new()
    }
}

impl CityPool {
    /// `Cities::init` `0x007358C0` — 8 x 20 default-constructed `City`. [measured]
    pub fn new() -> Self {
        CityPool {
            slots: (0..NUM_PLAYERS)
                .map(|_| vec![CityRecord::default(); CITIES_PER_PLAYER])
                .collect(),
            city_mark: [0; NUM_PLAYERS],
        }
    }

    /// `Cities::init_city(who, x, y, o)` `0x007352C0` — slot selection only.
    /// [measured, structure]
    ///
    /// Scans `0..city_mark` for the first slot with `city_flags & 1 == 0`; failing that
    /// takes slot `city_mark` if the array is long enough, otherwise appends. Then
    /// `city_mark = max(city_mark, idx + 1)`. `City::init` (`0x00737050`) fills the record;
    /// this function returns the index it chose.
    pub fn alloc_slot(&mut self, who: usize) -> usize {
        let mark = self.city_mark[who] as usize;
        let mut idx = None;
        for i in 0..mark {
            if !self.slots[who][i].active() {
                idx = Some(i);
                break;
            }
        }
        let idx = match idx {
            Some(i) => i,
            None => {
                if mark < self.slots[who].len() {
                    mark
                } else {
                    self.slots[who].push(CityRecord::default());
                    self.slots[who].len() - 1
                }
            }
        };
        let next = (idx as i32) + 1;
        if next > self.city_mark[who] {
            self.city_mark[who] = next;
        }
        idx
    }

    /// `Cities::close_city` `0x00733310` — trims `city_mark` back over trailing dead
    /// slots. The engine's loop stops at the first live trailing slot. [measured]
    pub fn trim_mark(&mut self, who: usize) {
        while self.city_mark[who] > 0 {
            let last = (self.city_mark[who] - 1) as usize;
            if self.slots[who][last].active() {
                break;
            }
            self.city_mark[who] -= 1;
        }
    }

    /// Live cities of a player, in slot order.
    pub fn iter_live(&self, who: usize) -> impl Iterator<Item = &CityRecord> {
        self.slots[who].iter().filter(|c| c.active())
    }

    /// `LeaderData::get_total_cities` `0x006D6060` counts these.
    pub fn count(&self, who: usize) -> i32 {
        self.iter_live(who).count() as i32
    }
}

/// `CheckSums::check_cities` `0x00937600` — channel 9 of `CheckSums::check_all`.
/// [measured]
///
/// For each of the 8 leader slots with `LeaderData::leader_flags & 1` set, walk **every
/// entry of that player's `PtrArray<City>`** (the array `length`, not `city_mark`), and
/// for each entry whose `city_flags & 1` is set, run `City::walk_data`.
///
/// Returns the channel accumulator; `check_all` adds it to the other 14.
pub fn check_cities(pool: &CityPool, leader_active: &[bool; NUM_PLAYERS]) -> u32 {
    let mut w = CheckSumWalk::new_channel();
    for who in 0..NUM_PLAYERS {
        if !leader_active[who] {
            continue;
        }
        for c in &pool.slots[who] {
            if c.active() {
                c.walk(&mut w);
            }
        }
    }
    w.accum
}

// ---------------------------------------------------------------------------------------
// 6. Capture, recapture, plunder.
// ---------------------------------------------------------------------------------------

/// The condition under which `City::capture` `0x00736C40` re-homes `race` to the
/// capturing player, rather than preserving the previous owner's. [measured]
#[derive(Clone, Copy, Debug, Default)]
pub struct CaptureContext {
    /// Capturing player is the same player the city already belonged to.
    pub same_player: bool,
    /// Mutual `diplos == 2` between capturer and previous owner (team/alliance).
    pub mutual_allied: bool,
    /// `LeaderData::leader_flags & 0x200000` on the capturer.
    pub leader_flag_200000: bool,
    /// `LeaderData::has_preq(0x2B9)` on the capturer.
    pub has_preq_2b9: bool,
}

impl CaptureContext {
    fn keeps_own_race(&self) -> bool {
        self.same_player || self.mutual_allied || self.leader_flag_200000 || self.has_preq_2b9
    }
}

/// `City::capture(City* old, short city, char who, short o)` `0x00736C40`, sim-state part.
/// [structure] [unverified]
///
/// The presentation, message and razing side effects are omitted; what is reproduced is
/// every field that lands inside the checksum window.
///
/// Returns the new record. Population bookkeeping (`LeaderData::pop` / `Game::pop` are
/// decremented by `pop_value` for the old center type and incremented for the new) is the
/// caller's job — see `pop_value`.
pub fn capture_city(
    old: &CityRecord,
    new_city_index: i16,
    new_who: i8,
    new_o: i16,
    new_x: i32,
    new_y: i32,
    game_frame: i32,
    ctx: CaptureContext,
) -> CityRecord {
    let mut c = old.clone();
    c.city = new_city_index;
    c.who = new_who;
    c.o = new_o;
    c.x = new_x;
    c.y = new_y;
    c.attack_stamp = game_frame;
    c.raid_stamp = game_frame;
    // pop, reg, scouted, founder, plundered, was_capital_flags, and the stamp block
    // +44..+76 all carry over unchanged — they are already in the clone.
    c.race = if ctx.keeps_own_race() {
        new_who
    } else {
        old.race
    };

    // city_flags = old & 0xF13F, then |= 0x100 unless the two players are on one team.
    let mut flags = old.city_flags & 0xF13F;
    if !(ctx.same_player || ctx.mutual_allied) {
        flags |= 0x100;
    }

    // capital handling: if the captured city was a capital, remember that for the old
    // owner and clear the capital bits.
    if flags & 0x10 != 0 {
        c.was_capital_flags |= 1u8 << ((old.who as u8) & 7);
        if flags & 0x4000 != 0 {
            flags |= 0x8000;
        }
    }
    flags &= 0xBFEF; // clear 0x4000 and 0x10
                     // if the new owner had a capital here before, restore it
    let bit = 1u8 << ((new_who as u8) & 7);
    if c.was_capital_flags & bit != 0 {
        flags |= 0x10;
        c.was_capital_flags &= !bit;
        if flags & 0x8000 != 0 && new_who == c.founder {
            flags = (flags & 0x3FFF) | 0x4010;
        }
        c.race = new_who;
    }
    c.city_flags = flags;
    c
}

/// Hit points the city-center building is left with after `City::capture`: `max_hits - 10`.
/// [measured, tail of `0x00736C40`]
pub fn hits_after_capture(max_hits: i32) -> i32 {
    max_hits - 10
}

/// The **recapture damage modifier**, applied at the very tail of
/// `ObjectData::get_damage` `0x00644130`. [measured]
///
/// If the target is a city-center building (`Object` flag bit 2 set and the build type
/// carries flag `0x20`), its `BuildData::city` index is valid, and
/// `cities[owner][city].race == attacker.who`, then the whole accumulated damage is
/// rescaled by `recapture_city_modifier / 256` — retail `512/256 == 2x`.
///
/// The rescale is the engine's exact expression
/// `(C*d + ((C*d) >> 31 & 0xFF)) >> 8`, i.e. truncation toward zero.
pub fn apply_recapture_modifier(rules: &CityRules, damage: i32) -> i32 {
    let p = rules.recapture_city_modifier.wrapping_mul(damage);
    (p.wrapping_add((p >> 31) & 0xFF)) >> 8
}

/// Whether the recapture modifier fires. [measured, `0x00644130` tail]
pub fn recapture_applies(
    target_is_city_center: bool,
    target_city_index: i16,
    city_race: i8,
    attacker_who: i8,
) -> bool {
    target_is_city_center && target_city_index >= 0 && city_race == attacker_who
}

/// `CITY_PLUNDER_PER_LEVEL` (`Constants +772`, retail 100) scaled by city level.
///
/// **[unverified]** — the constant is `[measured]` and named by the PDB, but no reader of
/// `Constants +0x304` appears in the bulk-decompiled corpus, so the multiplication site
/// has not been located. `Build::plunder` `0x00623660` (the general building plunder path)
/// uses `BuildTypeData::plunder_value` `+724` and `plunder_good` `+728` instead, scales by
/// `hits/max_hits` in **float** for non-flagged buildings, and applies
/// `LeaderData::plunder_scale` (`+2324`, `float`). Do not treat this helper as derived.
pub fn city_plunder(rules: &CityRules, level: CityLevel) -> i32 {
    rules.city_plunder_per_level * (level as i32)
}

// ---------------------------------------------------------------------------------------
// 7. City placement and minimum spacing.
// ---------------------------------------------------------------------------------------

/// A city already on the map, or planned, for the spacing test.
#[derive(Clone, Copy, Debug)]
pub struct SpacingCity {
    /// Owner slot.
    pub who: usize,
    /// Region index. The spacing test only compares cities in the **same region**.
    pub reg: i16,
    /// Tile coordinates.
    pub tx: i32,
    pub ty: i32,
}

/// `BuildTypeData::blocked_location` `0x006375B0`, the `is_city()` arm. [structure]
///
/// For each active leader `w`:
/// `spacing = CITY_SPACING`, and if `regions[reg].size * 9 / 10 <= leaders[w].reg_terr[reg]`
/// then `spacing -= RELAX_CITY_SPACING`. Any of `w`'s cities — built (`Cities[w]`, scanned
/// to `city_mark`) or planned (`UnbuiltCities::lists[w]`) — in the same region within
/// `vector_dist <= spacing` blocks the site, and the function returns **20**.
///
/// Returns the effective spacing for one candidate owner.
pub fn city_spacing_for(
    rules: &CityRules,
    region_tile_count: i32,
    owner_region_territory: i32,
) -> i32 {
    let mut spacing = rules.city_spacing;
    if (region_tile_count * 9) / 10 <= owner_region_territory {
        spacing -= rules.relax_city_spacing;
    }
    spacing
}

/// The blocked-reason code `BuildTypeData::blocked_location` returns when a city site is
/// too close to another city. [measured]
pub const BLOCKED_CITY_SPACING: i32 = 20;

/// `BuildTypeData::blocked_location` `0x006375B0`, city-spacing arm. [structure]
/// [unverified]
///
/// `spacing_for_owner(who)` supplies the per-owner relaxed spacing (see
/// [`city_spacing_for`]). Returns `Some(BLOCKED_CITY_SPACING)` if the site is rejected.
pub fn blocked_by_city_spacing(
    site_reg: i16,
    site_tx: i32,
    site_ty: i32,
    existing: &[SpacingCity],
    mut spacing_for_owner: impl FnMut(usize) -> i32,
) -> Option<i32> {
    for c in existing {
        if c.reg != site_reg {
            continue;
        }
        let spacing = spacing_for_owner(c.who);
        if vector_dist(c.tx - site_tx, c.ty - site_ty) <= spacing.max(0) as u32 {
            return Some(BLOCKED_CITY_SPACING);
        }
    }
    None
}

/// World units per tile. [measured — `Leader::produce_city` `0x006CB120` passes
/// `tile * 0x300` into the placement calls, and `+0x180` to reach a tile centre.]
pub const WORLD_UNITS_PER_TILE: i32 = 0x300;

// ---------------------------------------------------------------------------------------
// 8. Tech.
// ---------------------------------------------------------------------------------------

/// `LeaderData::tech : BitMask<806>` at `+27660`; payload 101 bytes at `+27672`
/// (`0x6C18`), bit index == `TypeIndex`. [measured, `LeaderData::LeaderData` `0x006D7540`
/// `memset(this+0x6C18, 0, 0x65)` and `LeaderData::walk_data` `0x006D6750`]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TechBitMask {
    pub bytes: [u8; 101],
}

impl Default for TechBitMask {
    fn default() -> Self {
        TechBitMask { bytes: [0; 101] }
    }
}

impl TechBitMask {
    /// `BitMask<806>::test` — the raw bit, no prerequisite logic. Mirrors
    /// `LeaderData::has_upgrade` `0x006E10E0`. [measured]
    pub fn get(&self, t: i32) -> bool {
        if t < 0 {
            return false;
        }
        let byte = (t >> 3) as usize;
        byte < self.bytes.len() && (self.bytes[byte] & (1 << (t & 7))) != 0
    }
    /// `BitMask<806>::set` `0x00450360`. [measured]
    pub fn set(&mut self, t: i32, on: bool) {
        if t < 0 {
            return;
        }
        let byte = (t >> 3) as usize;
        if byte >= self.bytes.len() {
            return;
        }
        let bit = 1u8 << (t & 7);
        if on {
            self.bytes[byte] |= bit;
        } else {
            self.bytes[byte] &= !bit;
        }
    }
}

/// The obfuscated tech counters in `LeaderDataEncrypt` (`LeaderData +28344`).
/// The engine stores them XORed; the XOR keys are `[measured]` from `Leader::lose_tech`
/// `0x006D2850` and `LeaderData::get_city_limit` `0x006D6130`.
///
/// | field | offset | XOR key |
/// |---|---|---|
/// | `ages` | `+220` | (unset in the read paths seen) |
/// | `epochs` | `+224` | `0x69587` |
/// | `discovered` | `+228` | `0x13985` |
/// | `epoch[4]` | `+232` | `0x63187` |
///
/// `epoch[cat]` is the count of researched library techs in that research category, and
/// `epoch[Civic]` is what sets the city limit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TechCounters {
    pub ages: i32,
    /// Total library ("epoch") techs — the quantity the age gate compares against.
    pub epochs: i32,
    /// Non-library techs.
    pub discovered: i32,
    /// Per research category.
    pub epoch: [i32; 4],
}

pub const XOR_EPOCHS: u32 = 0x69587;
pub const XOR_DISCOVERED: u32 = 0x13985;
pub const XOR_EPOCH_CAT: u32 = 0x63187;
pub const XOR_AGES: u32 = 0x62766;

/// `LeaderData::get_city_limit` `0x006D6130`. [measured]
///
/// `epoch[Civic] + (bantu && epoch[Civic] != 0 ? BANTU_CITY_LIMIT : 0) + 1
///  + (pyramids ? PYRAMIDS_CITY_LIMIT : 0)`.
pub fn city_limit(
    rules: &CityRules,
    counters: &TechCounters,
    has_tribe: bool,
    bantu: bool,
    pyramids: bool,
) -> i32 {
    let mut n = counters.epoch[ResearchCat::Civic as usize];
    if has_tribe && bantu && n != 0 {
        n += rules.bantu_city_limit;
    }
    let mut n = n + 1;
    if pyramids {
        n += rules.pyramids_city_limit;
    }
    n
}

/// `LeaderData::at_city_limit` `0x006E0D30` — `get_city_limit() <= get_total_cities()`.
/// [measured]
pub fn at_city_limit(limit: i32, total_cities: i32) -> bool {
    limit <= total_cities
}

/// `LeaderData::get_city_upgrade_level` `0x006D6D90` — `CITY_BUILDINGS` for TOWN,
/// `METRO_BUILDINGS` otherwise. [measured]
pub fn city_upgrade_level(rules: &CityRules, target_type: i32) -> i32 {
    if target_type == ty::TOWN {
        rules.city_buildings
    } else {
        rules.metro_buildings
    }
}

/// `LeaderData::techs_per_age` `0x006D7280` — library techs required to reach an age.
/// [measured]
///
/// `age` is `TechTypeData::age` (`+456`) of the age type. `start_age` and `end_age` come
/// from the game options block at `[0x00C061E8] +0x34 / +0x36`. In a full game
/// (`start = 0`, `end > 6`) this is simply `age * 4 + 2` — 2 techs for Classical, 6 for
/// Medieval, ... 26 for Information.
pub fn techs_per_age(age: i32, start_age: i32, end_age: i32) -> i32 {
    if start_age == 0 && end_age > 6 {
        return age * 4 + 2;
    }
    let n = end_age - start_age + 1;
    if n == 0 {
        return 0;
    }
    let r = ((age - start_age) + 1) * (28 / n) - 2;
    if start_age != 0 || age != 0 || r != 1 {
        r
    } else {
        2
    }
}

/// Static rules for one tech, as loaded from `ron-data/techrules.xml` into
/// `TechType` / `TypeData`.
///
/// Field offsets in `TypeData`: `job_time +8`, `tribe_mask +16`, `cat +20`,
/// `costs[6] +24`, `preq[3] +48`, `where +64`, `name +96`. `TechTypeData::age +456`.
/// [measured, PDB]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TechRule {
    pub name: String,
    /// `TechTypeData::age` `+456`.
    pub age: i32,
    /// `TypeData::costs[6]`, in FOOD/TIMBER/WEALTH/KNOWLEDGE/METAL/OIL order.
    pub costs: [i32; NUM_RES],
    /// `TypeData::preq[3]` as *names*; resolution to `TypeIndex` needs the full type table.
    pub preq: [Option<String>; 3],
    /// `TypeData::job_time` `+8`, in frames (15 frames == 1 game second).
    pub job_time: u32,
    /// `TypeData::tribe_mask` `+16`, one bit per tribe, from the `TRIBE_MASK` digit string.
    pub tribe_mask: u32,
    /// `TypeData::where` `+64` as a name — the building that researches it.
    pub where_name: String,
}

/// Parse `ron-data/techrules.xml`. [structure — the shipped file's schema is data, but
/// `Type::load_cost` `0x00663BA0` was located and **not** fully decoded, so the cost
/// tokenizer here is a reimplementation from the observed strings, not a port.]
///
/// Cost strings look like `"25k/25f"`, `"250k/100o"`, `"12f"`. The letter set is exactly
/// the six `<ABBREV>` values in `ron-data/resourcerules.xml`: F T G K M O.
pub fn parse_techrules(xml: &str) -> Vec<TechRule> {
    let mut out = Vec::new();
    for block in split_blocks(xml, "TECH") {
        let name = tag(&block, "NAME").unwrap_or_default();
        let age = tag(&block, "AGE")
            .and_then(|s| s.trim().parse::<i32>().ok())
            .unwrap_or(-1);
        let costs = parse_cost(&tag(&block, "COST").unwrap_or_default());
        let preq = [
            preq_name(tag(&block, "PREQ0")),
            preq_name(tag(&block, "PREQ1")),
            preq_name(tag(&block, "PREQ2")),
        ];
        let job_time = tag(&block, "JOB_TIME")
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0);
        let tribe_mask = tag(&block, "TRIBE_MASK")
            .map(|s| {
                let mut m = 0u32;
                for (i, ch) in s.trim().chars().take(32).enumerate() {
                    if ch == '1' {
                        m |= 1 << i;
                    }
                }
                m
            })
            .unwrap_or(0);
        let where_name = tag(&block, "WHERE").unwrap_or_default();
        out.push(TechRule {
            name,
            age,
            costs,
            preq,
            job_time,
            tribe_mask,
            where_name,
        });
    }
    out
}

fn preq_name(s: Option<String>) -> Option<String> {
    match s {
        None => None,
        Some(v) => {
            let v = v.trim();
            if v.is_empty() || v.eq_ignore_ascii_case("none") || v.eq_ignore_ascii_case("disable") {
                None
            } else {
                Some(v.to_string())
            }
        }
    }
}

/// `"25k/25f"` -> `costs[KNOWLEDGE] = 25, costs[FOOD] = 25`.
pub fn parse_cost(s: &str) -> [i32; NUM_RES] {
    let mut out = [0i32; NUM_RES];
    for part in s.split('/') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
        let letters: String = part
            .chars()
            .skip_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .trim()
            .to_string();
        let Ok(n) = digits.parse::<i32>() else {
            continue;
        };
        let key = letters.chars().next().map(|c| c.to_ascii_lowercase());
        if let Some(k) = key {
            if let Some(idx) = RES_ABBREV.iter().position(|&a| a == k) {
                out[idx] += n;
            }
        }
    }
    out
}

fn split_blocks(xml: &str, tag_name: &str) -> Vec<String> {
    let open = format!("<{tag_name}>");
    let close = format!("</{tag_name}>");
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find(&open) {
        let after = &rest[i + open.len()..];
        let Some(j) = after.find(&close) else { break };
        out.push(after[..j].to_string());
        rest = &after[j + close.len()..];
    }
    out
}

fn tag(block: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let i = block.find(&open)?;
    let after = &block[i + open.len()..];
    let j = after.find(&close)?;
    Some(after[..j].trim().to_string())
}

/// The runtime tech state of one player.
#[derive(Clone, Debug, Default)]
pub struct TechState {
    pub tech: TechBitMask,
    pub counters: TechCounters,
    /// The three invalidation bits written by both one-shot paths. `gain_tech` first ORs
    /// `0x0100_0000`, then `0x0200_0000`, and finally `0x0c00_0000`; `lose_tech` performs
    /// the final OR after `reset_obs_flags`. [measured, `0x006DCB99`, `0x006DD027`,
    /// `0x006E0315`, `0x006D2AF6..0x006D2AFD`]
    pub dirty_flags: u32,
}

impl TechState {
    /// `LeaderData::has_tech` `0x006E0C80` — the *raw* half of it. [measured]
    ///
    /// The full function is:
    /// `t == -1 -> true`; `t == -2 -> false`; `t < 50 -> true` (resources and gaia types
    /// are always "held"); then, for a type whose `vt[+0x14]` predicate is set (the
    /// build/wonder range 414..=542), it defers to `has_preq(t)`; otherwise it is the raw
    /// bit. `is_upgrade_type` selects that branch.
    pub fn has_tech(&self, t: i32, is_upgrade_type: bool, has_preq: impl FnOnce() -> bool) -> bool {
        if t == -1 {
            return true;
        }
        if t == -2 {
            return false;
        }
        if t < 50 {
            return true;
        }
        if is_upgrade_type {
            return has_preq();
        }
        self.tech.get(t)
    }

    /// The age gate inside `LeaderData::has_preq` `0x006DB810`: for an *age* type
    /// (`TypeIndex` in `[544, 550]`), require
    /// `epochs >= techs_per_age(age, start_age, end_age)`. [measured]
    pub fn age_requirement_met(&self, age: i32, start_age: i32, end_age: i32) -> bool {
        self.counters.epochs >= techs_per_age(age, start_age, end_age)
    }

    /// Low-level bit/counter helper retained for callers that own the one-shot effects.
    /// Queue completion should use [`execute_gain_tech_cohort`]: unlike this structural
    /// helper, that transaction includes retail's age counter, dirty flags, callbacks,
    /// and resource effects. [measured, `Leader::gain_tech` `0x006DCB60`]
    pub fn gain(&mut self, t: i32) {
        if self.tech.get(t) {
            return;
        }
        self.tech.set(t, true);
        if (ty::BASE_EPOCHTYPES..ty::END_EPOCHTYPES).contains(&t) {
            self.counters.epochs += 1;
            self.counters.epoch[research_cat(t) as usize] += 1;
        } else if !(ty::CLASSICAL_AGE..=ty::INFORMATION_AGE).contains(&t) {
            self.counters.discovered += 1;
        }
    }

    /// `Leader::lose_tech` `0x006D2850`, counter half. [measured]
    pub fn lose(&mut self, t: i32) {
        if !self.tech.get(t) {
            return;
        }
        self.tech.set(t, false);
        if (ty::BASE_EPOCHTYPES..ty::END_EPOCHTYPES).contains(&t) {
            self.counters.epochs -= 1;
            self.counters.epoch[research_cat(t) as usize] -= 1;
        } else if !(ty::CLASSICAL_AGE..=ty::INFORMATION_AGE).contains(&t) {
            self.counters.discovered -= 1;
        }
    }
}

/// Counter family selected by the virtual `Type::{is_age,is_epoch}` tests in the generic
/// head of `Leader::gain_tech`. [measured, `0x006DCD78..0x006DCE6C`]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TechCounterClass {
    Age,
    Epoch(ResearchCat),
    Discovered,
}

#[inline]
pub fn tech_counter_class(type_index: i32) -> TechCounterClass {
    if (ty::CLASSICAL_AGE..=ty::INFORMATION_AGE).contains(&type_index) {
        TechCounterClass::Age
    } else if (ty::BASE_EPOCHTYPES..ty::END_EPOCHTYPES).contains(&type_index) {
        TechCounterClass::Epoch(research_cat(type_index))
    } else {
        TechCounterClass::Discovered
    }
}

/// Resource grant multiplier selected before `LeaderData::bucket_add`. Both retail's
/// team-mode arm and its handicap arm reduce to a wrapping integer multiply here; their
/// admission predicates remain caller-owned game/rule facts. [measured,
/// `0x006DD071..0x006DD15F`, `0x006DD214..0x006DD260`]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TechResourceGrantScale {
    Standard,
    Multiply(i32),
}

impl TechResourceGrantScale {
    #[inline]
    fn apply(self, amount: i32) -> i32 {
        match self {
            Self::Standard => amount,
            Self::Multiply(multiplier) => amount.wrapping_mul(multiplier),
        }
    }
}

/// World/rules facts consumed by the exact resource-unlock cohort of `gain_tech`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GainTechCohortContext {
    /// `GameData[+0x821] & 8` bypasses the entire grant/sell/Classical-bonus block.
    pub suppress_resource_effects: bool,
    pub grant_scale: TechResourceGrantScale,
    /// The `game byte +0x2d == 8` arm overwrites each newly unlocked resource with 99,999.
    pub unlimited_resources: bool,
    /// Base Knowledge award from `Rules +0x60c` when Classical Age is gained while the
    /// relevant tribe bonus and rule toggle are both active. `None` means the arm is off.
    pub classical_knowledge_bonus: Option<i32>,
    /// Whether this leader is `Game::my_player`; gates `IFaceMainBase::age_update`.
    pub local_player: bool,
    /// `Game::frame`, stored in `LeaderData::age_stamp[type-CLASSICAL_AGE]` for a new age.
    pub game_frame: i32,
}

impl Default for GainTechCohortContext {
    fn default() -> Self {
        Self {
            suppress_resource_effects: false,
            grant_scale: TechResourceGrantScale::Standard,
            unlimited_resources: false,
            classical_knowledge_bonus: None,
            local_player: false,
            game_frame: 0,
        }
    }
}

pub const TECH_GAIN_ENTER_DIRTY: u32 = 0x0100_0000;
pub const TECH_GAIN_RESOURCES_DIRTY: u32 = 0x0200_0000;
pub const TECH_EFFECTS_FINAL_DIRTY: u32 = 0x0c00_0000;
pub const TECH_UNLIMITED_RESOURCE_AMOUNT: i32 = 99_999;
pub const TECH_RESOURCE_SELL_FLOOR: i32 = 100;

/// One ordered write/callback in the recovered generic gain/lose cohort. The two
/// `Complete*Effects` variants are explicit mandatory boundaries around the still-large
/// tech-specific middle of `gain_tech`; they prevent this exact cohort from silently
/// skipping that world-owned work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TechOneShotMutation {
    OrDirtyFlags(u32),
    MarkGameTechDirty,
    CounterChanged {
        type_index: i32,
        before: TechCounters,
        after: TechCounters,
    },
    RefreshAgeInterface,
    RecordAgeStamp {
        type_index: i32,
        slot: usize,
        frame: i32,
    },
    SetTechBit {
        type_index: i32,
        held: bool,
    },
    CompleteGainPreBitEffects(i32),
    CompleteGainAfterBitEffects(i32),
    GrantResource {
        resource: usize,
        amount: i32,
    },
    SetResourceAmount {
        resource: usize,
        amount: i32,
    },
    SellResource {
        resource: usize,
        mode: i32,
    },
    CompleteGainPostResourceEffects(i32),
    OutdateCamera,
    RefreshAgeConsumers(i32),
    TerrainOilGain(i32),
    CompleteLoseDependentSweep(i32),
    FixTechFlags,
    TerrainOilLose(i32),
    ResetObservationFlags,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TechOneShotMutationReceipt {
    pub mutation: TechOneShotMutation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TechOneShotError {
    MutationReceiptMismatch {
        expected: TechOneShotMutation,
        observed: TechOneShotMutation,
    },
    SellDidNotReduceResource {
        resource: usize,
        before: i32,
        after: i32,
    },
}

/// Typed object/rules boundary for the recovered generic one-shot cohort.
pub trait TechOneShotHost {
    /// Mandatory callback used for every ordered write/effect boundary.
    fn apply_tech_mutation(
        &mut self,
        state: &TechState,
        mutation: TechOneShotMutation,
    ) -> TechOneShotMutationReceipt;

    /// Six `has_preq(resource)` calls captured immediately before the tech bit is set.
    fn resource_prerequisite_held(&mut self, resource: usize) -> bool;
    /// `ResourceTypeData +0x30`: the tech that initially unlocks this resource bucket.
    fn resource_unlock_tech(&mut self, resource: usize) -> i32;
    /// `Rules +0x600 + resource*4`.
    fn resource_unlock_amount(&mut self, resource: usize) -> i32;
    /// `ResourceTypeData +0x4c`: gaining this tech sells the resource down to 100.
    fn resource_sell_tech(&mut self, resource: usize) -> i32;
    /// De-obfuscated current bucket amount, re-read after every `action_sell`.
    fn resource_amount(&mut self, resource: usize) -> i32;

    /// `Type::is_resource` at the head of `Leader::lose_tech`.
    fn lose_type_is_resource(&mut self, type_index: i32) -> bool;
    /// `Type` virtual `+0x28`; false skips the bit/counter/TerrainOil body.
    fn lose_type_is_removable(&mut self, type_index: i32) -> bool;
}

fn require_tech_mutation<H: TechOneShotHost>(
    state: &TechState,
    host: &mut H,
    expected: TechOneShotMutation,
) -> Result<(), TechOneShotError> {
    let observed = host.apply_tech_mutation(state, expected);
    if observed.mutation != expected {
        return Err(TechOneShotError::MutationReceiptMismatch {
            expected,
            observed: observed.mutation,
        });
    }
    Ok(())
}

fn change_gain_counter(state: &mut TechState, type_index: i32) {
    match tech_counter_class(type_index) {
        TechCounterClass::Age => state.counters.ages = state.counters.ages.wrapping_add(1),
        TechCounterClass::Epoch(category) => {
            state.counters.epochs = state.counters.epochs.wrapping_add(1);
            let slot = category as usize;
            state.counters.epoch[slot] = state.counters.epoch[slot].wrapping_add(1);
        }
        TechCounterClass::Discovered => {
            state.counters.discovered = state.counters.discovered.wrapping_add(1);
        }
    }
}

fn execute_gain_resource_effects<H: TechOneShotHost>(
    state: &TechState,
    host: &mut H,
    type_index: i32,
    prerequisite_before_gain: [bool; NUM_RES],
    context: GainTechCohortContext,
) -> Result<(usize, usize), TechOneShotError> {
    if context.suppress_resource_effects {
        return Ok((0, 0));
    }

    let mut grants = 0usize;
    let mut sales = 0usize;
    for (resource, prerequisite_held) in prerequisite_before_gain.into_iter().enumerate() {
        if prerequisite_held || host.resource_unlock_tech(resource) != type_index {
            continue;
        }
        let amount = context
            .grant_scale
            .apply(host.resource_unlock_amount(resource));
        require_tech_mutation(
            state,
            host,
            TechOneShotMutation::GrantResource { resource, amount },
        )?;
        grants += 1;
        if context.unlimited_resources {
            require_tech_mutation(
                state,
                host,
                TechOneShotMutation::SetResourceAmount {
                    resource,
                    amount: TECH_UNLIMITED_RESOURCE_AMOUNT,
                },
            )?;
        }
    }

    for resource in 0..NUM_RES {
        let sell_tech = host.resource_sell_tech(resource);
        // Retail performs the tech comparison before excluding Wealth and Knowledge.
        if sell_tech != type_index || resource == RES_WEALTH || resource == RES_KNOWLEDGE {
            continue;
        }
        let mut before = host.resource_amount(resource);
        while before > TECH_RESOURCE_SELL_FLOOR {
            require_tech_mutation(
                state,
                host,
                TechOneShotMutation::SellResource { resource, mode: 0 },
            )?;
            sales += 1;
            let after = host.resource_amount(resource);
            if after >= before {
                return Err(TechOneShotError::SellDidNotReduceResource {
                    resource,
                    before,
                    after,
                });
            }
            before = after;
        }
    }

    if type_index == ty::CLASSICAL_AGE {
        if let Some(base_amount) = context.classical_knowledge_bonus {
            let amount = context.grant_scale.apply(base_amount);
            require_tech_mutation(
                state,
                host,
                TechOneShotMutation::GrantResource {
                    resource: RES_KNOWLEDGE,
                    amount,
                },
            )?;
            grants += 1;
        }
    }
    Ok((grants, sales))
}

/// Receipt for the executable generic cohort surrounding `Leader::gain_tech`'s
/// tech-specific middle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GainTechCohortReceipt {
    pub type_index: i32,
    pub was_new: bool,
    pub counter_class: TechCounterClass,
    pub resource_grants: usize,
    pub resource_sales: usize,
}

/// Execute the exact generic counter/bit/resource/tail cohort of `Leader::gain_tech`.
/// [measured, `0x006DCB60..0x006E05D9`; resource block
/// `0x006DD02D..0x006DD268`]
///
/// The two mandatory completion mutations bracket the unported tech-specific body. This
/// function therefore closes a real cohort without pretending the 45 special branches are
/// absent. State mutation is deliberately not rolled back after a divergent host receipt:
/// retail's callbacks are infallible and may already have published world effects.
pub fn execute_gain_tech_cohort<H: TechOneShotHost>(
    state: &mut TechState,
    type_index: i32,
    context: GainTechCohortContext,
    host: &mut H,
) -> Result<GainTechCohortReceipt, TechOneShotError> {
    state.dirty_flags |= TECH_GAIN_ENTER_DIRTY;
    require_tech_mutation(
        state,
        host,
        TechOneShotMutation::OrDirtyFlags(TECH_GAIN_ENTER_DIRTY),
    )?;
    require_tech_mutation(state, host, TechOneShotMutation::MarkGameTechDirty)?;

    let was_new = !state.tech.get(type_index);
    let counter_class = tech_counter_class(type_index);
    if was_new {
        let before = state.counters;
        change_gain_counter(state, type_index);
        require_tech_mutation(
            state,
            host,
            TechOneShotMutation::CounterChanged {
                type_index,
                before,
                after: state.counters,
            },
        )?;
        if matches!(counter_class, TechCounterClass::Age) {
            if context.local_player {
                require_tech_mutation(state, host, TechOneShotMutation::RefreshAgeInterface)?;
            }
            require_tech_mutation(
                state,
                host,
                TechOneShotMutation::RecordAgeStamp {
                    type_index,
                    slot: (type_index - ty::CLASSICAL_AGE) as usize,
                    frame: context.game_frame,
                },
            )?;
        }
    }

    require_tech_mutation(
        state,
        host,
        TechOneShotMutation::CompleteGainPreBitEffects(type_index),
    )?;
    let mut prerequisite_before_gain = [false; NUM_RES];
    for (resource, held) in prerequisite_before_gain.iter_mut().enumerate() {
        *held = host.resource_prerequisite_held(resource);
    }

    state.tech.set(type_index, true);
    require_tech_mutation(
        state,
        host,
        TechOneShotMutation::SetTechBit {
            type_index,
            held: true,
        },
    )?;
    require_tech_mutation(
        state,
        host,
        TechOneShotMutation::CompleteGainAfterBitEffects(type_index),
    )?;
    state.dirty_flags |= TECH_GAIN_RESOURCES_DIRTY;
    require_tech_mutation(
        state,
        host,
        TechOneShotMutation::OrDirtyFlags(TECH_GAIN_RESOURCES_DIRTY),
    )?;

    let (resource_grants, resource_sales) =
        execute_gain_resource_effects(state, host, type_index, prerequisite_before_gain, context)?;
    require_tech_mutation(
        state,
        host,
        TechOneShotMutation::CompleteGainPostResourceEffects(type_index),
    )?;

    state.dirty_flags |= TECH_EFFECTS_FINAL_DIRTY;
    require_tech_mutation(
        state,
        host,
        TechOneShotMutation::OrDirtyFlags(TECH_EFFECTS_FINAL_DIRTY),
    )?;
    if matches!(counter_class, TechCounterClass::Age) {
        require_tech_mutation(
            state,
            host,
            TechOneShotMutation::RefreshAgeConsumers(type_index),
        )?;
    }
    require_tech_mutation(state, host, TechOneShotMutation::TerrainOilGain(type_index))?;

    Ok(GainTechCohortReceipt {
        type_index,
        was_new,
        counter_class,
        resource_grants,
        resource_sales,
    })
}

/// Receipt for the executable generic body/tail of `Leader::lose_tech`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoseTechCohortReceipt {
    pub type_index: i32,
    pub removed: bool,
    pub counter_class: TechCounterClass,
}

fn recompute_age_count(state: &TechState) -> i32 {
    (ty::CLASSICAL_AGE..=ty::INFORMATION_AGE)
        .filter(|&type_index| state.tech.get(type_index))
        .count() as i32
}

fn recompute_epoch_category(state: &TechState, category: ResearchCat) -> i32 {
    (ty::BASE_EPOCHTYPES..ty::END_EPOCHTYPES)
        .filter(|&type_index| research_cat(type_index) == category && state.tech.get(type_index))
        .count() as i32
}

/// Execute the generic `Leader::lose_tech` body with the dependency scan and world work
/// retained as mandatory ordered callbacks. [measured, `0x006D2850..0x006D2B0A`]
pub fn execute_lose_tech_cohort<H: TechOneShotHost>(
    state: &mut TechState,
    type_index: i32,
    host: &mut H,
) -> Result<LoseTechCohortReceipt, TechOneShotError> {
    let counter_class = tech_counter_class(type_index);
    let resource = host.lose_type_is_resource(type_index);
    let removable = resource || host.lose_type_is_removable(type_index);
    if !removable {
        require_tech_mutation(state, host, TechOneShotMutation::ResetObservationFlags)?;
        state.dirty_flags |= TECH_EFFECTS_FINAL_DIRTY;
        require_tech_mutation(
            state,
            host,
            TechOneShotMutation::OrDirtyFlags(TECH_EFFECTS_FINAL_DIRTY),
        )?;
        return Ok(LoseTechCohortReceipt {
            type_index,
            removed: false,
            counter_class,
        });
    }

    let before = state.counters;
    state.tech.set(type_index, false);
    if resource {
        state.counters.discovered = state.counters.discovered.wrapping_sub(1);
    }
    require_tech_mutation(
        state,
        host,
        TechOneShotMutation::SetTechBit {
            type_index,
            held: false,
        },
    )?;

    if !resource {
        require_tech_mutation(
            state,
            host,
            TechOneShotMutation::CompleteLoseDependentSweep(type_index),
        )?;
        require_tech_mutation(state, host, TechOneShotMutation::FixTechFlags)?;
        match counter_class {
            TechCounterClass::Age => {
                require_tech_mutation(state, host, TechOneShotMutation::OutdateCamera)?;
                state.counters.ages = recompute_age_count(state);
            }
            TechCounterClass::Epoch(category) => {
                state.counters.epochs = state.counters.epochs.wrapping_sub(1);
                state.counters.epoch[category as usize] = recompute_epoch_category(state, category);
            }
            TechCounterClass::Discovered => {
                state.counters.discovered = state.counters.discovered.wrapping_sub(1);
            }
        }
    }

    if state.counters != before {
        require_tech_mutation(
            state,
            host,
            TechOneShotMutation::CounterChanged {
                type_index,
                before,
                after: state.counters,
            },
        )?;
    }
    if !resource && matches!(counter_class, TechCounterClass::Age) {
        require_tech_mutation(
            state,
            host,
            TechOneShotMutation::RefreshAgeConsumers(type_index),
        )?;
    }
    if !resource {
        require_tech_mutation(state, host, TechOneShotMutation::TerrainOilLose(type_index))?;
    }
    require_tech_mutation(state, host, TechOneShotMutation::ResetObservationFlags)?;
    state.dirty_flags |= TECH_EFFECTS_FINAL_DIRTY;
    require_tech_mutation(
        state,
        host,
        TechOneShotMutation::OrDirtyFlags(TECH_EFFECTS_FINAL_DIRTY),
    )?;
    Ok(LoseTechCohortReceipt {
        type_index,
        removed: true,
        counter_class,
    })
}

pub const TECH_AUTO_UNLOCK_BUILD_FLAG: u32 = 0x4;
pub const TECH_AUTO_UNLOCK_UNIT_FLAG: u32 = 0x80;
pub const TECH_AUTO_UNLOCK_EXCLUDED_OBJ_MASK: u32 = 0x0400_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TechAutoUnlockClass {
    Building,
    Unit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TechAutoUnlockMutation {
    CheckCityUpgrades {
        town_type: i32,
    },
    RecursivelyGain {
        type_index: i32,
        class: TechAutoUnlockClass,
        coords: [i32; 2],
        tail: [i32; 2],
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TechAutoUnlockMutationReceipt {
    pub mutation: TechAutoUnlockMutation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TechAutoUnlockReceipt {
    pub gained_type: i32,
    pub city_upgrade_checks: usize,
    pub recursive_buildings: usize,
    pub recursive_units: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TechAutoUnlockError {
    MutationReceiptMismatch {
        expected: TechAutoUnlockMutation,
        observed: TechAutoUnlockMutation,
    },
}

/// Live type-table boundary for the two generic propagation sweeps at the end of
/// `Leader::gain_tech`. Every query remains live because each recursive gain may change
/// eligibility for a later ascending candidate.
pub trait TechAutoUnlockHost {
    fn has_prerequisite(&mut self, type_index: i32) -> bool;
    fn effective_prerequisite_count(&mut self, type_index: i32) -> i32;
    fn effective_prerequisite(&mut self, type_index: i32, slot: i32) -> i32;
    fn candidate_is_town(&mut self, type_index: i32) -> bool;
    fn building_flags(&mut self, type_index: i32) -> u32;
    fn type_eligible(&mut self, type_index: i32, strict: i32) -> bool;
    fn has_tech_live(&mut self, type_index: i32) -> bool;
    fn unit_flags(&mut self, type_index: i32) -> u32;
    fn unit_object_masks(&mut self, type_index: i32) -> u32;
    fn apply_auto_unlock(
        &mut self,
        mutation: TechAutoUnlockMutation,
    ) -> TechAutoUnlockMutationReceipt;
}

fn require_auto_unlock<H: TechAutoUnlockHost>(
    host: &mut H,
    expected: TechAutoUnlockMutation,
) -> Result<(), TechAutoUnlockError> {
    let observed = host.apply_auto_unlock(expected);
    if observed.mutation != expected {
        return Err(TechAutoUnlockError::MutationReceiptMismatch {
            expected,
            observed: observed.mutation,
        });
    }
    Ok(())
}

/// Execute the two exact generic auto-unlock sweeps in `Leader::gain_tech`.
/// [measured, `0x006DEBBE..0x006DED7F`]
///
/// Buildings/wonders are visited first in `[414,543)`, then units in `[50,402)`. Recursive
/// gains use zero coordinates and tail `(0,1)`. The executor intentionally does not cache
/// any host fact across candidates or recursion.
pub fn execute_gain_tech_auto_unlocks<H: TechAutoUnlockHost>(
    gained_type: i32,
    host: &mut H,
) -> Result<TechAutoUnlockReceipt, TechAutoUnlockError> {
    let mut receipt = TechAutoUnlockReceipt {
        gained_type,
        city_upgrade_checks: 0,
        recursive_buildings: 0,
        recursive_units: 0,
    };

    for candidate in ty::VILLAGE..543 {
        if !host.has_prerequisite(candidate) {
            continue;
        }
        let count = host.effective_prerequisite_count(candidate).max(0);
        let mut matched = false;
        for slot in 0..count {
            if host.effective_prerequisite(candidate, slot) == gained_type {
                matched = true;
                break;
            }
        }
        if !matched {
            continue;
        }
        if host.candidate_is_town(candidate) {
            let mutation = TechAutoUnlockMutation::CheckCityUpgrades {
                town_type: candidate,
            };
            require_auto_unlock(host, mutation)?;
            receipt.city_upgrade_checks += 1;
        }
        if host.building_flags(candidate) & TECH_AUTO_UNLOCK_BUILD_FLAG != 0
            && host.type_eligible(candidate, 1)
        {
            let mutation = TechAutoUnlockMutation::RecursivelyGain {
                type_index: candidate,
                class: TechAutoUnlockClass::Building,
                coords: [0; 2],
                tail: [0, 1],
            };
            require_auto_unlock(host, mutation)?;
            receipt.recursive_buildings += 1;
        }
    }

    for candidate in 50..402 {
        let preq0 = host.effective_prerequisite(candidate, 0);
        let preq1 = if preq0 == gained_type {
            preq0
        } else {
            host.effective_prerequisite(candidate, 1)
        };
        if preq0 != gained_type && preq1 != gained_type {
            continue;
        }
        if !host.has_prerequisite(candidate)
            || host.has_tech_live(candidate)
            || host.unit_flags(candidate) & TECH_AUTO_UNLOCK_UNIT_FLAG == 0
            || host.unit_object_masks(candidate) & TECH_AUTO_UNLOCK_EXCLUDED_OBJ_MASK != 0
            || !host.type_eligible(candidate, 1)
        {
            continue;
        }
        let mutation = TechAutoUnlockMutation::RecursivelyGain {
            type_index: candidate,
            class: TechAutoUnlockClass::Unit,
            coords: [0; 2],
            tail: [0, 1],
        };
        require_auto_unlock(host, mutation)?;
        receipt.recursive_units += 1;
    }
    Ok(receipt)
}

/// `Leader::set_age(int age)` `0x006D25A0` — the age ladder, which is the clearest
/// statement of how ages relate to techs. [measured, structure]
///
/// * clamp `age` to `[0, 7]`, then `target = age + CLASSICAL_AGE`;
/// * for `t` in `INFORMATION_AGE ..= target` descending: if held, `lose_tech(t)`;
/// * for `t` in `CLASSICAL_AGE .. target`: if not held, `gain_tech(t)`;
/// * then revalidate: over `[544, 629)`, `[50, 402)` and `[414, 543)`, any held type whose
///   `has_preq` no longer holds is dropped.
///
/// Returns the (target, ranges) so a caller can drive it without this module owning
/// `Leader`.
pub fn set_age_plan(age: i32) -> (i32, [(i32, i32); 3]) {
    let age = age.clamp(0, 7);
    let target = age + ty::CLASSICAL_AGE;
    (
        target,
        [
            (ty::CLASSICAL_AGE, ty::REVALIDATE_END),
            (50, 402),
            (ty::VILLAGE, 543),
        ],
    )
}

/// One completed `Leader::{set_age,set_epoch}` state transaction. [measured,
/// `Leader::set_age` `0x006D25A0`, `Leader::set_epoch` `0x006D26F0`]
///
/// `target` is the exclusive upper bound of the ladder after clamping the caller's
/// requested level. `gained` and `removed_from_ladder` retain retail call order;
/// `invalidated` retains the order of the three prerequisite sweeps.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TechSetReceipt {
    pub target: i32,
    pub gained: Vec<i32>,
    pub removed_from_ladder: Vec<i32>,
    pub invalidated: Vec<i32>,
}

/// Mandatory world effects reached by `Leader::{set_age,set_epoch}` around the tech-bit
/// transaction. Implementations must execute these callbacks; default no-op effects would
/// silently leave unit/build statistics stale.
///
/// `has_preq` is called against the already-mutated [`TechState`]. That is load-bearing:
/// retail removes invalid entries immediately, so a later entry in the same ascending
/// sweep observes all earlier removals.
pub trait TechSetHost {
    /// `LeaderData::has_preq` `0x006DB810`.
    fn has_preq(&mut self, state: &TechState, type_index: i32) -> bool;
    /// The state bit/counter mutation has happened; apply the remaining one-shot effects
    /// of `Leader::gain_tech` `0x006DCB60` before the transaction continues.
    fn gained_tech(&mut self, state: &TechState, type_index: i32);
    /// The state bit/counter mutation has happened; apply the remaining one-shot effects
    /// of `Leader::lose_tech` `0x006D2850` before the transaction continues.
    fn lost_tech(&mut self, state: &TechState, type_index: i32);
    /// `Leader::reset_obs_flags` `0x006E32A0`.
    fn reset_obs_flags(&mut self, state: &TechState);
    /// `Leader::calc_unit_stats` `0x006CF970`.
    fn calc_unit_stats(&mut self, state: &TechState);
    /// `Leader::calc_wall_stats` `0x006CF7C0`.
    fn calc_wall_stats(&mut self, state: &TechState);
    /// `Camera::outdate` `0x00844B80`.
    fn outdate_camera(&mut self, state: &TechState);
}

#[inline]
fn gain_for_set<H: TechSetHost>(
    state: &mut TechState,
    host: &mut H,
    type_index: i32,
    receipt: &mut TechSetReceipt,
) {
    state.gain(type_index);
    host.gained_tech(state, type_index);
    receipt.gained.push(type_index);
}

#[inline]
fn lose_for_set<H: TechSetHost>(
    state: &mut TechState,
    host: &mut H,
    type_index: i32,
    out: &mut Vec<i32>,
) {
    state.lose(type_index);
    host.lost_tech(state, type_index);
    out.push(type_index);
}

/// The common tail of `Leader::set_age` and `Leader::set_epoch`: drop held entries whose
/// prerequisites no longer hold, then refresh every derived consumer in retail order.
/// [measured, `0x006D2615..0x006D26E3` and `0x006D2780..0x006D2843`]
fn revalidate_after_tech_set<H: TechSetHost>(
    state: &mut TechState,
    host: &mut H,
    receipt: &mut TechSetReceipt,
) {
    // The first sweep dispatches Type::is_age (vtable +0x34) and skips age types. The
    // devirtualized body identifies exactly [0x220,0x227) as the age band.
    for type_index in ty::CLASSICAL_AGE..ty::REVALIDATE_END {
        if (ty::CLASSICAL_AGE..=ty::INFORMATION_AGE).contains(&type_index) {
            continue;
        }
        if state.tech.get(type_index) && !host.has_preq(state, type_index) {
            lose_for_set(state, host, type_index, &mut receipt.invalidated);
        }
    }

    // These two bodies test the raw BitMask<806> bit before calling has_preq.
    for (begin, end) in [(50, 402), (ty::VILLAGE, 543)] {
        for type_index in begin..end {
            if state.tech.get(type_index) && !host.has_preq(state, type_index) {
                lose_for_set(state, host, type_index, &mut receipt.invalidated);
            }
        }
    }

    host.reset_obs_flags(state);
    host.calc_unit_stats(state);
    host.calc_wall_stats(state);
    host.outdate_camera(state);
}

impl TechState {
    /// Execute `Leader::set_age(int age)` (`0x006D25A0`) over the checksum-visible tech
    /// mask/counters and a mandatory host for one-shot/world effects. [measured]
    ///
    /// Retail treats `age` as the number of completed age advances: level 0 holds none of
    /// `[CLASSICAL_AGE, INFORMATION_AGE]`, level 7 holds all seven. It first removes the
    /// suffix in descending order, then grants the prefix in ascending order.
    pub fn execute_set_age<H: TechSetHost>(&mut self, age: i32, host: &mut H) -> TechSetReceipt {
        let target = age.clamp(0, 7) + ty::CLASSICAL_AGE;
        let mut receipt = TechSetReceipt {
            target,
            ..TechSetReceipt::default()
        };

        for type_index in (target..=ty::INFORMATION_AGE).rev() {
            if self.tech.get(type_index) {
                lose_for_set(self, host, type_index, &mut receipt.removed_from_ladder);
            }
        }
        for type_index in ty::CLASSICAL_AGE..target {
            if !self.tech.get(type_index) {
                gain_for_set(self, host, type_index, &mut receipt);
            }
        }

        revalidate_after_tech_set(self, host, &mut receipt);
        receipt
    }

    /// Execute `Leader::set_epoch(int category, int level)` (`0x006D26F0`). [measured]
    ///
    /// Categories clamp to `0..=3` and use the retail `ResearchCat` numbering. Levels
    /// clamp to `0..=7`; level 0 removes the category's seven techs and level 7 grants all
    /// seven. Removal is descending and grant is ascending before the shared revalidation
    /// tail runs.
    pub fn execute_set_epoch<H: TechSetHost>(
        &mut self,
        category: i32,
        level: i32,
        host: &mut H,
    ) -> TechSetReceipt {
        let base = match category.clamp(0, 3) {
            0 => 572, // Military / THE_ART_OF_WAR
            1 => 565, // Civic / CITY_STATE
            2 => 558, // Commerce / BARTER
            _ => 551, // Science / WRITTEN_WORD
        };
        let target = base + level.clamp(0, 7);
        let mut receipt = TechSetReceipt {
            target,
            ..TechSetReceipt::default()
        };

        for type_index in (target..=base + 6).rev() {
            if self.tech.get(type_index) {
                lose_for_set(self, host, type_index, &mut receipt.removed_from_ladder);
            }
        }
        for type_index in base..target {
            if !self.tech.get(type_index) {
                gain_for_set(self, host, type_index, &mut receipt);
            }
        }

        revalidate_after_tech_set(self, host, &mut receipt);
        receipt
    }
}

// ---------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- adler32 -----------------------------------------------------------------

    /// Independent reference implementation of the RFC 1950 algorithm, to cross-check the
    /// port of `0x00A46830`. This is a cross-check between two implementations, not
    /// evidence against the binary.
    fn adler32_ref(mut adler: u32, buf: &[u8]) -> u32 {
        let mut s1 = adler & 0xFFFF;
        let mut s2 = (adler >> 16) & 0xFFFF;
        for &b in buf {
            s1 = (s1 + b as u32) % 65521;
            s2 = (s2 + s1) % 65521;
        }
        adler = (s2 << 16) | s1;
        adler
    }

    #[test]
    fn adler32_matches_reference() {
        let mut v: Vec<u8> = Vec::new();
        let mut x: u32 = 12345;
        for _ in 0..20000 {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            v.push((x >> 24) as u8);
        }
        for len in [0usize, 1, 15, 16, 17, 5551, 5552, 5553, 11104, 20000] {
            assert_eq!(
                adler32(1, Some(&v[..len])),
                adler32_ref(1, &v[..len]),
                "len {len}"
            );
        }
    }

    #[test]
    fn adler32_null_buffer_returns_one() {
        // The engine's `if (buf == 0) return 1;` — it discards the incoming accumulator.
        assert_eq!(adler32(0xDEADBEEF, None), 1);
    }

    // ---- vector_dist -------------------------------------------------------------

    #[test]
    fn vector_dist_axis_and_symmetry() {
        assert_eq!(vector_dist(0, 0), 0);
        // On an axis the approximation is exact.
        for n in [1, 7, 24, 1000] {
            assert_eq!(vector_dist(n, 0), n as u32);
            assert_eq!(vector_dist(0, n), n as u32);
            assert_eq!(vector_dist(-n, 0), n as u32);
        }
        // Symmetric in its two arguments and in sign.
        for (a, b) in [(3, 4), (24, 7), (100, 33), (5, 5)] {
            assert_eq!(vector_dist(a, b), vector_dist(b, a));
            assert_eq!(vector_dist(a, b), vector_dist(-a, -b));
        }
    }

    #[test]
    fn vector_dist_is_the_engine_formula() {
        // hi + lo*lo/(2*hi), integer-truncating. 3,4 -> 4 + 9/8 = 5.
        assert_eq!(vector_dist(3, 4), 5);
        // 5,5 -> 5 + 25/10 = 7 (true hypotenuse 7.07).
        assert_eq!(vector_dist(5, 5), 7);
        // Overflow guard: both legs above 59999 switches to hi + lo/2.
        let d = vector_dist(60000, 70000);
        assert_eq!(d, (60000u32 + 70000 * 2) >> 1);
    }

    // ---- city levels -------------------------------------------------------------

    #[test]
    fn level_ladder() {
        assert_eq!(city_level(ty::VILLAGE), CityLevel::Village);
        assert_eq!(city_level(ty::TOWN), CityLevel::Town);
        assert_eq!(city_level(ty::METROPOLIS), CityLevel::Metropolis);
        assert_eq!(city_level(ty::FORBIDDEN_CITY), CityLevel::Metropolis);
        assert_eq!(upgrades_to(ty::VILLAGE), Some(ty::TOWN));
        assert_eq!(upgrades_to(ty::TOWN), Some(ty::METROPOLIS));
        assert_eq!(upgrades_to(ty::METROPOLIS), None);
        assert_eq!(upgrades_to(ty::FORBIDDEN_CITY), None);
    }

    #[test]
    fn pop_values_and_radius() {
        let r = CityRules::RETAIL;
        assert_eq!(pop_value(ty::VILLAGE), 1);
        assert_eq!(pop_value(ty::TOWN), 3);
        assert_eq!(pop_value(ty::METROPOLIS), 5);
        // VILLAGE_POP is 0 in retail rules, so cities grant no pop cap.
        assert_eq!(pop_cap(&r, ty::METROPOLIS), 0);
        // 20 + (level-1)*4
        assert_eq!(city_radius(&r, ty::VILLAGE, false), 20);
        assert_eq!(city_radius(&r, ty::TOWN, false), 24);
        assert_eq!(city_radius(&r, ty::METROPOLIS, false), 28);
        assert_eq!(city_radius(&r, ty::METROPOLIS, true), 32);
    }

    #[test]
    fn upgrade_thresholds() {
        let r = CityRules::RETAIL;
        // CITY_BUILDINGS + 1 == 6 distinct kinds for VILLAGE -> TOWN.
        assert_eq!(num_kinds_needed(&r, ty::VILLAGE, true), 6);
        // METRO_BUILDINGS + 1 == 10 for TOWN -> METROPOLIS.
        assert_eq!(num_kinds_needed(&r, ty::TOWN, true), 10);
        assert_eq!(num_kinds_needed(&r, ty::METROPOLIS, true), 0);
        assert_eq!(num_kinds_needed(&r, ty::VILLAGE, false), 0);

        assert!(!ready_to_upgrade(&r, ty::VILLAGE, true, 5));
        assert!(ready_to_upgrade(&r, ty::VILLAGE, true, 6));
        assert!(!ready_to_upgrade(&r, ty::TOWN, true, 9));
        assert!(ready_to_upgrade(&r, ty::TOWN, true, 10));
        assert!(!ready_to_upgrade(&r, ty::METROPOLIS, true, 40));
    }

    #[test]
    fn enhancer_tables_are_level_minus_one_indexed() {
        let r = CityRules::RETAIL;
        let e = calc_gather_enhancers(&r, 1, 4, 3);
        assert_eq!(e.granary, 20); // granary_bonus[0]
        assert_eq!(e.lumber_mill, 200); // lumbermill_bonus[3]
        assert_eq!(e.smelter, 150); // smelter_bonus[2]
        assert_eq!(e.refinery, 0); // stored as literal 0 by City::calc_gather
        let none = calc_gather_enhancers(&r, 0, 0, 0);
        assert_eq!(none, GatherEnhancers::default());
    }

    #[test]
    fn lumber_ladder_tops_out_at_four() {
        assert_eq!(lumber_level(false, true, true, true), 0);
        assert_eq!(lumber_level(true, false, false, false), 1);
        assert_eq!(lumber_level(true, true, false, false), 2);
        assert_eq!(lumber_level(true, true, true, false), 3);
        assert_eq!(lumber_level(true, true, true, true), 4);
    }

    #[test]
    fn gather_slots_exclude_knowledge_from_total() {
        let b = [
            CityGatherBuilding {
                build_type: ty::FARM,
                gather_max: 4,
                num_gatherers: 1,
            },
            CityGatherBuilding {
                build_type: ty::UNIVERSITY,
                gather_max: 3,
                num_gatherers: 3,
            },
            CityGatherBuilding {
                build_type: ty::MINE,
                gather_max: 2,
                num_gatherers: 0,
            },
            CityGatherBuilding {
                build_type: ty::TEMPLE,
                gather_max: 9,
                num_gatherers: 0,
            },
        ];
        let g = count_gather_slots(&b);
        assert_eq!(g.slots[RES_FOOD], 4);
        assert_eq!(g.slots[RES_KNOWLEDGE], 3);
        assert_eq!(g.slots[RES_METAL], 2);
        assert_eq!(g.free[RES_FOOD], 3);
        assert_eq!(g.free[RES_KNOWLEDGE], 0);
        // Temple is not a gather building; knowledge does not count toward the total.
        assert_eq!(g.total, 6);
    }

    #[test]
    fn taxes_and_literacy_use_retail_constants() {
        let r = CityRules::RETAIL;
        // VILLAGE_TAXES 0 + n*BUILDING_TAXES 0 + MARKET_TAXES 10.
        assert_eq!(city_taxes(&r, 7, true, false, 300, true), 10);
        // Porcelain Tower scales the market term by (300 + 100)/100.
        assert_eq!(city_taxes(&r, 7, true, true, 300, false), 40);
        assert_eq!(city_literacy(&r, true, true), 10);
        assert_eq!(city_literacy(&r, false, true), 0);
    }

    // ---- checksum ----------------------------------------------------------------

    #[test]
    fn city_pod_window_is_108_bytes() {
        let c = CityRecord::default();
        assert_eq!(c.pod_bytes().len(), CITY_POD_LEN);
    }

    #[test]
    fn one_live_city_walks_114_bytes() {
        // 2 bytes of city_flags + the 108-byte POD window + 4 bytes for an empty
        // Array<CaravanLink> length. Names are gated out.
        let c = CityRecord {
            city_flags: 1,
            ..CityRecord::default()
        };
        let mut w = CheckSumWalk::new_channel();
        c.walk(&mut w);
        assert_eq!(w.size, 2 + 108 + 4);

        // A dead slot is never walked at all by check_cities.
        let dead = CityRecord::default();
        let mut w2 = CheckSumWalk::new_channel();
        dead.walk(&mut w2);
        assert_eq!(
            w2.size, 2,
            "City::walk_data still emits the flags word if called directly"
        );
    }

    #[test]
    fn pool_is_160_slots() {
        let p = CityPool::new();
        assert_eq!(p.slots.len(), NUM_PLAYERS);
        let total: usize = p.slots.iter().map(|v| v.len()).sum();
        assert_eq!(total, CITY_POOL_SLOTS);
        assert_eq!(total, 160);
    }

    #[test]
    fn slot_allocation_reuses_the_lowest_dead_slot() {
        let mut p = CityPool::new();
        for _ in 0..3 {
            let i = p.alloc_slot(0);
            p.slots[0][i].city_flags |= 1;
            p.slots[0][i].city = i as i16;
        }
        assert_eq!(p.city_mark[0], 3);
        // free slot 1
        p.slots[0][1].city_flags &= !1;
        let i = p.alloc_slot(0);
        assert_eq!(i, 1);
        assert_eq!(p.city_mark[0], 3);
        // fill everything, then the array grows past 20
        for i in 0..CITIES_PER_PLAYER {
            p.slots[0][i].city_flags |= 1;
        }
        p.city_mark[0] = CITIES_PER_PLAYER as i32;
        let i = p.alloc_slot(0);
        assert_eq!(i, CITIES_PER_PLAYER);
        assert_eq!(p.slots[0].len(), CITIES_PER_PLAYER + 1);
    }

    #[test]
    fn trim_mark_stops_at_the_first_live_trailing_slot() {
        let mut p = CityPool::new();
        for i in 0..5 {
            p.slots[0][i].city_flags |= 1;
        }
        p.city_mark[0] = 5;
        p.slots[0][4].city_flags &= !1;
        p.slots[0][3].city_flags &= !1;
        p.trim_mark(0);
        assert_eq!(p.city_mark[0], 3);
        p.slots[0][1].city_flags &= !1;
        p.trim_mark(0);
        assert_eq!(
            p.city_mark[0], 3,
            "a hole below the top does not shrink the mark"
        );
    }

    #[test]
    fn checksum_ignores_dead_slots_and_inactive_leaders() {
        let mut p = CityPool::new();
        let mut leaders = [false; NUM_PLAYERS];
        leaders[0] = true;
        let empty = check_cities(&p, &leaders);
        assert_eq!(empty, 1, "no walked bytes leaves the adler seed alone");

        p.slots[0][0].city_flags = 1;
        p.slots[0][0].city = 0;
        p.slots[0][0].who = 0;
        let one = check_cities(&p, &leaders);
        assert_ne!(one, empty);

        // A dead slot's contents must not perturb the channel.
        let mut q = p.clone();
        q.slots[0][7].city_flags = 0;
        q.slots[0][7].x = 999999;
        q.slots[0][7].pop = 200;
        assert_eq!(check_cities(&q, &leaders), one);

        // An inactive leader's live cities must not either.
        let mut r = p.clone();
        r.slots[3][0].city_flags = 1;
        r.slots[3][0].who = 3;
        assert_eq!(check_cities(&r, &leaders), one);
        leaders[3] = true;
        assert_ne!(check_cities(&r, &leaders), one);
    }

    #[test]
    fn checksum_is_sensitive_to_every_walked_field() {
        let mut leaders = [false; NUM_PLAYERS];
        leaders[0] = true;
        let mut base = CityPool::new();
        base.slots[0][0] = CityRecord {
            city_flags: 1,
            ..CityRecord::default()
        };
        let b = check_cities(&base, &leaders);

        let mut probes: Vec<(&str, Box<dyn Fn(&mut CityRecord)>)> = Vec::new();
        probes.push(("city", Box::new(|c: &mut CityRecord| c.city = 5)));
        probes.push(("o", Box::new(|c: &mut CityRecord| c.o = 5)));
        probes.push(("reg", Box::new(|c: &mut CityRecord| c.reg = 5)));
        probes.push(("x", Box::new(|c: &mut CityRecord| c.x = 768)));
        probes.push(("y", Box::new(|c: &mut CityRecord| c.y = 768)));
        probes.push((
            "capture_stamp",
            Box::new(|c: &mut CityRecord| c.capture_stamp = 7),
        ));
        probes.push((
            "traded_with",
            Box::new(|c: &mut CityRecord| c.traded_with[7] = 1),
        ));
        probes.push(("granary", Box::new(|c: &mut CityRecord| c.granary = 20)));
        probes.push(("pop", Box::new(|c: &mut CityRecord| c.pop = 3)));
        probes.push(("who", Box::new(|c: &mut CityRecord| c.who = 2)));
        probes.push(("race", Box::new(|c: &mut CityRecord| c.race = 2)));
        probes.push(("ter", Box::new(|c: &mut CityRecord| c.ter[5] = 9)));
        probes.push((
            "vans",
            Box::new(|c: &mut CityRecord| {
                c.vans.items.push(CaravanLink { cara: 1, who: 0 });
                c.vans.capacity = 4;
            }),
        ));
        for (name, f) in probes {
            let mut p = base.clone();
            f(&mut p.slots[0][0]);
            assert_ne!(check_cities(&p, &leaders), b, "channel ignored {name}");
        }

        // The names are gated out of the sync checksum.
        let mut p = base.clone();
        p.slots[0][0].name = "Rome".into();
        p.slots[0][0].id = "rome-1".into();
        assert_eq!(
            check_cities(&p, &leaders),
            b,
            "name/id must not enter the sync checksum"
        );
    }

    #[test]
    fn caravan_array_capacity_is_checksummed() {
        let mut a = CaravanLinkArray {
            items: vec![CaravanLink { cara: 3, who: 1 }],
            capacity: 4,
            grow: -1,
            flags: 0,
        };
        let mut w1 = CheckSumWalk::new_channel();
        a.walk(&mut w1);
        a.capacity = 8;
        let mut w2 = CheckSumWalk::new_channel();
        a.walk(&mut w2);
        assert_ne!(w1.accum, w2.accum, "capacity is part of the walked image");
        assert_eq!(w1.size, 4 + 4 + 2 + 1 + 8);

        // An empty array walks only its length.
        let e = CaravanLinkArray {
            capacity: 4,
            grow: -1,
            ..Default::default()
        };
        let mut w = CheckSumWalk::new_channel();
        e.walk(&mut w);
        assert_eq!(w.size, 4);
    }

    // ---- capture -----------------------------------------------------------------

    #[test]
    fn recapture_modifier_doubles_and_truncates_toward_zero() {
        let r = CityRules::RETAIL;
        assert_eq!(apply_recapture_modifier(&r, 0), 0);
        assert_eq!(apply_recapture_modifier(&r, 37), 74);
        assert_eq!(apply_recapture_modifier(&r, -37), -74);
        // A non-integral ratio truncates toward zero, which is what the +0xFF sign fixup
        // buys: 3/2 at scale 256 is 384.
        let three_halves = CityRules {
            recapture_city_modifier: 384,
            ..CityRules::RETAIL
        };
        assert_eq!(apply_recapture_modifier(&three_halves, 5), 7); // 7.5 -> 7
        assert_eq!(apply_recapture_modifier(&three_halves, -5), -7); // -7.5 -> -7
    }

    #[test]
    fn recapture_gate() {
        assert!(recapture_applies(true, 3, 2, 2));
        assert!(
            !recapture_applies(true, -1, 2, 2),
            "no city index -> no bonus"
        );
        assert!(
            !recapture_applies(false, 3, 2, 2),
            "not a city center -> no bonus"
        );
        assert!(
            !recapture_applies(true, 3, 1, 2),
            "race must equal the attacker"
        );
    }

    #[test]
    fn capture_sets_foreign_flag_and_rehomes_race_for_own_recapture() {
        let old = CityRecord {
            city_flags: 1 | 0x100,
            who: 1,
            race: 1,
            founder: 1,
            pop: 4,
            reg: 3,
            ..CityRecord::default()
        };
        // Enemy capture: race is preserved, foreign flag set.
        let hostile = capture_city(&old, 0, 2, 11, 768, 768, 900, CaptureContext::default());
        assert_eq!(hostile.who, 2);
        assert_eq!(hostile.race, 1);
        assert_eq!(hostile.city_flags & 0x100, 0x100);
        assert_eq!(hostile.pop, 4, "population carries over");
        assert_eq!(hostile.reg, 3);
        assert_eq!(hostile.attack_stamp, 900);
        assert_eq!(hostile.raid_stamp, 900);

        // Recapture by the city's own nation: race re-homes, foreign flag cleared.
        let back = capture_city(
            &hostile,
            0,
            1,
            11,
            768,
            768,
            950,
            CaptureContext {
                same_player: true,
                ..CaptureContext::default()
            },
        );
        assert_eq!(back.race, 1);
        // 0x100 survives the `& 0xF13F` mask, so a recapture does NOT clear it — the city
        // stays flagged until `City::assimilate` 0x00738E90 runs. Recording the engine's
        // behaviour, not the intuitive one.
        assert_eq!(
            back.city_flags & 0x100,
            0x100,
            "0x100 is inside the 0xF13F keep-mask"
        );

        // ... whereas a city that was never cross-team captured never gains the bit.
        let virgin = CityRecord {
            city_flags: 1,
            who: 1,
            race: 1,
            ..CityRecord::default()
        };
        let friendly = capture_city(
            &virgin,
            0,
            1,
            11,
            0,
            0,
            10,
            CaptureContext {
                same_player: true,
                ..CaptureContext::default()
            },
        );
        assert_eq!(friendly.city_flags & 0x100, 0);
    }

    #[test]
    fn capital_capture_records_the_previous_owner() {
        let old = CityRecord {
            city_flags: 1 | 0x10,
            who: 1,
            race: 1,
            founder: 1,
            ..CityRecord::default()
        };
        let taken = capture_city(&old, 0, 2, 11, 0, 0, 10, CaptureContext::default());
        assert_eq!(
            taken.was_capital_flags & 0b10,
            0b10,
            "player 1's capital is remembered"
        );
        assert_eq!(
            taken.city_flags & 0x10,
            0,
            "the capital bit does not transfer"
        );

        // ... and is restored when player 1 takes it back.
        let back = capture_city(
            &taken,
            0,
            1,
            11,
            0,
            0,
            20,
            CaptureContext {
                same_player: true,
                ..CaptureContext::default()
            },
        );
        assert_eq!(back.city_flags & 0x10, 0x10);
        assert_eq!(back.was_capital_flags & 0b10, 0);
    }

    #[test]
    fn capture_leaves_the_center_ten_below_max_hits() {
        assert_eq!(hits_after_capture(3000), 2990);
    }

    // ---- placement ---------------------------------------------------------------

    #[test]
    fn spacing_relaxes_against_a_region_dominant_owner() {
        let r = CityRules::RETAIL;
        // 100-tile region, owner holds 50 -> full 24-tile spacing.
        assert_eq!(city_spacing_for(&r, 100, 50), 24);
        // owner holds 90 of 100 -> 90*... the test is `size*9/10 <= terr`, so 90 relaxes.
        assert_eq!(city_spacing_for(&r, 100, 90), 18);
        assert_eq!(city_spacing_for(&r, 100, 89), 24);
    }

    #[test]
    fn spacing_blocks_only_within_the_same_region() {
        let r = CityRules::RETAIL;
        let existing = [
            SpacingCity {
                who: 0,
                reg: 1,
                tx: 10,
                ty: 10,
            },
            SpacingCity {
                who: 1,
                reg: 2,
                tx: 32,
                ty: 10,
            },
        ];
        let sp = |_who: usize| city_spacing_for(&r, 100, 0);
        // 20 tiles away in the same region -> blocked (24-tile spacing).
        assert_eq!(
            blocked_by_city_spacing(1, 30, 10, &existing, sp),
            Some(BLOCKED_CITY_SPACING)
        );
        // 40 tiles away in the same region -> allowed.
        assert_eq!(blocked_by_city_spacing(1, 50, 10, &existing, sp), None);
        // Adjacent but in a different region -> the loop skips it.
        assert_eq!(blocked_by_city_spacing(3, 33, 10, &existing, sp), None);
    }

    // ---- tech --------------------------------------------------------------------

    #[test]
    fn research_categories_partition_the_epoch_block() {
        // 28 library techs, 7 per category.
        let mut counts = [0usize; 4];
        for t in ty::BASE_EPOCHTYPES..ty::END_EPOCHTYPES {
            counts[research_cat(t) as usize] += 1;
        }
        assert_eq!(counts, [7, 7, 7, 7]);
        // WRITTEN_WORD (551) is Science, BARTER (558) Commerce,
        // CITY_STATE (565) Civic, THE_ART_OF_WAR (572) Military.
        assert_eq!(research_cat(551), ResearchCat::Science);
        assert_eq!(research_cat(558), ResearchCat::Commerce);
        assert_eq!(research_cat(565), ResearchCat::Civic);
        assert_eq!(research_cat(572), ResearchCat::Military);
    }

    #[test]
    fn city_limit_comes_from_civic_techs() {
        let r = CityRules::RETAIL;
        let mut s = TechState::default();
        assert_eq!(city_limit(&r, &s.counters, true, false, false), 1);
        // City-State (565) is the first Civic tech.
        s.gain(565);
        assert_eq!(s.counters.epoch[ResearchCat::Civic as usize], 1);
        assert_eq!(city_limit(&r, &s.counters, true, false, false), 2);
        s.gain(566); // Empire
        assert_eq!(city_limit(&r, &s.counters, true, false, false), 3);
        // Pyramids adds one more.
        assert_eq!(city_limit(&r, &s.counters, true, false, true), 4);
        // Bantu adds one, but only once a Civic tech exists.
        assert_eq!(city_limit(&r, &s.counters, true, true, false), 4);
        let empty = TechCounters::default();
        assert_eq!(city_limit(&r, &empty, true, true, false), 1);

        assert!(at_city_limit(3, 3));
        assert!(!at_city_limit(3, 2));
    }

    #[test]
    fn epoch_counters_track_gain_and_lose() {
        let mut s = TechState::default();
        s.gain(551); // Written Word, Science
        s.gain(572); // The Art of War, Military
        assert_eq!(s.counters.epochs, 2);
        assert_eq!(s.counters.epoch, [1, 0, 0, 1]);
        s.gain(551); // idempotent
        assert_eq!(s.counters.epochs, 2);
        s.lose(551);
        assert_eq!(s.counters.epochs, 1);
        assert_eq!(s.counters.epoch, [1, 0, 0, 0]);
        // Age types do not count toward either counter.
        s.gain(ty::CLASSICAL_AGE);
        assert_eq!(s.counters.epochs, 1);
        assert_eq!(s.counters.discovered, 0);
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum OneShotEvent {
        Mutation(TechOneShotMutation),
        ResourcePreq(usize, bool),
        ResourceUnlock(usize, i32),
        ResourceUnlockAmount(usize, i32),
        ResourceSellTech(usize, i32),
        ResourceAmount(usize, i32),
        LoseIsResource(i32, bool),
        LoseIsRemovable(i32, bool),
    }

    struct OneShotProbe {
        resource_preq: [bool; NUM_RES],
        resource_unlock: [i32; NUM_RES],
        resource_unlock_amount: [i32; NUM_RES],
        resource_sell_tech: [i32; NUM_RES],
        resource_amounts: [Vec<i32>; NUM_RES],
        lose_is_resource: bool,
        lose_is_removable: bool,
        mutation_override: Option<TechOneShotMutation>,
        events: Vec<OneShotEvent>,
    }

    impl Default for OneShotProbe {
        fn default() -> Self {
            Self {
                resource_preq: [false; NUM_RES],
                resource_unlock: [-1; NUM_RES],
                resource_unlock_amount: [0; NUM_RES],
                resource_sell_tech: [-1; NUM_RES],
                resource_amounts: std::array::from_fn(|_| Vec::new()),
                lose_is_resource: false,
                lose_is_removable: true,
                mutation_override: None,
                events: Vec::new(),
            }
        }
    }

    impl TechOneShotHost for OneShotProbe {
        fn apply_tech_mutation(
            &mut self,
            _state: &TechState,
            mutation: TechOneShotMutation,
        ) -> TechOneShotMutationReceipt {
            self.events.push(OneShotEvent::Mutation(mutation));
            TechOneShotMutationReceipt {
                mutation: self.mutation_override.unwrap_or(mutation),
            }
        }

        fn resource_prerequisite_held(&mut self, resource: usize) -> bool {
            let value = self.resource_preq[resource];
            self.events
                .push(OneShotEvent::ResourcePreq(resource, value));
            value
        }

        fn resource_unlock_tech(&mut self, resource: usize) -> i32 {
            let value = self.resource_unlock[resource];
            self.events
                .push(OneShotEvent::ResourceUnlock(resource, value));
            value
        }

        fn resource_unlock_amount(&mut self, resource: usize) -> i32 {
            let value = self.resource_unlock_amount[resource];
            self.events
                .push(OneShotEvent::ResourceUnlockAmount(resource, value));
            value
        }

        fn resource_sell_tech(&mut self, resource: usize) -> i32 {
            let value = self.resource_sell_tech[resource];
            self.events
                .push(OneShotEvent::ResourceSellTech(resource, value));
            value
        }

        fn resource_amount(&mut self, resource: usize) -> i32 {
            let value = self.resource_amounts[resource].remove(0);
            self.events
                .push(OneShotEvent::ResourceAmount(resource, value));
            value
        }

        fn lose_type_is_resource(&mut self, type_index: i32) -> bool {
            self.events.push(OneShotEvent::LoseIsResource(
                type_index,
                self.lose_is_resource,
            ));
            self.lose_is_resource
        }

        fn lose_type_is_removable(&mut self, type_index: i32) -> bool {
            self.events.push(OneShotEvent::LoseIsRemovable(
                type_index,
                self.lose_is_removable,
            ));
            self.lose_is_removable
        }
    }

    struct AutoUnlockProbe {
        has_prerequisite: Vec<bool>,
        prerequisites: Vec<Vec<i32>>,
        is_town: Vec<bool>,
        building_flags: Vec<u32>,
        eligible: Vec<bool>,
        held: Vec<bool>,
        unit_flags: Vec<u32>,
        object_masks: Vec<u32>,
        mutation_override: Option<TechAutoUnlockMutation>,
        mutations: Vec<TechAutoUnlockMutation>,
    }

    impl Default for AutoUnlockProbe {
        fn default() -> Self {
            Self {
                has_prerequisite: vec![false; ty::NUM_TYPES],
                prerequisites: vec![Vec::new(); ty::NUM_TYPES],
                is_town: vec![false; ty::NUM_TYPES],
                building_flags: vec![0; ty::NUM_TYPES],
                eligible: vec![false; ty::NUM_TYPES],
                held: vec![false; ty::NUM_TYPES],
                unit_flags: vec![0; ty::NUM_TYPES],
                object_masks: vec![0; ty::NUM_TYPES],
                mutation_override: None,
                mutations: Vec::new(),
            }
        }
    }

    impl TechAutoUnlockHost for AutoUnlockProbe {
        fn has_prerequisite(&mut self, type_index: i32) -> bool {
            self.has_prerequisite[type_index as usize]
        }

        fn effective_prerequisite_count(&mut self, type_index: i32) -> i32 {
            self.prerequisites[type_index as usize].len() as i32
        }

        fn effective_prerequisite(&mut self, type_index: i32, slot: i32) -> i32 {
            self.prerequisites[type_index as usize]
                .get(slot as usize)
                .copied()
                .unwrap_or(-1)
        }

        fn candidate_is_town(&mut self, type_index: i32) -> bool {
            self.is_town[type_index as usize]
        }

        fn building_flags(&mut self, type_index: i32) -> u32 {
            self.building_flags[type_index as usize]
        }

        fn type_eligible(&mut self, type_index: i32, strict: i32) -> bool {
            assert_eq!(strict, 1);
            self.eligible[type_index as usize]
        }

        fn has_tech_live(&mut self, type_index: i32) -> bool {
            self.held[type_index as usize]
        }

        fn unit_flags(&mut self, type_index: i32) -> u32 {
            self.unit_flags[type_index as usize]
        }

        fn unit_object_masks(&mut self, type_index: i32) -> u32 {
            self.object_masks[type_index as usize]
        }

        fn apply_auto_unlock(
            &mut self,
            mutation: TechAutoUnlockMutation,
        ) -> TechAutoUnlockMutationReceipt {
            self.mutations.push(mutation);
            TechAutoUnlockMutationReceipt {
                mutation: self.mutation_override.unwrap_or(mutation),
            }
        }
    }

    #[test]
    fn gain_tech_auto_unlocks_scan_buildings_then_units_with_live_filters() {
        let gained_type = 600;
        let mut host = AutoUnlockProbe::default();

        host.has_prerequisite[ty::TOWN as usize] = true;
        host.prerequisites[ty::TOWN as usize] = vec![gained_type];
        host.is_town[ty::TOWN as usize] = true;

        host.has_prerequisite[ty::UNIVERSITY as usize] = true;
        host.prerequisites[ty::UNIVERSITY as usize] = vec![599, gained_type];
        host.building_flags[ty::UNIVERSITY as usize] = TECH_AUTO_UNLOCK_BUILD_FLAG;
        host.eligible[ty::UNIVERSITY as usize] = true;

        host.prerequisites[60] = vec![gained_type, -1];
        host.has_prerequisite[60] = true;
        host.unit_flags[60] = TECH_AUTO_UNLOCK_UNIT_FLAG;
        host.eligible[60] = true;

        // A held candidate and an excluded object class both match the gained prereq, but
        // retail filters them before the recursive callback.
        host.prerequisites[61] = vec![-1, gained_type];
        host.has_prerequisite[61] = true;
        host.held[61] = true;
        host.unit_flags[61] = TECH_AUTO_UNLOCK_UNIT_FLAG;
        host.eligible[61] = true;
        host.prerequisites[62] = vec![gained_type, -1];
        host.has_prerequisite[62] = true;
        host.unit_flags[62] = TECH_AUTO_UNLOCK_UNIT_FLAG;
        host.object_masks[62] = TECH_AUTO_UNLOCK_EXCLUDED_OBJ_MASK;
        host.eligible[62] = true;

        assert_eq!(
            execute_gain_tech_auto_unlocks(gained_type, &mut host).unwrap(),
            TechAutoUnlockReceipt {
                gained_type,
                city_upgrade_checks: 1,
                recursive_buildings: 1,
                recursive_units: 1,
            }
        );
        assert_eq!(
            host.mutations,
            vec![
                TechAutoUnlockMutation::CheckCityUpgrades {
                    town_type: ty::TOWN,
                },
                TechAutoUnlockMutation::RecursivelyGain {
                    type_index: ty::UNIVERSITY,
                    class: TechAutoUnlockClass::Building,
                    coords: [0, 0],
                    tail: [0, 1],
                },
                TechAutoUnlockMutation::RecursivelyGain {
                    type_index: 60,
                    class: TechAutoUnlockClass::Unit,
                    coords: [0, 0],
                    tail: [0, 1],
                },
            ]
        );
    }

    #[test]
    fn gain_tech_auto_unlocks_fail_closed_on_callback_receipt_drift() {
        let gained_type = 600;
        let expected = TechAutoUnlockMutation::RecursivelyGain {
            type_index: ty::UNIVERSITY,
            class: TechAutoUnlockClass::Building,
            coords: [0, 0],
            tail: [0, 1],
        };
        let observed = TechAutoUnlockMutation::CheckCityUpgrades {
            town_type: ty::TOWN,
        };
        let mut host = AutoUnlockProbe {
            mutation_override: Some(observed),
            ..AutoUnlockProbe::default()
        };
        host.has_prerequisite[ty::UNIVERSITY as usize] = true;
        host.prerequisites[ty::UNIVERSITY as usize] = vec![gained_type];
        host.building_flags[ty::UNIVERSITY as usize] = TECH_AUTO_UNLOCK_BUILD_FLAG;
        host.eligible[ty::UNIVERSITY as usize] = true;

        assert_eq!(
            execute_gain_tech_auto_unlocks(gained_type, &mut host),
            Err(TechAutoUnlockError::MutationReceiptMismatch { expected, observed })
        );
        assert_eq!(host.mutations, vec![expected]);
    }

    #[test]
    fn gain_tech_cohort_orders_age_counter_resource_unlock_sell_and_tail() {
        let mut state = TechState::default();
        let before = state.counters;
        let after = TechCounters { ages: 1, ..before };
        let mut host = OneShotProbe::default();
        host.resource_preq[RES_TIMBER] = true;
        host.resource_unlock[RES_FOOD] = ty::CLASSICAL_AGE;
        host.resource_unlock[RES_TIMBER] = ty::CLASSICAL_AGE;
        host.resource_unlock_amount[RES_FOOD] = 5;
        host.resource_sell_tech[RES_METAL] = ty::CLASSICAL_AGE;
        host.resource_amounts[RES_METAL] = vec![250, 150, 100];

        let receipt = execute_gain_tech_cohort(
            &mut state,
            ty::CLASSICAL_AGE,
            GainTechCohortContext {
                grant_scale: TechResourceGrantScale::Multiply(2),
                unlimited_resources: true,
                classical_knowledge_bonus: Some(7),
                local_player: true,
                game_frame: 123,
                ..GainTechCohortContext::default()
            },
            &mut host,
        )
        .unwrap();
        assert_eq!(
            receipt,
            GainTechCohortReceipt {
                type_index: ty::CLASSICAL_AGE,
                was_new: true,
                counter_class: TechCounterClass::Age,
                resource_grants: 2,
                resource_sales: 2,
            }
        );
        assert!(state.tech.get(ty::CLASSICAL_AGE));
        assert_eq!(state.counters, after);
        assert_eq!(
            state.dirty_flags,
            TECH_GAIN_ENTER_DIRTY | TECH_GAIN_RESOURCES_DIRTY | TECH_EFFECTS_FINAL_DIRTY
        );

        let mutations: Vec<_> = host
            .events
            .iter()
            .filter_map(|event| match event {
                OneShotEvent::Mutation(mutation) => Some(*mutation),
                _ => None,
            })
            .collect();
        assert_eq!(
            mutations,
            vec![
                TechOneShotMutation::OrDirtyFlags(TECH_GAIN_ENTER_DIRTY),
                TechOneShotMutation::MarkGameTechDirty,
                TechOneShotMutation::CounterChanged {
                    type_index: ty::CLASSICAL_AGE,
                    before,
                    after,
                },
                TechOneShotMutation::RefreshAgeInterface,
                TechOneShotMutation::RecordAgeStamp {
                    type_index: ty::CLASSICAL_AGE,
                    slot: 0,
                    frame: 123,
                },
                TechOneShotMutation::CompleteGainPreBitEffects(ty::CLASSICAL_AGE),
                TechOneShotMutation::SetTechBit {
                    type_index: ty::CLASSICAL_AGE,
                    held: true,
                },
                TechOneShotMutation::CompleteGainAfterBitEffects(ty::CLASSICAL_AGE),
                TechOneShotMutation::OrDirtyFlags(TECH_GAIN_RESOURCES_DIRTY),
                TechOneShotMutation::GrantResource {
                    resource: RES_FOOD,
                    amount: 10,
                },
                TechOneShotMutation::SetResourceAmount {
                    resource: RES_FOOD,
                    amount: TECH_UNLIMITED_RESOURCE_AMOUNT,
                },
                TechOneShotMutation::SellResource {
                    resource: RES_METAL,
                    mode: 0,
                },
                TechOneShotMutation::SellResource {
                    resource: RES_METAL,
                    mode: 0,
                },
                TechOneShotMutation::GrantResource {
                    resource: RES_KNOWLEDGE,
                    amount: 14,
                },
                TechOneShotMutation::CompleteGainPostResourceEffects(ty::CLASSICAL_AGE),
                TechOneShotMutation::OrDirtyFlags(TECH_EFFECTS_FINAL_DIRTY),
                TechOneShotMutation::RefreshAgeConsumers(ty::CLASSICAL_AGE),
                TechOneShotMutation::TerrainOilGain(ty::CLASSICAL_AGE),
            ]
        );

        let bit_event = host
            .events
            .iter()
            .position(|event| {
                *event
                    == OneShotEvent::Mutation(TechOneShotMutation::SetTechBit {
                        type_index: ty::CLASSICAL_AGE,
                        held: true,
                    })
            })
            .unwrap();
        assert_eq!(
            &host.events[bit_event - NUM_RES..bit_event],
            &[
                OneShotEvent::ResourcePreq(RES_FOOD, false),
                OneShotEvent::ResourcePreq(RES_TIMBER, true),
                OneShotEvent::ResourcePreq(RES_WEALTH, false),
                OneShotEvent::ResourcePreq(RES_KNOWLEDGE, false),
                OneShotEvent::ResourcePreq(RES_METAL, false),
                OneShotEvent::ResourcePreq(RES_OIL, false),
            ]
        );
        assert_eq!(
            host.events
                .iter()
                .filter_map(|event| match event {
                    OneShotEvent::ResourceAmount(resource, amount) => Some((*resource, *amount)),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec![(RES_METAL, 250), (RES_METAL, 150), (RES_METAL, 100)]
        );
    }

    #[test]
    fn lose_tech_cohort_recomputes_epoch_category_before_world_tail() {
        let mut state = TechState::default();
        state.tech.set(551, true);
        state.tech.set(552, true);
        state.counters.epochs = 2;
        state.counters.epoch[ResearchCat::Science as usize] = 2;
        let before = state.counters;
        let after = TechCounters {
            epochs: 1,
            epoch: [0, 0, 0, 1],
            ..before
        };
        let mut host = OneShotProbe::default();

        assert_eq!(
            execute_lose_tech_cohort(&mut state, 551, &mut host).unwrap(),
            LoseTechCohortReceipt {
                type_index: 551,
                removed: true,
                counter_class: TechCounterClass::Epoch(ResearchCat::Science),
            }
        );
        assert!(!state.tech.get(551));
        assert!(state.tech.get(552));
        assert_eq!(state.counters, after);
        assert_eq!(
            host.events,
            vec![
                OneShotEvent::LoseIsResource(551, false),
                OneShotEvent::LoseIsRemovable(551, true),
                OneShotEvent::Mutation(TechOneShotMutation::SetTechBit {
                    type_index: 551,
                    held: false,
                }),
                OneShotEvent::Mutation(TechOneShotMutation::CompleteLoseDependentSweep(551)),
                OneShotEvent::Mutation(TechOneShotMutation::FixTechFlags),
                OneShotEvent::Mutation(TechOneShotMutation::CounterChanged {
                    type_index: 551,
                    before,
                    after,
                }),
                OneShotEvent::Mutation(TechOneShotMutation::TerrainOilLose(551)),
                OneShotEvent::Mutation(TechOneShotMutation::ResetObservationFlags),
                OneShotEvent::Mutation(
                    TechOneShotMutation::OrDirtyFlags(TECH_EFFECTS_FINAL_DIRTY,)
                ),
            ]
        );
    }

    #[test]
    fn gain_tech_cohort_fails_closed_on_receipt_and_non_decreasing_sell() {
        let mut state = TechState::default();
        let expected = TechOneShotMutation::OrDirtyFlags(TECH_GAIN_ENTER_DIRTY);
        let observed = TechOneShotMutation::ResetObservationFlags;
        let mut host = OneShotProbe {
            mutation_override: Some(observed),
            ..OneShotProbe::default()
        };
        assert_eq!(
            execute_gain_tech_cohort(&mut state, 600, GainTechCohortContext::default(), &mut host,),
            Err(TechOneShotError::MutationReceiptMismatch { expected, observed })
        );
        assert_eq!(host.events, vec![OneShotEvent::Mutation(expected)]);

        let mut state = TechState::default();
        let mut host = OneShotProbe::default();
        host.resource_sell_tech[RES_OIL] = 600;
        host.resource_amounts[RES_OIL] = vec![150, 150];
        assert_eq!(
            execute_gain_tech_cohort(&mut state, 600, GainTechCohortContext::default(), &mut host,),
            Err(TechOneShotError::SellDidNotReduceResource {
                resource: RES_OIL,
                before: 150,
                after: 150,
            })
        );
        assert!(!host.events.iter().any(|event| {
            matches!(
                event,
                OneShotEvent::Mutation(TechOneShotMutation::CompleteGainPostResourceEffects(_))
            )
        }));
    }

    #[test]
    fn techs_per_age_full_game_ladder() {
        // start 0, end > 6: age*4 + 2.
        let need: Vec<i32> = (0..7).map(|a| techs_per_age(a, 0, 7)).collect();
        assert_eq!(need, vec![2, 6, 10, 14, 18, 22, 26]);
    }

    #[test]
    fn techs_per_age_bounded_game() {
        // start 0, end 6: n = 7, 28/7 = 4 -> same ladder as the unbounded case.
        assert_eq!(techs_per_age(0, 0, 6), 2);
        assert_eq!(techs_per_age(6, 0, 6), 26);
        // start 2, end 6: n = 5, 28/5 = 5 -> (age-2+1)*5 - 2.
        assert_eq!(techs_per_age(2, 2, 6), 3);
        assert_eq!(techs_per_age(3, 2, 6), 8);
    }

    #[test]
    fn age_gate_uses_total_library_techs() {
        let mut s = TechState::default();
        // Medieval Age has AGE == 1 in techrules.xml -> 6 library techs.
        assert!(!s.age_requirement_met(1, 0, 7));
        for t in 551..557 {
            s.gain(t);
        }
        assert_eq!(s.counters.epochs, 6);
        assert!(s.age_requirement_met(1, 0, 7));
        assert!(!s.age_requirement_met(2, 0, 7));
    }

    #[test]
    fn bitmask_is_101_bytes_and_indexed_by_type_index() {
        let mut m = TechBitMask::default();
        assert_eq!(m.bytes.len(), 101);
        assert!(m.bytes.len() * 8 >= ty::NUM_TYPES);
        m.set(805, true);
        assert!(m.get(805));
        assert!(!m.get(804));
        m.set(805, false);
        assert!(!m.get(805));
        assert!(!m.get(-1));
    }

    #[test]
    fn has_tech_special_indices() {
        let s = TechState::default();
        assert!(s.has_tech(-1, false, || false), "-1 is 'no requirement'");
        assert!(!s.has_tech(-2, false, || true), "-2 is 'never'");
        assert!(
            s.has_tech(10, false, || false),
            "type indices below 50 are always held"
        );
        assert!(
            !s.has_tech(551, false, || true),
            "a tech index reads the bit"
        );
        assert!(
            s.has_tech(ty::MARKET, true, || true),
            "build types defer to has_preq"
        );
    }

    #[test]
    fn set_age_plan_targets_the_right_type_indices() {
        let (t, ranges) = set_age_plan(0);
        assert_eq!(t, ty::CLASSICAL_AGE);
        let (t, _) = set_age_plan(6);
        assert_eq!(t, ty::INFORMATION_AGE);
        let (t, _) = set_age_plan(99);
        assert_eq!(t, ty::CLASSICAL_AGE + 7);
        assert_eq!(ranges[0], (544, 629));
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum TechSetEvent {
        HasPreq(i32, bool),
        Gained(i32),
        Lost(i32),
        ResetObs,
        UnitStats,
        WallStats,
        Camera,
    }

    #[derive(Default)]
    struct TechSetProbe {
        invalid: Vec<i32>,
        /// `(type, dependency)`: `type` is valid only while `dependency` is held.
        dependencies: Vec<(i32, i32)>,
        events: Vec<TechSetEvent>,
    }

    impl TechSetHost for TechSetProbe {
        fn has_preq(&mut self, state: &TechState, type_index: i32) -> bool {
            let valid = !self.invalid.contains(&type_index)
                && self
                    .dependencies
                    .iter()
                    .filter(|(t, _)| *t == type_index)
                    .all(|(_, dependency)| state.tech.get(*dependency));
            self.events.push(TechSetEvent::HasPreq(type_index, valid));
            valid
        }

        fn gained_tech(&mut self, state: &TechState, type_index: i32) {
            assert!(
                state.tech.get(type_index),
                "gain callback must observe the committed bit"
            );
            self.events.push(TechSetEvent::Gained(type_index));
        }

        fn lost_tech(&mut self, state: &TechState, type_index: i32) {
            assert!(
                !state.tech.get(type_index),
                "loss callback must observe the cleared bit"
            );
            self.events.push(TechSetEvent::Lost(type_index));
        }

        fn reset_obs_flags(&mut self, _: &TechState) {
            self.events.push(TechSetEvent::ResetObs);
        }

        fn calc_unit_stats(&mut self, _: &TechState) {
            self.events.push(TechSetEvent::UnitStats);
        }

        fn calc_wall_stats(&mut self, _: &TechState) {
            self.events.push(TechSetEvent::WallStats);
        }

        fn outdate_camera(&mut self, _: &TechState) {
            self.events.push(TechSetEvent::Camera);
        }
    }

    #[test]
    fn set_age_executes_descending_loss_ascending_gain_and_exact_tail() {
        let mut state = TechState::default();
        for t in [545, 546, 548, 550] {
            state.gain(t);
        }
        let mut host = TechSetProbe::default();

        let receipt = state.execute_set_age(2, &mut host);

        assert_eq!(receipt.target, 546);
        assert_eq!(receipt.removed_from_ladder, vec![550, 548, 546]);
        assert_eq!(receipt.gained, vec![544]);
        assert!(receipt.invalidated.is_empty());
        assert!(state.tech.get(544));
        assert!(state.tech.get(545));
        assert!(!(546..=550).any(|t| state.tech.get(t)));
        assert_eq!(
            host.events,
            vec![
                TechSetEvent::Lost(550),
                TechSetEvent::Lost(548),
                TechSetEvent::Lost(546),
                TechSetEvent::Gained(544),
                TechSetEvent::ResetObs,
                TechSetEvent::UnitStats,
                TechSetEvent::WallStats,
                TechSetEvent::Camera,
            ]
        );
    }

    #[test]
    fn set_epoch_mutates_the_selected_counter_band_at_exclusive_level() {
        let mut state = TechState::default();
        let mut host = TechSetProbe::default();

        let receipt = state.execute_set_epoch(ResearchCat::Civic as i32, 3, &mut host);
        assert_eq!(receipt.target, 568);
        assert_eq!(receipt.gained, vec![565, 566, 567]);
        assert_eq!(state.counters.epochs, 3);
        assert_eq!(state.counters.epoch, [0, 3, 0, 0]);
        assert!((565..568).all(|t| state.tech.get(t)));
        assert!(!(568..572).any(|t| state.tech.get(t)));

        host.events.clear();
        let receipt = state.execute_set_epoch(ResearchCat::Civic as i32, 1, &mut host);
        assert_eq!(receipt.target, 566);
        assert_eq!(receipt.removed_from_ladder, vec![567, 566]);
        assert!(receipt.gained.is_empty());
        assert_eq!(state.counters.epochs, 1);
        assert_eq!(state.counters.epoch, [0, 1, 0, 0]);
        assert!(state.tech.get(565));
        assert!(!state.tech.get(566));
        assert!(!state.tech.get(567));
    }

    #[test]
    fn tech_revalidation_is_live_ordered_and_skips_age_types() {
        let mut state = TechState::default();
        for t in [551, 552, 50, 51, 414, 415] {
            state.gain(t);
        }
        let mut host = TechSetProbe {
            invalid: vec![551, 50, 414],
            dependencies: vec![(552, 551), (51, 50), (415, 414)],
            events: Vec::new(),
        };

        let receipt = state.execute_set_age(7, &mut host);

        assert_eq!(receipt.target, 551);
        assert_eq!(receipt.gained, (544..551).collect::<Vec<_>>());
        assert_eq!(receipt.invalidated, vec![551, 552, 50, 51, 414, 415]);
        assert_eq!(state.counters.epochs, 0);
        assert_eq!(state.counters.discovered, 0);
        assert!((544..551).all(|t| state.tech.get(t)));

        let queried = host
            .events
            .iter()
            .filter_map(|event| match event {
                TechSetEvent::HasPreq(t, _) => Some(*t),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(queried, vec![551, 552, 50, 51, 414, 415]);
        assert!(queried.iter().all(|t| !(544..=550).contains(t)));
        assert_eq!(
            &host.events[host.events.len() - 4..],
            &[
                TechSetEvent::ResetObs,
                TechSetEvent::UnitStats,
                TechSetEvent::WallStats,
                TechSetEvent::Camera,
            ]
        );
    }

    #[test]
    fn set_epoch_clamps_category_and_level_before_selecting_retail_base() {
        let mut military = TechState::default();
        let mut military_host = TechSetProbe::default();
        let receipt = military.execute_set_epoch(-99, 99, &mut military_host);
        assert_eq!(receipt.target, 579);
        assert_eq!(receipt.gained, (572..579).collect::<Vec<_>>());
        assert_eq!(military.counters.epoch, [7, 0, 0, 0]);

        let mut science = TechState::default();
        let mut science_host = TechSetProbe::default();
        let receipt = science.execute_set_epoch(99, -99, &mut science_host);
        assert_eq!(receipt.target, 551);
        assert!(receipt.gained.is_empty());
        assert!(science.tech.bytes.iter().all(|byte| *byte == 0));
    }

    // ---- techrules.xml -----------------------------------------------------------

    #[test]
    fn cost_tokenizer() {
        let c = parse_cost("25k/25f");
        assert_eq!(c[RES_KNOWLEDGE], 25);
        assert_eq!(c[RES_FOOD], 25);
        assert_eq!(c[RES_TIMBER], 0);
        let c = parse_cost("250k/100o");
        assert_eq!(c[RES_KNOWLEDGE], 250);
        assert_eq!(c[RES_OIL], 100);
        let c = parse_cost("120t");
        assert_eq!(c[RES_TIMBER], 120);
        let c = parse_cost("12g/12m");
        assert_eq!(c[RES_WEALTH], 12);
        assert_eq!(c[RES_METAL], 12);
    }

    fn techrules_path() -> std::path::PathBuf {
        // crates/don-sim/src/systems -> repo root
        let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("../../ron-data/techrules.xml");
        p
    }

    #[test]
    fn techrules_xml_loads_and_agrees_with_the_type_enum() {
        let path = techrules_path();
        let Ok(xml) = std::fs::read_to_string(&path) else {
            eprintln!(
                "skipping: {} not present (ron-data is gitignored)",
                path.display()
            );
            return;
        };
        let techs = parse_techrules(&xml);
        assert_eq!(techs.len(), 85, "techrules.xml TECH count");

        // The first seven entries are the ages, in TypeIndex order 544..550, with AGE 0..6.
        let ages: Vec<&TechRule> = techs.iter().take(7).collect();
        for (i, t) in ages.iter().enumerate() {
            assert_eq!(t.age, i as i32, "{} age", t.name);
            assert_eq!(t.where_name, "Library");
        }
        assert_eq!(ages[0].name, "Classical Age");
        assert_eq!(ages[6].name, "Information Age");
        // Classical Age: 25f, no prerequisite, 400 frames.
        assert_eq!(ages[0].costs[RES_FOOD], 25);
        assert_eq!(ages[0].preq[0], None);
        assert_eq!(ages[0].job_time, 400);
        // Medieval Age: 25k/25f, prereq Classical Age.
        assert_eq!(ages[1].costs[RES_KNOWLEDGE], 25);
        assert_eq!(ages[1].costs[RES_FOOD], 25);
        assert_eq!(ages[1].preq[0].as_deref(), Some("Classical Age"));

        // Every tech has a nonzero cost and a job time, and its prerequisites all name
        // other techs in this file or types outside it.
        for t in &techs {
            assert!(t.costs.iter().any(|&c| c > 0), "{} has no cost", t.name);
            assert!(t.job_time > 0, "{} has no job time", t.name);
            assert!(t.tribe_mask != 0, "{} is available to no tribe", t.name);
        }

        // The 39 Library techs are the ages plus the 28 epoch techs plus four extras.
        let library = techs.iter().filter(|t| t.where_name == "Library").count();
        assert_eq!(library, 39);
    }

    #[test]
    fn techrules_prereq_graph_is_acyclic_and_resolvable() {
        let path = techrules_path();
        let Ok(xml) = std::fs::read_to_string(&path) else {
            return;
        };
        let techs = parse_techrules(&xml);
        let index: std::collections::HashMap<&str, usize> = techs
            .iter()
            .enumerate()
            .map(|(i, t)| (t.name.as_str(), i))
            .collect();

        // Depth-first cycle check over the in-file edges.
        let mut state = vec![0u8; techs.len()]; // 0 unvisited, 1 on stack, 2 done
        fn visit(
            i: usize,
            techs: &[TechRule],
            index: &std::collections::HashMap<&str, usize>,
            state: &mut Vec<u8>,
        ) {
            if state[i] == 2 {
                return;
            }
            assert_ne!(state[i], 1, "cycle through {}", techs[i].name);
            state[i] = 1;
            for p in techs[i].preq.iter().flatten() {
                if let Some(&j) = index.get(p.as_str()) {
                    visit(j, techs, index, state);
                }
            }
            state[i] = 2;
        }
        for i in 0..techs.len() {
            visit(i, &techs, &index, &mut state);
        }

        // Every age's declared prerequisite is the previous age.
        for w in techs.iter().take(7).collect::<Vec<_>>().windows(2) {
            assert_eq!(w[1].preq[0].as_deref(), Some(w[0].name.as_str()));
        }
    }
}
