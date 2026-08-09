//! The economy tick: gather, caps, market, taxes, tribute, caravans.
//!
//! # What this is
//!
//! An executable port of the engine's per-leader economy, from `game/leaders.cpp`,
//! `game/gamedaemon.cpp` and `game/city.cpp`. It is the *tick*, not the constants: the
//! constants were already derived (`docs/derivation/economy.md`,
//! `docs/derivation/rules-constants.json`) and are consumed here through
//! [`EconRules`], which is the engine's own `Rules` value block addressed by **byte
//! offset**, so `crates/don-rules`'s `Rules::raw` drops straight in.
//!
//! The call chain this file implements, all names from the shipped PDB
//! (`ron-bin/sbl/rise.pdb`) — the previously-used names `Player::TickResource` /
//! `Player::UpdateCommerceCaps` were guesses and are wrong:
//!
//! ```text
//! Game::do_frame              0x00591EF0
//!  +- Leaders::process_all    0x006ED2A0   for each active leader, slot order
//!  |   +- Leader::gather              0x006CE280  -> [`leader_gather`]
//!  |       +- Leader::calc_gather     0x006CEEE0  -> [`calc_gather`]      <- gross income
//!  |       |   +- LeaderData::calc_rare            0x006E08D0 -> [`calc_rare`]
//!  |       |   +- LeaderData::calc_resource_bonuses 0x006DB030 -> [`calc_resource_bonuses`]
//!  |       |   +- UnitData::calc_gather            0x00609180 -> [`share_among_gatherers`]
//!  |       +- Leader::calc_resource_caps  0x006CE900  -> [`calc_resource_caps`]
//!  |       +- Leader::do_gather          0x006CE450  -> [`do_gather`]     <- the payout
//!  +- GameDaemon::process_all  0x00732700
//!      +- GameDaemon::calc_markets  0x00732180 -> [`calc_markets`]
//!          +- GameDaemon::calc_market 0x00732270 -> [`calc_market`]       <- draws RNG
//! ```
//!
//! Player-initiated: `Leader::do_buy` `0x006CFBD0` / `Leader::do_sell` `0x006CFC60`
//! ([`do_buy`], [`do_sell`]) over `LeaderData::calc_market_prices` `0x006DC2A0`
//! ([`calc_market_prices`]); `LeaderData::scale_tribute` `0x006D5240` ([`scale_tribute`]);
//! `LeaderData::get_caravan_limit` `0x006DCA50` ([`caravan_limit`]);
//! `CityData::get_taxes` `0x00737B50` ([`city_taxes`]).
//!
//! # Provenance and tier
//!
//! Every function carries the VA it was ported from. Structure came from
//! `re/decomp-all/<VA>.c`; every non-obvious control-flow decision and every constant was
//! re-read at the instruction level with capstone against
//! `ron-bin/riseofnations.exe` (sha256 `30478a44...625079`). That makes this **[measured]
//! structure, Tier C behaviour**: nothing here has been executed against retail. It is
//! *not* verified, *not* differentially tested, and no claim in this file may be promoted
//! without an oracle run. Where the object graph supplies a value we cannot resolve (a
//! tribe-bonus query, a spatial search, a wonder check), it is an **input**, exactly as
//! `mechanics.rs` does for the damage chain — never a guess.
//!
//! # Two things that will silently break a reimplementation
//!
//! 1. **The leader economy block is XOR-obfuscated per field.** `stockpile ^ 0x8221`,
//!    `accumulator ^ 0x3421`, `commerce cap ^ 0x1281`, `gross ^ 0x872`,
//!    `expense ^ 0x26076`, `displayed ^ 0x90236`, `capped flag ^ 0x8932`,
//!    `age ^ 0x63187`. The lockstep checksum runs over the **obfuscated** bytes, so
//!    [`LeaderEcon::image`] applies the masks and [`LeaderEcon::adler32`] hashes that.
//!    A checksum over plain values is wrong on frame 1.
//! 2. **`calc_market` draws from the shared simulation RNG** (`GameAccess::game_random`,
//!    `0x00E37A8C`) — up to three `Random::get(0, 0xFFFF)` calls per resource per market
//!    cycle. Get the *number* of draws wrong and every downstream consumer desyncs, not
//!    just the market.
//!
//! # Resource slot ordering — settled
//!
//! `docs/derivation/sim-economy.md` named only slots 0–3 and refused 4 and 5. All six are
//! now pinned [measured], by three independent witnesses:
//!
//! * the shipped `BASIC_GATHER` / `CITY_GATHER` entry strings, in slot order:
//!   `0food` `0timb` `0gd` `0know` `0met` `0oil`;
//! * `AMERICANS_BARRACKS_GATHER`, whose XML text is *"2 each of food, timber, metal, and
//!   gold"* and whose code at `0x006CF0E2`.. adds to slots **0, 1, 4, 2** in that order;
//! * `REFINERY_BONUS` scales slot **5** (`0x006CF4xx`) and `INCA_WEALTH_PER_MINER` moves
//!   slot **4** into slot **2** (`0x006CF4C6`) — oil and metal respectively.
//!
//! Confirmed a fourth time by `Leader::do_buy`, which pays from `econ + 8`, i.e. slot 2.

#![allow(clippy::needless_range_loop)]

use crate::rng::Random;

// ---------------------------------------------------------------------------------------
// Resource slots
// ---------------------------------------------------------------------------------------

/// Slot 0. `BASIC_GATHER` entry `"0food"`.
pub const RES_FOOD: usize = 0;
/// Slot 1. `BASIC_GATHER` entry `"0timb"`.
pub const RES_TIMBER: usize = 1;
/// Slot 2. `BASIC_GATHER` entry `"0gd"`; the slot `Leader::do_buy` pays from (`econ + 8`).
pub const RES_WEALTH: usize = 2;
/// Slot 3. `BASIC_GATHER` entry `"0know"`; the slot with the hardcoded 999 cap.
pub const RES_KNOWLEDGE: usize = 3;
/// Slot 4. `BASIC_GATHER` entry `"0met"`; the slot `INCA_WEALTH_PER_MINER` redirects.
pub const RES_METAL: usize = 4;
/// Slot 5. `BASIC_GATHER` entry `"0oil"`; the slot `REFINERY_BONUS` scales.
pub const RES_OIL: usize = 5;
/// The engine's resource count. Every economy loop in `leaders.cpp` is `for (i = 0; i < 6;)`.
pub const NUM_RESOURCES: usize = 6;

/// Slot names, in engine order.
pub const RES_NAMES: [&str; NUM_RESOURCES] =
    ["food", "timber", "wealth", "knowledge", "metal", "oil"];

// ---------------------------------------------------------------------------------------
// Integer helpers that mirror the machine code
// ---------------------------------------------------------------------------------------

/// `imul reg, imm; ...; sar edx, 5` — signed truncating divide by 100, the `0x51EB851F`
/// idiom that appears ~40 times in the economy code. Rust's `/` already truncates toward
/// zero on `i32`, so this exists to make the intent auditable at the call site.
///
/// `pub` because `systems::leaders` ports the same idiom out of `Leader::calc_attrition`
/// and `Game::retake_capital`; one definition beats two.
#[inline]
pub fn pct(v: i32, hundredths: i32) -> i32 {
    v.wrapping_mul(hundredths) / 100
}

/// `cdq; and edx, 0xFF; add eax, edx; sar eax, 8` — 8.8 unscale, truncating toward zero.
/// Used by the resource-substitution step at `0x006CF6A6`.
#[inline]
fn unscale_8_8(v: i32) -> i32 {
    let bias = if v < 0 { 0xFF } else { 0 };
    v.wrapping_add(bias) >> 8
}

/// `cdq; sub eax, edx; sar eax, 1` — halve, truncating toward zero (`0x00732292`).
#[inline]
fn half_toward_zero(v: i32) -> i32 {
    let s = v >> 31;
    (v.wrapping_sub(s)) >> 1
}

// ---------------------------------------------------------------------------------------
// The rules block
// ---------------------------------------------------------------------------------------

/// Number of `i32` slots in the engine's `Rules` value block. `RULES + 0xD40` is the XML
/// section object, so `0xD40 / 4` bounds the block.
pub const RULES_DWORDS: usize = 848;

/// The engine's `Rules` value block, addressed by **byte offset** exactly as a disassembly
/// listing names it.
///
/// `crates/don-rules` owns the derivation of this block (719 constants / 845 slots,
/// live-validated against a running match, scale set `{192, 256, 100}` proven exhaustive).
/// This type deliberately does *not* import it — `don-sim` stays dependency-free, the same
/// choice `mechanics.rs` made for `CombatRules` — but the layout is identical, so
/// `EconRules::from_block(&don_rules::Rules::shipped().raw)` is the wiring.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EconRules {
    raw: [i32; RULES_DWORDS],
}

impl Default for EconRules {
    fn default() -> Self {
        EconRules::shipped()
    }
}

/// One shipped constant: byte offset, name, and the integer the engine stores.
///
/// Extracted from `docs/derivation/rules-constants.json`, which records the loader call
/// site, the parse mode and the `rules.xml` text for each. Only the economy-relevant
/// subset is carried here; the full 845-slot table lives in `crates/don-rules`.
pub const SHIPPED_ECONOMY_SLOTS: &[(usize, &str, i32)] = &[
    (588, "basic_gather[0] food", 0),
    (592, "basic_gather[1] timber", 0),
    (596, "basic_gather[2] wealth", 0),
    (600, "basic_gather[3] knowledge", 0),
    (604, "basic_gather[4] metal", 0),
    (608, "basic_gather[5] oil", 0),
    (612, "city_gather[0]", 10),
    (616, "city_gather[1]", 10),
    (620, "city_gather[2]", 0),
    (624, "city_gather[3]", 0),
    (628, "city_gather[4]", 0),
    (632, "city_gather[5]", 0),
    (636, "gather_rate", 450),
    (640, "peasant_rate", 2560),
    (644, "scholar_rate[0]", 5),
    (648, "scholar_rate[1]", 7),
    (652, "scholar_rate[2]", 10),
    (656, "scholar_rate[3]", 15),
    (660, "scholar_rate[4]", 20),
    (668, "oil_rate", 8960),
    (1160, "forbidden_city_gather", 25),
    (1164, "forbidden_city_base_gather", 50),
    (1568, "roman_city_gather", 10),
    (1864, "german_city_gather", 5),
    (3232, "theceo_production_bonus", 50),
    (680, "fishermen_bonus[0]", 0),
    (684, "fishermen_bonus[1]", 50),
    (688, "fishermen_bonus[2]", 100),
    (692, "fishermen_bonus[3]", 200),
    (696, "fishermen_bonus[4]", 200),
    (760, "refinery_bonus", 33),
    (764, "base_tribute", 51),
    (768, "commerce_tribute", 7),
    (784, "merchants_bonus[0]", 100),
    (788, "merchants_bonus[1]", 120),
    (792, "merchants_bonus[2]", 150),
    (796, "merchants_bonus[3]", 200),
    (800, "merchants_bonus[4]", 300),
    (804, "village_taxes", 0),
    (808, "building_taxes", 0),
    (812, "market_taxes", 10),
    (816, "temple_taxes", 0),
    (820, "territory_taxes[0]", 0),
    (824, "territory_taxes[1]", 50),
    (828, "territory_taxes[2]", 100),
    (832, "territory_taxes[3]", 200),
    (836, "territory_taxes[4]", 300),
    (1024, "commerce_cap[0]", 70),
    (1028, "commerce_cap[1]", 100),
    (1032, "commerce_cap[2]", 150),
    (1036, "commerce_cap[3]", 200),
    (1040, "commerce_cap[4]", 260),
    (1044, "commerce_cap[5]", 320),
    (1048, "commerce_cap[6]", 400),
    (1052, "commerce_cap[7]", 500),
    (1060, "global_prosperity", 25),
    (1072, "pyramids_food", 20),
    (1080, "colossus_wealth", 30),
    (1088, "colossus_caravan", 0),
    (1096, "hanging_gardens_knowledge", 50),
    (1168, "tikal_timber", 50),
    (1200, "porcelain_rare", 200),
    (1204, "porcelain_market", 300),
    (1232, "angkor_metal", 50),
    (1276, "taj_wealth", 100),
    (1344, "eiffel_oil", 100),
    (1356, "super_buy", 125),
    (1360, "super_sell", 50),
    (1432, "inca_wealth_cap", 33),
    (1436, "inca_wealth_per_miner", 10),
    (1468, "nubian_rare", 50),
    (1472, "nubian_caravan_limit", 1),
    (1488, "nubian_market_prices", 20),
    (1620, "egyptian_food_commerce", 10),
    (1740, "french_timber_commerce", 10),
    (1744, "british_commerce", 25),
    (1772, "british_taxation", 100),
    (1876, "russian_oil", 20),
    (1880, "russian_communism", 0),
    (1964, "japanese_fishing_boats", 25),
    (2044, "mongol_nomadic_food", 1),
    (2120, "lakota_food", 4),
    (2184, "americans_barracks_gather", 2),
    (2216, "dutch_interest", 5),
    (2220, "dutch_interest_cap", 50),
    (2284, "silk_caravan", 0),
    (2308, "amber_market", 10),
    (2428, "coffee_income_bonus", 10),
    (2496, "capitalism_oil_prod", 100),
    (2788, "ctw_market_bonus_buy", 25),
    (2792, "ctw_market_bonus_sell", 20),
    (2800, "ctw_prod_rate_bonus", 5),
    (2824, "ctw_missionaries_bonus", 25),
    (3288, "market_basement", 10),
    (3292, "market_equilibrium", 65),
    (3296, "market_min_variance", 2),
    (3300, "market_min_trend", 8),
    (3304, "market_trend_range", 16),
    (3312, "market_supply_demand", 3),
    // MARKET_CYCLE_RATE is listed separately: see `EconRules::market_cycle_rate`.
    (3308, "market_cycle_rate", 1),
];

macro_rules! rule {
    ($(#[$m:meta])* $name:ident, $off:expr) => {
        $(#[$m])*
        #[inline]
        pub fn $name(&self) -> i32 { self.raw[$off / 4] }
    };
}

macro_rules! rule_arr {
    ($(#[$m:meta])* $name:ident, $off:expr, $n:expr) => {
        $(#[$m])*
        ///
        /// Index is clamped to the array's own length, as the engine's callers already
        /// guarantee by construction; an out-of-range index here is a port bug, not a
        /// game state, so it saturates rather than reading a neighbouring constant.
        #[inline]
        pub fn $name(&self, i: usize) -> i32 {
            self.raw[$off / 4 + i.min($n - 1)]
        }
    };
}

impl EconRules {
    /// A block of all zeros. Not a state the engine can be in — several fields are
    /// divisors — so prefer [`EconRules::shipped`] unless you are deliberately testing.
    pub const fn zeroed() -> EconRules {
        EconRules {
            raw: [0; RULES_DWORDS],
        }
    }

    /// The economy-relevant subset of `rules.xml` as shipped, from
    /// [`SHIPPED_ECONOMY_SLOTS`]. Every other slot is zero; use [`EconRules::from_block`]
    /// with `crates/don-rules`'s full table when the whole block matters.
    pub fn shipped() -> EconRules {
        let mut r = EconRules::zeroed();
        let mut i = 0;
        while i < SHIPPED_ECONOMY_SLOTS.len() {
            let (off, _, v) = SHIPPED_ECONOMY_SLOTS[i];
            r.raw[off / 4] = v;
            i += 1;
        }
        r
    }

    /// Load from a full `Rules` value block — `don_rules::Rules::raw`, or a live capture
    /// of `[[0x00C061E4]]`. Shorter slices are accepted and zero-extended.
    pub fn from_block(block: &[i32]) -> EconRules {
        let mut r = EconRules::zeroed();
        let n = block.len().min(RULES_DWORDS);
        r.raw[..n].copy_from_slice(&block[..n]);
        r
    }

    /// Read a slot by **byte** offset, the way a disassembly listing names it.
    #[inline]
    pub fn at(&self, byte_offset: usize) -> i32 {
        self.raw[byte_offset / 4]
    }

    /// Write a slot by byte offset. For tests and for modelling a mod.
    #[inline]
    pub fn set(&mut self, byte_offset: usize, v: i32) {
        self.raw[byte_offset / 4] = v;
    }

    rule_arr!(
        /// `BASIC_GATHER[6]` — the flat baseline income every leader gets, before any
        /// worker. Written as `out[i] = BASIC_GATHER[i] << 4` at `0x006CF0A6`. Shipped 0.
        basic_gather, 588, 6
    );
    rule_arr!(
        /// `CITY_GATHER[6]` — per-city baseline, consumed by `LeaderData::calc_city_resources`.
        city_gather, 612, 6
    );
    rule!(
        /// `GATHER_RATE` = `"450 frames"`. The accumulator period is `GATHER_RATE * 16`.
        gather_rate, 636
    );
    rule!(
        /// `PEASANT_RATE` = `"10 resources"` at scale 256, so **2560 = 10.0 in 8.8**.
        /// See [`worker_rate`].
        peasant_rate, 640
    );
    rule_arr!(
        /// `SCHOLAR_RATE[5]` = `5, 7, 10, 15, 20`, indexed **`level - 1`**
        /// (`0x006D5754`: `RULES[0x284 + (level-1)*4]`). See [`scholar_rate_for_level`].
        scholar_rate, 644, 5
    );
    rule!(
        /// `OIL_RATE` = `"35 oil"` at scale 256, so **8960 = 35.0 in 8.8**.
        oil_rate, 668
    );
    rule_arr!(
        /// `FISHERMEN_BONUS[5]` — indexed by the fishing upgrade level in
        /// `LeaderData::calc_rare`, and applied **only** to good types 6 and 0x1F.
        fishermen_bonus, 680, 5
    );
    rule!(
        /// `REFINERY_BONUS` = `"33% per refinery"`. Multiplies **oil** income by
        /// `(refineries * 33 + 100) / 100` at `0x006CF48C`.
        refinery_bonus, 760
    );
    rule!(
        /// `BASE_TRIBUTE` = `"51% of gift gets through"`.
        base_tribute, 764
    );
    rule!(
        /// `COMMERCE_TRIBUTE` = `"7% additional gift gets through"`, per age.
        commerce_tribute, 768
    );
    rule_arr!(
        /// `MERCHANTS_BONUS[5]` — indexed by `LeaderData::get_merchants_level`
        /// (`0x006D6DC0`); it *replaces* the 100% baseline for a rare's yield, it does not
        /// add to it.
        merchants_bonus, 784, 5
    );
    rule!(
        /// `VILLAGE_TAXES` — flat wealth per city.
        village_taxes, 804
    );
    rule!(
        /// `BUILDING_TAXES` — wealth per building in the city.
        building_taxes, 808
    );
    rule!(
        /// `MARKET_TAXES` = 10 wealth if the city has a market.
        market_taxes, 812
    );
    rule!(
        /// `TEMPLE_TAXES` — wealth if the city has a temple.
        temple_taxes, 816
    );
    rule_arr!(
        /// `TERRITORY_TAXES[5]` = `0 / 50 / 100 / 200 / 300` percent, indexed by
        /// [`taxation_level`].
        territory_taxes, 820, 5
    );
    rule_arr!(
        /// `COMMERCE_CAP[8]` = `70,100,150,200,260,320,400,500`, indexed by **age**.
        commerce_cap, 1024, 8
    );
    rule!(
        /// `GLOBAL_PROSPERITY` — scales every resource except knowledge.
        global_prosperity, 1060
    );
    rule!(/// `PYRAMIDS_FOOD` — +20% food.
        pyramids_food, 1072);
    rule!(/// `COLOSSUS_WEALTH` — +30% wealth.
        colossus_wealth, 1080);
    rule!(/// `COLOSSUS_CARAVAN` — caravan-limit bonus.
        colossus_caravan, 1088);
    rule!(/// `HANGING_GARDENS_KNOWLEDGE` — flat +50 knowledge income (`<< 4`).
        hanging_gardens_knowledge, 1096);
    rule!(/// `TIKAL_TIMBER` — +50% timber.
        tikal_timber, 1168);
    rule!(
        /// `FORBIDDEN_CITY_GATHER` = `"25% bonus"` — scales **every** slot of a city whose
        /// head building is type `0x213`.
        forbidden_city_gather, 1160
    );
    rule!(
        /// `FORBIDDEN_CITY_BASE_GATHER` = `"50 resources"` — *replaces* `CITY_GATHER[i]`
        /// for that city, per slot, rather than adding to it (`0x006D57xx`).
        forbidden_city_base_gather, 1164
    );
    rule!(/// `ROMAN_CITY_GATHER` = 10 wealth per city.
        roman_city_gather, 1568);
    rule!(
        /// `GERMAN_CITY_GATHER` = 5, applied to food, timber and (if available) metal.
        german_city_gather, 1864
    );
    rule!(
        /// `THECEO_PRODUCTION_BONUS` = `"50% better in one city"` — the campaign hero's
        /// city bonus, applied to every slot of the one city he stands in.
        theceo_production_bonus, 3232
    );
    rule!(/// `PORCELAIN_RARE` — +200% on rares in own territory.
        porcelain_rare, 1200);
    rule!(/// `PORCELAIN_MARKET` — scales `MARKET_TAXES` in `CityData::get_taxes`.
        porcelain_market, 1204);
    rule!(/// `ANGKOR_METAL` — +50% metal.
        angkor_metal, 1232);
    rule!(/// `TAJ_WEALTH` — +100% wealth.
        taj_wealth, 1276);
    rule!(/// `EIFFEL_OIL` — +100% oil.
        eiffel_oil, 1344);
    rule!(
        /// `SUPER_BUY` = `"125 is maximum buy price"`. Only applied under one wonder.
        super_buy, 1356
    );
    rule!(
        /// `SUPER_SELL` = `"50 is minimum sell price"`.
        super_sell, 1360
    );
    rule!(/// `INCA_WEALTH_CAP` — +33% to the wealth commerce cap.
        inca_wealth_cap, 1432);
    rule!(
        /// `INCA_WEALTH_PER_MINER`. Note the gate at `0x006CF4B9` is
        /// `if (INCA_WEALTH_PER_MINER < 0)`, and the shipped value is `10`, so the
        /// `wealth += metal` step is **off in shipped data**.
        inca_wealth_per_miner, 1436
    );
    rule!(/// `NUBIAN_RARE` — +50% on rares.
        nubian_rare, 1468);
    rule!(/// `NUBIAN_CARAVAN_LIMIT` — +1 caravan.
        nubian_caravan_limit, 1472);
    rule!(/// `NUBIAN_MARKET_PRICES` — better buy *and* sell prices.
        nubian_market_prices, 1488);
    rule!(/// `RUSSIAN_OIL` — +20% oil.
        russian_oil, 1876);
    rule!(
        /// `RUSSIAN_COMMUNISM`. Shipped **0**, which disables the fixed-100 market prices
        /// at `0x006DC3E8` entirely — the branch is `if (rule != 0) { ... }`.
        russian_communism, 1880
    );
    rule!(/// `JAPANESE_FISHING_BOATS` — +25% on fish rares.
        japanese_fishing_boats, 1964);
    rule!(
        /// `MONGOL_NOMADIC_FOOD` = 1. A **divisor** in the territory-food term at
        /// `0x006CF55E`, guarded `!= 0` at the call site.
        mongol_nomadic_food, 2044
    );
    rule!(/// `LAKOTA_FOOD` = 4 food per qualifying unit.
        lakota_food, 2120);
    rule!(
        /// `AMERICANS_BARRACKS_GATHER` = `"2 each of food, timber, metal, and gold"` —
        /// the XML text that pins slots 0, 1, 4, 2.
        americans_barracks_gather, 2184
    );
    rule!(/// `DUTCH_INTEREST` = 5%.
        dutch_interest, 2216);
    rule!(/// `DUTCH_INTEREST_CAP` = `"50 over econ cap"`.
        dutch_interest_cap, 2220);
    rule!(/// `SILK_CARAVAN` — caravan-limit bonus from the silk rare.
        silk_caravan, 2284);
    rule!(/// `AMBER_MARKET` — better prices from the amber rare.
        amber_market, 2308);
    rule!(/// `COFFEE_INCOME_BONUS` = 10% on **every** resource.
        coffee_income_bonus, 2428);
    rule!(/// `CAPITALISM_OIL_PROD` = flat +100 oil income (`<< 4`).
        capitalism_oil_prod, 2496);
    rule!(/// `CTW_MARKET_BONUS_BUY`, Conquer-the-World only.
        ctw_market_bonus_buy, 2788);
    rule!(/// `CTW_MARKET_BONUS_SELL`, Conquer-the-World only.
        ctw_market_bonus_sell, 2792);
    rule!(/// `CTW_PROD_RATE_BONUS`, Conquer-the-World only.
        ctw_prod_rate_bonus, 2800);
    rule!(/// `CTW_MISSIONARIES_BONUS`, Conquer-the-World only.
        ctw_missionaries_bonus, 2824);
    rule!(
        /// `MARKET_BASEMENT` = 10. A floor on the base price, and `2 *` it floors the buy
        /// price.
        market_basement, 3288
    );
    rule!(
        /// `MARKET_EQUILIBRIUM` = 65. The base price walks toward this every 256 cycles.
        market_equilibrium, 3292
    );
    rule!(/// `MARKET_MIN_VARIANCE` = 2.
        market_min_variance, 3296);
    rule!(/// `MARKET_MIN_TREND` = `"8 cycles"`.
        market_min_trend, 3300);
    rule!(/// `MARKET_TREND_RANGE` = `"16 cycles"`.
        market_trend_range, 3304);
    rule!(
        /// `MARKET_CYCLE_RATE` = `"1 frames"`. `<= 1` means **every frame**
        /// (`cmp esi,1; jle` at `0x0073219E`).
        market_cycle_rate, 3308
    );
    rule!(
        /// `MARKET_SUPPLY_DEMAND` = `"3 +/- to sell price (double to buy price)"` — the
        /// base-price kick each 100-unit trade applies.
        market_supply_demand, 3312
    );
    rule!(/// `BRITISH_COMMERCE` — +25% to every commerce cap.
        british_commerce, 1744);
    rule!(/// `BRITISH_TAXATION` — +100% to the territory tax rate.
        british_taxation, 1772);
    rule!(/// `FRENCH_TIMBER_COMMERCE` — +10% timber cap.
        french_timber_commerce, 1740);
    rule!(/// `EGYPTIAN_FOOD_COMMERCE` — +10% food cap.
        egyptian_food_commerce, 1620);
}

// ---------------------------------------------------------------------------------------
// The leader economy block
// ---------------------------------------------------------------------------------------

/// XOR masks the engine applies to the fields of the economy block. Anti-cheat, and
/// **load-bearing for the checksum**: `CheckSums::check_all` hashes these bytes as stored.
pub mod obfuscation {
    /// `econ + 0x00 + res*4` — stockpile. `0x006CFC0A`, `0x006CFCB0`.
    pub const STOCKPILE: u32 = 0x8221;
    /// `econ + 0x18 + res*4` — fractional accumulator. `0x006CE7DE`.
    pub const ACCUMULATOR: u32 = 0x3421;
    /// `econ + 0x30 + res*4` — commerce cap. `0x006CE95C`.
    pub const COMMERCE_CAP: u32 = 0x1281;
    /// `econ + 0x4C + res*4` — "income is capped" flag (0, 1 or 2). `0x006CE60A`.
    pub const CAPPED_FLAG: u32 = 0x8932;
    /// `econ + 0x64 + res*4` — gross income for the tick. `0x006CE4C8`.
    pub const GROSS: u32 = 0x872;
    /// `econ + 0x7C + res*4` — expenses for the tick. `0x006CE4D5`.
    pub const EXPENSE: u32 = 0x26076;
    /// `econ + 0x94 + res*4` — displayed income. `0x006CE723`.
    pub const DISPLAYED: u32 = 0x90236;
    /// `econ + 0xC4 + res*4` — per-resource breakdown, reset each `calc_gather`.
    pub const BREAKDOWN: u32 = 0x6722;
    /// `econ + 0xF0` — the leader's age, as `calc_resource_caps` reads it. `0x006CE90D`.
    pub const AGE: u32 = 0x63187;
    /// `econ + 0xDC` — the second age field, as `calc_market_prices` reads it. `0x006DC44F`.
    pub const AGE_ALT: u32 = 0x62766;
}

/// Byte offsets inside the economy block (`*(Leader + 0x6EB8)`), recovered from the read
/// and write sites in `Leader::do_gather`, `Leader::calc_resource_caps`, `Leader::do_buy`,
/// `Leader::do_sell` and `LeaderData::calc_market_prices` [measured].
pub mod econ_offsets {
    /// 6 dwords. Stockpile.
    pub const STOCKPILE: usize = 0x00;
    /// 6 dwords. Fractional carry toward the next whole resource.
    pub const ACCUMULATOR: usize = 0x18;
    /// 6 dwords. Commerce cap.
    pub const COMMERCE_CAP: usize = 0x30;
    /// 6 dwords. Capped-this-tick flag.
    pub const CAPPED_FLAG: usize = 0x4C;
    /// 6 dwords. Gross income.
    pub const GROSS: usize = 0x64;
    /// 6 dwords. Expense.
    pub const EXPENSE: usize = 0x7C;
    /// 6 dwords. Displayed (post-cap, pre-handicap) income.
    pub const DISPLAYED: usize = 0x94;
    /// 6 dwords. Per-source breakdown, cleared each `calc_gather`.
    pub const BREAKDOWN: usize = 0xC4;
    /// 1 dword. Alternate age field.
    pub const AGE_ALT: usize = 0xDC;
    /// 1 dword. Age.
    pub const AGE: usize = 0xF0;
    /// The size of the region we model and checksum: `[0x00, 0xF4)`.
    pub const MODELLED_LEN: usize = 0xF4;
}

/// The per-leader economy block, in plain integers.
///
/// Held unobfuscated for arithmetic; [`LeaderEcon::image`] applies the engine's XOR masks
/// to produce the byte image the lockstep checksum actually runs over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LeaderEcon {
    /// `econ + 0x00`. Whole resources held.
    pub stockpile: [i32; NUM_RESOURCES],
    /// `econ + 0x18`. Fractional carry, in the same 1/16-per-`GATHER_RATE` units as income.
    pub accumulator: [i32; NUM_RESOURCES],
    /// `econ + 0x30`. Per-tick income ceiling.
    pub commerce_cap: [i32; NUM_RESOURCES],
    /// `econ + 0x4C`. 0 = not capped, 1 = capped, 2 = capped at a cap above 15,983.
    pub capped_flag: [i32; NUM_RESOURCES],
    /// `econ + 0x64`. Gross income, written by [`calc_gather`].
    pub gross: [i32; NUM_RESOURCES],
    /// `econ + 0x7C`. Expenses, zeroed at the top of [`leader_gather`].
    pub expense: [i32; NUM_RESOURCES],
    /// `econ + 0x94`. The number the HUD shows.
    pub displayed: [i32; NUM_RESOURCES],
    /// `econ + 0xC4`. Per-source breakdown; cleared to 0 by `calc_gather`.
    pub breakdown: [i32; NUM_RESOURCES],
    /// `econ + 0xDC`.
    pub age_alt: i32,
    /// `econ + 0xF0`. Drives the commerce cap and the tribute scale.
    pub age: i32,
}

impl Default for LeaderEcon {
    fn default() -> Self {
        LeaderEcon::new()
    }
}

impl LeaderEcon {
    /// A zeroed block at age 0.
    pub const fn new() -> LeaderEcon {
        LeaderEcon {
            stockpile: [0; NUM_RESOURCES],
            accumulator: [0; NUM_RESOURCES],
            commerce_cap: [0; NUM_RESOURCES],
            capped_flag: [0; NUM_RESOURCES],
            gross: [0; NUM_RESOURCES],
            expense: [0; NUM_RESOURCES],
            displayed: [0; NUM_RESOURCES],
            breakdown: [0; NUM_RESOURCES],
            age_alt: 0,
            age: 0,
        }
    }

    /// The block as the engine stores it: little-endian dwords, each XORed with its own
    /// mask. This is what `CheckSums::check_all` hashes for the **leaders** channel.
    ///
    /// Unmodelled dwords inside `[0, 0xF4)` are emitted as zero. That is a *known* and
    /// stated divergence from retail — the block has slots at `0x48`, `0xAC..0xC4` and
    /// `0xE0..0xF0` we have not identified — so the resulting checksum is comparable
    /// between two runs of *this* implementation, and is **not yet** comparable against
    /// the engine's. Closing that gap needs a live read of one econ block; see the report.
    pub fn image(&self) -> [u8; econ_offsets::MODELLED_LEN] {
        let mut out = [0u8; econ_offsets::MODELLED_LEN];
        let mut put = |off: usize, v: i32, mask: u32| {
            let w = (v as u32) ^ mask;
            out[off..off + 4].copy_from_slice(&w.to_le_bytes());
        };
        for r in 0..NUM_RESOURCES {
            put(
                econ_offsets::STOCKPILE + r * 4,
                self.stockpile[r],
                obfuscation::STOCKPILE,
            );
            put(
                econ_offsets::ACCUMULATOR + r * 4,
                self.accumulator[r],
                obfuscation::ACCUMULATOR,
            );
            put(
                econ_offsets::COMMERCE_CAP + r * 4,
                self.commerce_cap[r],
                obfuscation::COMMERCE_CAP,
            );
            put(
                econ_offsets::CAPPED_FLAG + r * 4,
                self.capped_flag[r],
                obfuscation::CAPPED_FLAG,
            );
            put(
                econ_offsets::GROSS + r * 4,
                self.gross[r],
                obfuscation::GROSS,
            );
            put(
                econ_offsets::EXPENSE + r * 4,
                self.expense[r],
                obfuscation::EXPENSE,
            );
            put(
                econ_offsets::DISPLAYED + r * 4,
                self.displayed[r],
                obfuscation::DISPLAYED,
            );
            put(
                econ_offsets::BREAKDOWN + r * 4,
                self.breakdown[r],
                obfuscation::BREAKDOWN,
            );
        }
        put(econ_offsets::AGE_ALT, self.age_alt, obfuscation::AGE_ALT);
        put(econ_offsets::AGE, self.age, obfuscation::AGE);
        out
    }

    /// adler-32 of [`LeaderEcon::image`], the way a channel of `CheckSums::check_all`
    /// accumulates. See [`adler32`].
    pub fn adler32(&self) -> u32 {
        adler32(1, &self.image())
    }
}

/// zlib `adler32`, the lockstep checksum primitive (`0x00A46830`, `__fastcall`).
///
/// Differentially tested against retail by the checksum lane — 500,000 calls, 0
/// mismatches (`docs/derivation/checksum.md` §2). That shared checksum module now exists,
/// so this defers to it exactly as this comment used to ask for.
pub use crate::checksum::adler32;

// ---------------------------------------------------------------------------------------
// Leader::calc_gather  0x006CEEE0  -- the gross-income composition
// ---------------------------------------------------------------------------------------

/// The scheduling predicate at the head of `Leader::calc_gather` (`0x006CEEE0`).
///
/// This is the "resource tick period" the task asked for, and it is **not** the accumulator
/// period. Recomputing gross income is expensive (it walks every city, building, gatherer
/// and rare), so the engine does it rarely and staggers it across players; the *payout*
/// then runs every frame off the cached gross.
///
/// ```text
/// 006cef07  test dword [leader], 0x2000000     ; the "economy is dirty" bit
///           dirty:   frame != 0 && (leader_slot + frame) % 8 == 0
///           clean:   frame >= last_frame + 300 && (frame + leader_slot*8) % 256 == 0
/// ```
///
/// Both moduli are taken on the **low byte** (`and 0x800000FF` / `and 0x80000007` with the
/// negative fixup), i.e. signed remainder; `frame` is non-negative in practice.
///
/// So: **every 256 frames per leader normally, every 8 frames while dirty**, with a
/// 300-frame floor on the clean path. At 15 frames/second that is one full recomputation
/// per leader per ~17 s, or ~0.53 s while dirty.
#[inline]
pub fn calc_gather_due(frame: i32, leader_slot: i32, last_calc_frame: i32, dirty: bool) -> bool {
    if dirty {
        // 0x006CEF3D: `if (frame != 0)`, then `(slot + frame) % 8 == 0`.
        frame != 0 && (leader_slot.wrapping_add(frame)) % 8 == 0
    } else {
        // 0x006CEEFB / 0x006CEF12.
        if frame < last_calc_frame.wrapping_add(300) {
            return false;
        }
        (frame.wrapping_add(leader_slot.wrapping_mul(8))) % 256 == 0
    }
}

/// Number of distinct rare-resource good types the engine tracks per leader.
///
/// `Leader::calc_gather` zeroes a 44-entry counter array at `leader + 0x6D8` and iterates
/// bits `0..0x2C` of the rare bitmask at `leader + 0x6DCC`; the corresponding container
/// type in the PDB is literally `BitMask<44>`.
pub const NUM_RARES: usize = 44;

/// The good-type index of the first rare. `Leader::calc_gather` calls
/// `LeaderData::calc_rare(bit + 6, ...)` at `0x006CF521`, and `calc_rare`'s own fish
/// special-case tests `index == 6`, so bit 0 is good type 6.
pub const FIRST_RARE_GOOD_TYPE: i32 = 6;

/// A `GoodType`'s payout: **two** `(resource, amount)` pairs, at `GoodType + 0x2DC`/`+0x2E0`
/// (resource ids) and `+0x2E4`/`+0x2E8` (amounts) [measured, the loop bounds `0x2E4..0x2EC`
/// at `0x006E0913`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct GoodTypeYield {
    /// Resource slot for each of the two payout terms; a value `>= 6` disables that term
    /// (the engine's guard is an **unsigned** `< 6`, so negatives are disabled too).
    pub res_id: [i32; 2],
    /// Amount for each term, in whole resources per `GATHER_RATE`. Multiplied by 16 on the
    /// way in.
    pub amount: [i32; 2],
}

/// Everything `LeaderData::calc_rare` gets by querying the leader's tech/tribe/wonder state.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RareContext {
    /// `LeaderData::get_merchants_level` `0x006D6DC0`, indexing `MERCHANTS_BONUS[5]`.
    pub merchants_level: usize,
    /// 3 / 2 / 1 / 0 from `has_preq(0x2C1) / (0x2C0) / (0x2BF)`, indexing
    /// `FISHERMEN_BONUS[5]`. Only consulted for good types 6 and 0x1F.
    pub fishing_level: usize,
    /// `has_tribe_bonus(0x0F)` — the Japanese fishing bonus.
    pub japanese: bool,
    /// `has_tribe_bonus(0x04)` — the Nubian rare bonus.
    pub nubian: bool,
    /// `has_wonder(0x215)` — the Porcelain Tower.
    pub porcelain: bool,
}

/// `LeaderData::calc_rare` `0x006E08D0` — the yield of one resource node for one leader.
///
/// ```text
/// pct = MERCHANTS_BONUS[get_merchants_level()]
/// mult[0..6] = 100
/// for k in 0..2:
///     rid = good.res_id[k]
///     if (unsigned)rid < 6:
///         out[rid] += good.amount[k] * 16
///         if own_territory || ((good_type == 6 || good_type == 0x1F) && rid != 0):
///             mult[rid] = pct
/// ```
///
/// Note what that gate means: the merchant multiplier **replaces** 100%, it does not stack
/// with it, and for a fish node it applies to every slot *except food*.
///
/// The bonus tail, verbatim from `0x006E0A80..0x006E0B4B`: slot 0 takes `bonus_a + bonus_b`,
/// slots 1..5 take **only** `bonus_b`, and each is guarded by
/// `bonus != 0 || mult[i] > 100` so an untouched slot is left exactly alone rather than
/// being round-tripped through `* 100 / 100`.
pub fn calc_rare(
    rules: &EconRules,
    good_type: i32,
    good: &GoodTypeYield,
    ctx: &RareContext,
    own_territory: bool,
) -> [i32; NUM_RESOURCES] {
    let is_fish = good_type == 6 || good_type == 0x1F;
    let merchant_pct = rules.merchants_bonus(ctx.merchants_level);

    let mut mult = [100i32; NUM_RESOURCES];
    let mut out = [0i32; NUM_RESOURCES];

    for k in 0..2 {
        let rid = good.res_id[k];
        if (rid as u32) < NUM_RESOURCES as u32 {
            let r = rid as usize;
            out[r] = out[r].wrapping_add(good.amount[k].wrapping_mul(16));
            if own_territory || (is_fish && rid != 0) {
                mult[r] = merchant_pct;
            }
        }
    }

    // 0x006E0994 onward. `bonus_a` is food-only; `bonus_b` applies to every slot.
    let mut bonus_a = 0i32;
    let mut bonus_b = 0i32;
    if is_fish {
        bonus_a = rules.fishermen_bonus(ctx.fishing_level);
        if ctx.japanese {
            bonus_b = rules.japanese_fishing_boats();
        }
    } else if own_territory {
        if ctx.porcelain {
            bonus_b = rules.porcelain_rare();
        }
        if ctx.nubian {
            bonus_b = bonus_b.wrapping_add(rules.nubian_rare());
        }
    }

    if bonus_a.wrapping_add(bonus_b) != 0 || mult[0] > 100 {
        out[0] = pct(out[0], bonus_a.wrapping_add(bonus_b).wrapping_add(mult[0]));
    }
    for i in 1..NUM_RESOURCES {
        if bonus_b != 0 || mult[i] > 100 {
            out[i] = pct(out[i], mult[i].wrapping_add(bonus_b));
        }
    }
    out
}

/// The crowding divisor from `UnitData::calc_gather` `0x00609180`.
///
/// After resolving which node a gatherer is working, the engine scans the neighbourhood for
/// other units of the **same owner** that are (a) gathering, (b) of a compatible type, and
/// (c) within `(their_radius + my_radius) * 192` world units — 192 being one tile — and
/// divides **all six** yield slots by `competitors + 1` (`0x006098xx`, six consecutive
/// `idiv`s). Same-node contention is therefore a plain integer share, applied *after* the
/// rare bonuses, so the rounding loss is per-slot and per-unit.
///
/// The spatial search itself needs the world and object registry and is not ported here;
/// this is the arithmetic it ends in, and the divisor is the caller's to supply.
#[inline]
pub fn share_among_gatherers(
    yield_: [i32; NUM_RESOURCES],
    competitors: i32,
) -> [i32; NUM_RESOURCES] {
    if competitors <= 0 {
        return yield_;
    }
    let d = competitors.wrapping_add(1);
    let mut out = yield_;
    for v in out.iter_mut() {
        *v /= d;
    }
    out
}

// ---------------------------------------------------------------------------------------
// LeaderData::calc_city_resources  0x006D5530  -- the per-city yield
// ---------------------------------------------------------------------------------------

/// The per-worker gather rate, `PEASANT_RATE` unscaled from 8.8 (`0x00639E40 +0x18D`:
/// `(rate + (rate >> 31 & 0xFF)) >> 8`).
///
/// Shipped `PEASANT_RATE` is 2560, so **10 resources per `GATHER_RATE` frames per worker**
/// — 10 per 30 game-seconds. Oil workers use `OIL_RATE` instead (8960 → 35), selected at
/// `0x00639E40 +0x9E5`.
#[inline]
pub fn worker_rate(rules: &EconRules, oil: bool) -> i32 {
    let raw = if oil {
        rules.oil_rate()
    } else {
        rules.peasant_rate()
    };
    unscale_8_8(raw)
}

/// `SCHOLAR_RATE[level - 1]`, the university/scholar payout, unscaled the same way.
///
/// **The table is indexed `level - 1`, not `level`** (`0x006D5754`), which is the same
/// off-by-one convention the sibling tech-cities lane found on the other gather-enhancer
/// tables (`GRANARY_BONUS`, `LUMBERMILL_BONUS`, `SMELTER_BONUS`, `FISHERMEN_BONUS`). A
/// port that indexes by `level` reads the next tier's number for every building in the
/// game.
///
/// ⚠ **Unresolved, and stated rather than smoothed over.** The engine computes
/// `(SCHOLAR_RATE[level-1] * 16 + bias) >> 8`, i.e. `value / 16`, whereas the peasant path
/// is `value / 256` on a scale-256 constant. `SCHOLAR_RATE` is a **scale-1** field (shipped
/// `"5"` → 5), so `5 / 16` truncates to **0**. Either the constant is intended to be read
/// at a different scale, or the knowledge term is genuinely zero at level 1 in this build.
/// We have not resolved which, so this function returns exactly what the instructions
/// compute and the caller is warned. Do not "fix" it to 5 without an oracle run.
#[inline]
pub fn scholar_rate_for_level(rules: &EconRules, level: i32) -> i32 {
    let idx = level.wrapping_sub(1).max(0) as usize;
    unscale_8_8(rules.scholar_rate(idx).wrapping_mul(16))
}

/// Inputs to [`calc_city_resources`] for one city (or for the city-less census).
#[derive(Clone, Debug, Default)]
pub struct CityResourceInputs {
    /// `city + 0x52`, a per-city wealth term added straight into slot 2 before anything
    /// else (`0x006D5598`, sign-extended from a `short`). Only in city mode.
    pub city_wealth_field: i32,
    /// The summed six-slot result of `BuildData::calc_gather` (`0x0062D360`) over every
    /// building the walk accepts: alive, its type `is_gather_enhancer`, **not** carrying
    /// property `0x1A5` or `0x1A6`, and belonging to this city (`data + 0x72 == city`).
    ///
    /// The walk itself is a linked list through `build->data[0x74]` from the city's head
    /// building at `city + 0x08`; in census mode it is instead a scan of two object bands
    /// collecting everything with `data + 0x72 == -1`, capped at **99** entries
    /// (`0x006D5630`: `if (n > 0x62) break`).
    pub enhancer_income: [i32; NUM_RESOURCES],
    /// The head building's type is `0x213` — the Forbidden City. Scales every slot by
    /// `FORBIDDEN_CITY_GATHER` **and** replaces `CITY_GATHER[i]` with
    /// `FORBIDDEN_CITY_BASE_GATHER`.
    pub forbidden_city: bool,
    /// The campaign hero with property `0x165` is standing in this city
    /// (`ObjectsData::find_city_at` `0x0065B870` returned this city index).
    pub ceo_present: bool,
    /// `has_tribe_bonus(0x06)` — Romans.
    pub roman: bool,
    /// `has_tribe_bonus(0x0C)` — Germans.
    pub german: bool,
    /// `LeaderData::type_avail(RES_METAL, 1)` — gates the German metal share only.
    pub metal_available: bool,
    /// `CityData::get_taxes` `0x00737B50`; see [`city_taxes`].
    pub taxes: i32,
    /// `CityData::get_literacy` `0x00737C00` — the city's knowledge contribution.
    pub literacy: i32,
}

/// `LeaderData::calc_city_resources` `0x006D5530` — one city's six-slot contribution.
///
/// **This is the function the tech-cities lane named as its open gap.** It has two modes,
/// selected by the `city` argument, and they are materially different:
///
/// * **`Some(city)`** — a real city. Walks the city's building list, then adds the
///   Forbidden City multiplier, the hero multiplier, `CITY_GATHER`, the Roman and German
///   civ terms, `CityData::get_taxes` and `CityData::get_literacy`.
/// * **`None`** — the *census* of buildings that belong to no city (`data + 0x72 == -1`).
///   Adds only `VILLAGE_TAXES` and returns. Every step above is skipped, which is why
///   **the census contributes no knowledge at all** — knowledge enters solely through
///   `get_literacy`, which is city-mode only.
///
/// Every term lands in 1/16 units: the flat ones are `<< 4` at their site, the enhancer
/// income already arrives shifted from `BuildData::calc_gather`.
///
/// One retail oddity, reported not smoothed: `CityData::get_level` is called and stored
/// (`0x006D5570`) on the *city* path, but the only reader of that slot is the
/// `VILLAGE_TAXES` term on the *census* path, where the value is the literal 1
/// (`0x006D557B`). So the level is computed and discarded. Shipped `VILLAGE_TAXES` is 0,
/// so nothing observable rides on it either way.
pub fn calc_city_resources(
    rules: &EconRules,
    city: Option<&CityResourceInputs>,
) -> [i32; NUM_RESOURCES] {
    let mut out = [0i32; NUM_RESOURCES];

    let Some(c) = city else {
        // Census mode. `local_1b8` is the literal 1 here.
        out[RES_WEALTH] = rules.village_taxes().wrapping_mul(16);
        return out;
    };

    // 0x006D5598 -- the city's own wealth field, before anything else.
    out[RES_WEALTH] = out[RES_WEALTH].wrapping_add(c.city_wealth_field);

    // 0x006D57xx -- the accepted buildings' calc_gather sum.
    for i in 0..NUM_RESOURCES {
        out[i] = out[i].wrapping_add(c.enhancer_income[i]);
    }

    // 0x006D5820 -- Forbidden City, every slot.
    if c.forbidden_city {
        for i in 0..NUM_RESOURCES {
            out[i] = pct(out[i], rules.forbidden_city_gather().wrapping_add(100));
        }
    }

    // 0x006D58F0 -- the hero, every slot. Retail breaks after the first match.
    if c.ceo_present {
        for i in 0..NUM_RESOURCES {
            out[i] = pct(out[i], rules.theceo_production_bonus().wrapping_add(100));
        }
    }

    // 0x006D5960 -- CITY_GATHER, per slot, skipped when the rule is 0. The Forbidden City
    // *replaces* the value rather than scaling it, and only when the override is non-zero.
    for i in 0..NUM_RESOURCES {
        if rules.city_gather(i) == 0 {
            continue;
        }
        let v = if c.forbidden_city && rules.forbidden_city_base_gather() != 0 {
            rules.forbidden_city_base_gather()
        } else {
            rules.city_gather(i)
        };
        out[i] = out[i].wrapping_add(v.wrapping_mul(16));
    }

    // 0x006D59B0 -- Romans.
    if rules.roman_city_gather() != 0 && c.roman {
        out[RES_WEALTH] = out[RES_WEALTH].wrapping_add(rules.roman_city_gather().wrapping_mul(16));
    }

    // 0x006D59D0 -- Germans. Metal is gated on `type_avail`; food and timber are not.
    if rules.german_city_gather() != 0 && c.german {
        let v = rules.german_city_gather().wrapping_mul(16);
        out[RES_FOOD] = out[RES_FOOD].wrapping_add(v);
        out[RES_TIMBER] = out[RES_TIMBER].wrapping_add(v);
        if c.metal_available {
            out[RES_METAL] = out[RES_METAL].wrapping_add(v);
        }
    }

    // 0x006D5A20 / 0x006D5A35.
    out[RES_WEALTH] = out[RES_WEALTH].wrapping_add(c.taxes.wrapping_mul(16));
    out[RES_KNOWLEDGE] = out[RES_KNOWLEDGE].wrapping_add(c.literacy.wrapping_mul(16));

    out
}

/// Leader-wide wonder / tech / civ multipliers, `LeaderData::calc_resource_bonuses`
/// `0x006DB030`, applied last inside `calc_gather`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ResourceBonusGates {
    /// `has_tribe_bonus(0x0D)` — Russian oil.
    pub russian: bool,
    /// `has_wonder(0x20E)` — Pyramids, food.
    pub pyramids: bool,
    /// `has_wonder(0x20F)` — Colossus, wealth.
    pub colossus: bool,
    /// `has_wonder(0x210)` — Hanging Gardens, flat knowledge.
    pub hanging_gardens: bool,
    /// `has_wonder(0x217)` — Angkor Wat, metal.
    pub angkor: bool,
    /// `has_wonder(0x21B)` — Taj Mahal, wealth.
    pub taj: bool,
    /// `has_wonder(0x21C)` — Eiffel Tower, oil.
    pub eiffel: bool,
    /// `has_wonder(0x214)` — Temple of Tikal, timber.
    pub tikal: bool,
    /// `has_preq(0x2B7)` — global prosperity; every slot **except knowledge**.
    pub global_prosperity: bool,
    /// Conquer-the-World only (`game[0x822] & 2`): a per-resource stack count at
    /// `leader + 0x691C + res`, each stack worth `CTW_PROD_RATE_BONUS` percent.
    pub ctw_stacks: Option<[u8; NUM_RESOURCES]>,
}

/// `LeaderData::calc_resource_bonuses` `0x006DB030`.
///
/// Order matters — these are sequential multiplies with truncation between each, so
/// reordering them changes the result. Transcribed in source order.
pub fn calc_resource_bonuses(
    rules: &EconRules,
    gates: &ResourceBonusGates,
    out: &mut [i32; NUM_RESOURCES],
) {
    if gates.russian {
        out[RES_OIL] = pct(out[RES_OIL], rules.russian_oil().wrapping_add(100));
    }
    if gates.pyramids {
        out[RES_FOOD] = pct(out[RES_FOOD], rules.pyramids_food().wrapping_add(100));
    }
    if gates.colossus {
        out[RES_WEALTH] = pct(out[RES_WEALTH], rules.colossus_wealth().wrapping_add(100));
    }
    if gates.hanging_gardens {
        // Flat, and already in 1/16 units: `+= rule << 4` at 0x006DB0AE.
        out[RES_KNOWLEDGE] =
            out[RES_KNOWLEDGE].wrapping_add(rules.hanging_gardens_knowledge().wrapping_mul(16));
    }
    if gates.angkor {
        out[RES_METAL] = pct(out[RES_METAL], rules.angkor_metal().wrapping_add(100));
    }
    if gates.taj {
        out[RES_WEALTH] = pct(out[RES_WEALTH], rules.taj_wealth().wrapping_add(100));
    }
    if gates.eiffel {
        out[RES_OIL] = pct(out[RES_OIL], rules.eiffel_oil().wrapping_add(100));
    }
    if gates.tikal {
        out[RES_TIMBER] = pct(out[RES_TIMBER], rules.tikal_timber().wrapping_add(100));
    }
    if gates.global_prosperity {
        for i in 0..NUM_RESOURCES {
            if i != RES_KNOWLEDGE {
                out[i] = pct(out[i], rules.global_prosperity().wrapping_add(100));
            }
        }
    }
    if let Some(stacks) = gates.ctw_stacks {
        // 0x006DB13A: `out[i] += (stacks[i] * CTW_PROD_RATE_BONUS * out[i]) / 100`
        // -- an *additive* term computed from the pre-update value, not a `* (100+x)/100`.
        for i in 0..NUM_RESOURCES {
            let s = stacks[i] as i32;
            if s != 0 {
                let add = s
                    .wrapping_mul(rules.ctw_prod_rate_bonus())
                    .wrapping_mul(out[i])
                    / 100;
                out[i] = out[i].wrapping_add(add);
            }
        }
    }
}

/// One resource's substitution rule, from the `GoodType`/resource type record read at
/// `0x006CF67C`: `type + 0x2C4` is the destination slot (negative = none) and
/// `type + 0x2D8` is an 8.8 conversion rate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Substitution {
    /// Destination resource slot, or negative for "discard".
    pub target: i32,
    /// Conversion rate in 8.8 fixed point.
    pub rate_8_8: i32,
}

impl Default for Substitution {
    fn default() -> Self {
        Substitution {
            target: -1,
            rate_8_8: 0,
        }
    }
}

/// Everything `Leader::calc_gather` obtains by walking the object graph or querying the
/// leader's tech state. Nothing here is invented: each field names the engine call it
/// stands in for.
#[derive(Clone, Debug)]
pub struct GatherInputs {
    /// Sum of the six-slot contributions from every city
    /// (`City::calc_gather` `0x00737C60` -> `LeaderData::calc_city_resources` `0x006D5530`),
    /// every gather-enhancing building (`BuildData::calc_gather` `0x0062D360`), the
    /// band-2000 special buildings of type `0x1A2`/`0x1A3`, and every gathering unit
    /// (`Unit::do_gather` `0x005FCE20` -> `UnitData::calc_gather` `0x00609180`).
    ///
    /// These are four separate loops in retail; they are summed into the same accumulator
    /// with no intervening arithmetic, so a single total is faithful.
    pub object_income: [i32; NUM_RESOURCES],
    /// `has_tribe_bonus(0x13)` (Lakota): the unit-count expression at `0x006CF0C0`,
    /// `(a - b - c - d) + e + f` over six leader counters. Supplied as the composed count.
    pub lakota_units: Option<i32>,
    /// `has_tribe_bonus(0x14)` (Americans): the barracks-like building count at
    /// `0x006CF0E2`. The bonus lands on slots 0, 1, 4, 2, each gated by `type_avail`.
    pub americans_buildings: Option<i32>,
    /// `LeaderData::get_buildings(0x1AA, ...)` at `0x006CF44C` — the refinery count.
    pub refineries: i32,
    /// `has_preq(0x325)` — capitalism, flat oil income.
    pub capitalism: bool,
    /// `has_tribe_bonus(0x02)` — Inca. Only fires when `INCA_WEALTH_PER_MINER < 0`, which
    /// shipped data does not satisfy.
    pub inca: bool,
    /// The leader's rare-resource bitmask, `leader + 0x6DCC`, 44 bits.
    pub rares: [bool; NUM_RARES],
    /// Yield record per rare good type, indexed the same way as `rares`.
    pub rare_yields: [GoodTypeYield; NUM_RARES],
    /// Passed to `calc_rare` for the leader's own bonuses.
    pub rare_ctx: RareContext,
    /// `LeaderData::has_rare(0x2C)` — coffee, +10% on everything.
    pub coffee: bool,
    /// `LeaderData::get_taxation` `0x006D6E20`: 0..4, indexing `TERRITORY_TAXES`.
    pub taxation_level: usize,
    /// `has_tribe_bonus(0x0B)` — British, doubles the territory tax rate.
    pub british: bool,
    /// Conquer-the-World missionaries (`game[0x822] & 2 && has_conquest_bonus(0x20)`).
    pub ctw_missionaries: bool,
    /// `leader + 0x9D8` — tiles of territory owned.
    pub territory_tiles: i32,
    /// `world + 0x78` — total land tiles. A **divisor**; zero disables the whole tax term
    /// (`test eax,eax; je` at `0x006CF4E8`).
    pub total_land_tiles: i32,
    /// `has_tribe_bonus(0x11)` — Mongols, food from territory.
    pub mongol: bool,
    /// `game + 0x6A0`, the multiplier in the Mongol food term at `0x006CF55E`.
    pub mongol_game_term: i32,
    /// Passed through to [`calc_resource_bonuses`].
    pub bonus_gates: ResourceBonusGates,
    /// `LeaderData::type_avail(res, 1)` per resource — is this resource in play at all.
    pub type_avail: [bool; NUM_RESOURCES],
    /// `LeaderData::has_preq(res)` per resource, the second half of the substitution gate.
    pub has_preq: [bool; NUM_RESOURCES],
    /// Substitution target and rate per resource.
    pub substitution: [Substitution; NUM_RESOURCES],
}

impl Default for GatherInputs {
    fn default() -> Self {
        GatherInputs {
            object_income: [0; NUM_RESOURCES],
            lakota_units: None,
            americans_buildings: None,
            refineries: 0,
            capitalism: false,
            inca: false,
            rares: [false; NUM_RARES],
            rare_yields: [GoodTypeYield::default(); NUM_RARES],
            rare_ctx: RareContext::default(),
            coffee: false,
            taxation_level: 0,
            british: false,
            ctw_missionaries: false,
            territory_tiles: 0,
            total_land_tiles: 0,
            mongol: false,
            mongol_game_term: 0,
            bonus_gates: ResourceBonusGates::default(),
            type_avail: [true; NUM_RESOURCES],
            has_preq: [false; NUM_RESOURCES],
            substitution: [Substitution::default(); NUM_RESOURCES],
        }
    }
}

/// What `calc_gather` produces besides the income itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GatherOutput {
    /// Gross income per resource, in **sixteenths of a resource per `GATHER_RATE` frames**.
    pub gross: [i32; NUM_RESOURCES],
    /// `leader + 0x6D8 + i*4`: how many nodes of rare `i` contributed this pass. Written
    /// by the rare loop at `0x006CF50C` and by `UnitData::calc_gather`'s `param_6` counter.
    pub rare_counts: [i32; NUM_RARES],
}

impl Default for GatherOutput {
    fn default() -> Self {
        GatherOutput {
            gross: [0; NUM_RESOURCES],
            rare_counts: [0; NUM_RARES],
        }
    }
}

/// `Leader::calc_gather` `0x006CEEE0` — compose one leader's gross income.
///
/// **This is the function `docs/derivation/sim-economy.md` §7 called "the single largest
/// hole left in the economy"**, listed there as `FUN_006CEEE0`. The composition is now
/// ported; the four object-graph loops that feed `object_income` are not, and are inputs.
///
/// Step order is retail order; every step truncates, so it is not reorderable.
pub fn calc_gather(rules: &EconRules, inp: &GatherInputs) -> GatherOutput {
    let mut out = [0i32; NUM_RESOURCES];
    let mut rare_counts = [0i32; NUM_RARES];

    // 1. 0x006CF0A6 -- the flat baseline, shifted into 1/16 units.
    for i in 0..NUM_RESOURCES {
        out[i] = rules.basic_gather(i).wrapping_mul(16);
    }

    // 2. 0x006CF0C0 -- Lakota food.
    if let Some(n) = inp.lakota_units {
        out[RES_FOOD] =
            out[RES_FOOD].wrapping_add(n.wrapping_mul(rules.lakota_food()).wrapping_mul(16));
    }

    // 3. 0x006CF0E2 -- Americans. Slot order 0, 1, 4, 2 exactly as retail emits it; each
    //    `type_avail`-gated, so an unavailable resource silently drops its share.
    if let Some(n) = inp.americans_buildings {
        let v = n
            .wrapping_mul(rules.americans_barracks_gather())
            .wrapping_mul(16);
        for &slot in &[RES_FOOD, RES_TIMBER, RES_METAL, RES_WEALTH] {
            if inp.type_avail[slot] {
                out[slot] = out[slot].wrapping_add(v);
            }
        }
    }

    // 4. 0x006CF12A..0x006CF43F -- cities, gather buildings, special builds, gatherers.
    for i in 0..NUM_RESOURCES {
        out[i] = out[i].wrapping_add(inp.object_income[i]);
    }

    // 5. 0x006CF48C -- refineries scale oil.
    out[RES_OIL] = pct(
        out[RES_OIL],
        inp.refineries
            .wrapping_mul(rules.refinery_bonus())
            .wrapping_add(100),
    );

    // 6. 0x006CF4A6 -- capitalism, flat.
    if inp.capitalism {
        out[RES_OIL] = out[RES_OIL].wrapping_add(rules.capitalism_oil_prod().wrapping_mul(16));
    }

    // 7. 0x006CF4B9 -- Inca. The gate really is `< 0`; shipped `10` disables it.
    if rules.inca_wealth_per_miner() < 0 && inp.inca {
        out[RES_WEALTH] = out[RES_WEALTH].wrapping_add(out[RES_METAL]);
    }

    // 8. 0x006CF4DC -- rare resources, one `calc_rare` per set bit.
    for bit in 0..NUM_RARES {
        if !inp.rares[bit] {
            continue;
        }
        rare_counts[bit] = rare_counts[bit].wrapping_add(1);
        let y = calc_rare(
            rules,
            FIRST_RARE_GOOD_TYPE.wrapping_add(bit as i32),
            &inp.rare_yields[bit],
            &inp.rare_ctx,
            false,
        );
        for i in 0..NUM_RESOURCES {
            out[i] = out[i].wrapping_add(y[i]);
        }
    }

    // 9. 0x006CF5B7 -- coffee, every slot.
    if inp.coffee {
        for i in 0..NUM_RESOURCES {
            out[i] = pct(out[i], rules.coffee_income_bonus().wrapping_add(100));
        }
    }

    // 10. 0x006CF4E8 -- territory taxation. `total_land_tiles == 0` skips the whole block.
    if inp.total_land_tiles != 0 {
        let mut t = rules.territory_taxes(inp.taxation_level);
        if inp.british {
            t = pct(t, rules.british_taxation().wrapping_add(100));
        }
        if inp.ctw_missionaries {
            t = pct(t, rules.ctw_missionaries_bonus().wrapping_add(100));
        }
        // 0x006CF53C: `(territory * t * 16) / total_land`.
        let wealth = inp.territory_tiles.wrapping_mul(t).wrapping_mul(16) / inp.total_land_tiles;
        out[RES_WEALTH] = out[RES_WEALTH].wrapping_add(wealth);

        // 0x006CF55E: Mongols. Note the literal 800, not `<< 4`, and the double divide.
        if inp.mongol && rules.mongol_nomadic_food() != 0 {
            let food = (inp
                .mongol_game_term
                .wrapping_mul(inp.territory_tiles)
                .wrapping_mul(800)
                / inp.total_land_tiles)
                / rules.mongol_nomadic_food();
            out[RES_FOOD] = out[RES_FOOD].wrapping_add(food);
        }
    }

    // 11. 0x006CF5C6 -- LeaderData::calc_resource_bonuses.
    calc_resource_bonuses(rules, &inp.bonus_gates, &mut out);

    // 12. 0x006CF64C -- resource substitution. An unavailable resource whose prerequisite
    //     the leader nonetheless holds is converted at an 8.8 rate into another slot, and
    //     its own slot is zeroed. This is how an age without oil still produces something.
    for res in 0..NUM_RESOURCES {
        if inp.type_avail[res] || !inp.has_preq[res] {
            continue;
        }
        let sub = inp.substitution[res];
        if sub.target >= 0 {
            let t = sub.target as usize;
            if t < NUM_RESOURCES {
                let moved = unscale_8_8(out[res].wrapping_mul(sub.rate_8_8));
                out[t] = out[t].wrapping_add(moved);
            }
        }
        out[res] = 0;
    }

    GatherOutput {
        gross: out,
        rare_counts,
    }
}

// ---------------------------------------------------------------------------------------
// Leader::calc_resource_caps  0x006CE900
// ---------------------------------------------------------------------------------------

/// The knowledge cap, stored as the literal `0x1166` at `0x006CE92C`; `0x1166 ^ 0x1281`
/// is `0x3E7`. It is **not** in `rules.xml`, and the `jmp` after it skips every bonus, so
/// no civ or wonder can move it.
pub const KNOWLEDGE_CAP: i32 = 999;

/// Per-leader gates for `Leader::calc_resource_caps`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CapGates {
    /// `has_tribe_bonus(0x0B)` — British, +25% to every cap.
    pub british: bool,
    /// `has_tribe_bonus(0x02)` — Inca, wealth only.
    pub inca: bool,
    /// `has_tribe_bonus(0x0A)` — French, timber only.
    pub french: bool,
    /// `has_tribe_bonus(0x07)` — Egyptian, food only.
    pub egyptian: bool,
    /// Sum of the additive wonder/tech terms from `0x006CEAA6` on, per resource. Each has
    /// its own wonder check and none is derived, so passing zero models "no wonders"
    /// honestly rather than guessing.
    pub wonder_additive: [i32; NUM_RESOURCES],
}

/// `Leader::calc_resource_caps` `0x006CE900` — the per-tick income ceiling for all six
/// resources.
///
/// The per-resource civ branches are **mutually exclusive** (`0x006CE9A9` / `0x006CEA04` /
/// `0x006CEA5C` each jump past the others), and slots 4 and 5 have none. Each branch is
/// skipped outright when its rule is 0, so a zeroed rule never even queries the property.
pub fn calc_resource_caps(rules: &EconRules, age: i32, gates: &CapGates) -> [i32; NUM_RESOURCES] {
    let mut caps = [0i32; NUM_RESOURCES];
    let age_idx = age.clamp(0, 7) as usize;
    for res in 0..NUM_RESOURCES {
        if res == RES_KNOWLEDGE {
            caps[res] = KNOWLEDGE_CAP;
            continue;
        }
        let mut c = rules.commerce_cap(age_idx);
        if gates.british && rules.british_commerce() != 0 {
            c = pct(c, rules.british_commerce().wrapping_add(100));
        }
        // Mutually exclusive, in retail's branch order.
        if res == RES_WEALTH && gates.inca && rules.inca_wealth_cap() != 0 {
            c = pct(c, rules.inca_wealth_cap().wrapping_add(100));
        } else if res == RES_TIMBER && gates.french && rules.french_timber_commerce() != 0 {
            c = pct(c, rules.french_timber_commerce().wrapping_add(100));
        } else if res == RES_FOOD && gates.egyptian && rules.egyptian_food_commerce() != 0 {
            c = pct(c, rules.egyptian_food_commerce().wrapping_add(100));
        }
        caps[res] = c.wrapping_add(gates.wonder_additive[res]);
    }
    caps
}

// ---------------------------------------------------------------------------------------
// Leader::do_gather  0x006CE450  -- the payout
// ---------------------------------------------------------------------------------------

/// The global per-resource income ceiling, `0x3E70` at `0x006CE706` — 16,000 sixteenths,
/// i.e. **1,000 resources per `GATHER_RATE` frames**. Applied only on the Dutch-interest
/// path; knowledge and non-Dutch leaders have no ceiling.
pub const HARD_INCOME_CEILING: i32 = 0x3E70;

/// Per-tick, per-leader context for [`do_gather`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DoGatherContext {
    /// `LeaderData::type_avail(res, 1)` — resources that are not in play are skipped
    /// entirely (no payout, no accumulator movement).
    pub type_avail: [bool; NUM_RESOURCES],
    /// `leader + 0x4B0 + res*4` — an extra income term added before the cap test.
    pub extra_income: [i32; NUM_RESOURCES],
    /// `has_tribe_bonus(0x16)` — the Dutch interest path. Also the only path that applies
    /// [`HARD_INCOME_CEILING`].
    pub dutch: bool,
    /// The interest threshold, built at `0x006CE666`/`0x006CE684`/`0x006CE69A` from a
    /// game-config table (and, on one branch, a rule). Per resource. Not derived; supplied.
    pub interest_threshold: [i32; NUM_RESOURCES],
    /// `LeaderData::get_gather_handicap` `0x006D66A0` — a percentage *added* to 100, and
    /// **the AI difficulty cheat**: the difficulty lane measures it from −35 at Easiest to
    /// +50 at Toughest, a 2.29x end-to-end income spread between the extremes on identical
    /// towns [reported, sibling lane].
    ///
    /// Note the asymmetry the truncation creates. The step is
    /// `income * (100 + h) / 100` with C truncation, so at negative `h` the leftover
    /// fraction is always discarded and the *effective* penalty is strictly worse than
    /// nominal, while at positive `h` the bonus is merely rounded down. Low difficulties
    /// are therefore harsher than the number suggests, and the error is largest for small
    /// incomes — i.e. early game. A separate **cost** handicap
    /// (`LeaderData::get_handicap`) stacks on top at the extremes; it is not part of this
    /// function and is not modelled here.
    pub gather_handicap: i32,
    /// `game + 0x2F`. Difficulty > 4 penalises **knowledge only**: `*3/4` at 5..6, `/2`
    /// above 6.
    pub difficulty: u8,
    /// `game[0x20] & 2` or `game[0x2A] == 9` — a game-setting pair worth `*3/2`.
    pub bonus_setting: bool,
    /// `GameAccess::ai_speed` `[0x00C061C0]`. Multiplies income when `> 1`.
    pub ai_speed: i32,
}

impl Default for DoGatherContext {
    fn default() -> Self {
        DoGatherContext {
            type_avail: [true; NUM_RESOURCES],
            extra_income: [0; NUM_RESOURCES],
            dutch: false,
            interest_threshold: [0; NUM_RESOURCES],
            gather_handicap: 0,
            difficulty: 0,
            bonus_setting: false,
            ai_speed: 1,
        }
    }
}

/// The accumulator period, `GATHER_RATE * 16` (`shl ecx, 4` at `0x006CE7B9`). 7,200 with
/// shipped data — 450 frames of 16ths.
#[inline]
pub fn accumulator_period(rules: &EconRules) -> i32 {
    rules.gather_rate().wrapping_mul(16)
}

/// What one resource's payout did this frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Payout {
    /// Whole resources credited to the stockpile this frame.
    pub whole: i32,
    /// The post-cap, pre-handicap income the HUD is shown.
    pub displayed: i32,
}

/// `Leader::do_gather` `0x006CE450` — run the payout for all six resources.
///
/// Called once per frame per active leader from `Leader::gather` `0x006CE280`; there is no
/// modulo gate anywhere on the chain. All the periodicity is in the accumulator: income is
/// carried in sixteenths-per-`GATHER_RATE`, so `income / (GATHER_RATE * 16)` whole units
/// land immediately and the remainder accrues.
///
/// The pipeline, retail order (`docs/derivation/sim-economy.md` §4.1 corrected the earlier
/// write-up on three of these and they are honoured here):
///
/// 1. `income = gross - expense + extra` (`0x006CE4C8`)
/// 2. **negative income abandons the resource entirely** — the display is cached, the
///    capped flag cleared, and no stockpile write happens (`0x006CE4E7`)
/// 3. clamp to the commerce cap, setting `capped_flag` to `1 + (cap > 0x3E6F)` (`0x006CE512`)
/// 4. Dutch interest, `res != 3` only: `+= DUTCH_INTEREST% of surplus`, clamped to
///    `(DUTCH_INTEREST_CAP << 4) + cap` — note the shift is on the **rule alone**
///    (`0x006CE6F6`)
/// 5. `min(income, 16000)`, **only on that same gated path** (`0x006CE706`)
/// 6. cache the display value (`0x006CE723`)
/// 7. gather handicap (`0x006CE72C`)
/// 8. knowledge difficulty penalty (`0x006CE755`)
/// 9. game-setting `*3/2` (`0x006CE783`)
/// 10. `ai_speed` multiply when `> 1` (`0x006CE79C`)
/// 11. accumulate and credit (`0x006CE7AE`)
pub fn do_gather(
    rules: &EconRules,
    econ: &mut LeaderEcon,
    ctx: &DoGatherContext,
) -> [Payout; NUM_RESOURCES] {
    let period = accumulator_period(rules);
    let mut payouts = [Payout::default(); NUM_RESOURCES];

    for res in 0..NUM_RESOURCES {
        if !ctx.type_avail[res] {
            continue;
        }

        // 1.
        let mut income = econ.gross[res]
            .wrapping_sub(econ.expense[res])
            .wrapping_add(ctx.extra_income[res]);

        // 2.
        if income < 0 {
            econ.displayed[res] = income;
            econ.capped_flag[res] = 0;
            payouts[res].displayed = income;
            continue;
        }

        // 3.
        let cap = econ.commerce_cap[res];
        if income > cap {
            econ.capped_flag[res] = 1 + i32::from(cap > 0x3E6F);
            income = cap;
        } else {
            econ.capped_flag[res] = 0;
        }

        // 4 + 5. Both live behind `res != 3 && has_tribe_bonus(0x16)`.
        if res != RES_KNOWLEDGE && ctx.dutch {
            let surplus = econ.stockpile[res].wrapping_sub(ctx.interest_threshold[res]);
            if surplus > 0 {
                let with_interest = income.wrapping_add(
                    (rules.dutch_interest().wrapping_mul(surplus) / 100).wrapping_mul(16),
                );
                let limit = rules
                    .dutch_interest_cap()
                    .wrapping_mul(16)
                    .wrapping_add(cap);
                income = with_interest.min(limit);
            }
            if income >= HARD_INCOME_CEILING {
                income = HARD_INCOME_CEILING;
            }
        }

        // 6.
        econ.displayed[res] = income;
        payouts[res].displayed = income;

        // 7. `get_gather_handicap` returning 0 skips the multiply entirely (`jz`), which
        //    matters only in that `* 100 / 100` would be a no-op anyway.
        if ctx.gather_handicap != 0 {
            income = pct(income, ctx.gather_handicap.wrapping_add(100));
        }

        // 8. 0x006CE755: `res == 3 && difficulty > 1 && difficulty > 4`.
        if res == RES_KNOWLEDGE && ctx.difficulty > 4 {
            if ctx.difficulty < 7 {
                // `(x*3) >> 2` with the round-toward-zero bias.
                let x3 = income.wrapping_mul(3);
                income = (x3.wrapping_add((x3 >> 31) & 3)) >> 2;
            } else {
                income /= 2;
            }
        }

        // 9.
        if ctx.bonus_setting {
            income = income.wrapping_mul(3) / 2;
        }

        // 10.
        if ctx.ai_speed > 1 {
            income = income.wrapping_mul(ctx.ai_speed);
        }

        // 11. 0x006CE7AE. The carry loop is a loop, not folded into the division: a
        //     remainder left over from earlier frames changes which frame a payout lands
        //     on, and that is observable.
        let mut whole = income / period;
        econ.accumulator[res] = econ.accumulator[res].wrapping_add(income % period);
        while econ.accumulator[res] >= period {
            whole = whole.wrapping_add(1);
            econ.accumulator[res] = econ.accumulator[res].wrapping_sub(period);
        }
        econ.stockpile[res] = econ.stockpile[res].wrapping_add(whole);
        payouts[res].whole = whole;
    }

    payouts
}

/// `Leader::gather` `0x006CE280` — the per-frame entry point.
///
/// Copies the six gross-income slots out of the block, recomputes them when
/// [`calc_gather_due`] says so, writes them back, **zeroes the six expense slots**, then
/// runs `calc_resource_caps` and `do_gather` in that order.
///
/// The expense zeroing is easy to miss and is why expenses are per-frame rather than
/// cumulative: they are re-accrued by the consumers during the frame that follows.
#[allow(clippy::too_many_arguments)]
pub fn leader_gather(
    rules: &EconRules,
    econ: &mut LeaderEcon,
    frame: i32,
    leader_slot: i32,
    last_calc_frame: &mut i32,
    dirty: &mut bool,
    gather_inputs: &GatherInputs,
    cap_gates: &CapGates,
    ctx: &DoGatherContext,
) -> [Payout; NUM_RESOURCES] {
    if calc_gather_due(frame, leader_slot, *last_calc_frame, *dirty) {
        let g = calc_gather(rules, gather_inputs);
        econ.gross = g.gross;
        econ.breakdown = [0; NUM_RESOURCES];
        // 0x006CF5A2 / 0x006CF5AB: the dirty bit is cleared and the stamp taken *after*
        // the composition, so a change made during the same frame re-dirties correctly.
        *dirty = false;
        *last_calc_frame = frame;
    }
    econ.expense = [0; NUM_RESOURCES];
    econ.commerce_cap = calc_resource_caps(rules, econ.age, cap_gates);
    do_gather(rules, econ, ctx)
}

// ---------------------------------------------------------------------------------------
// The market
// ---------------------------------------------------------------------------------------

/// The market state, `Game + 0x564 .. Game + 0x5E0`.
///
/// Offsets recovered from `GameDaemon::calc_markets` (`esi` walking `0x568..0x580` with
/// `+0x18`, `+0x48` and `+0x60` displacements off it) and `GameDaemon::calc_market`
/// (`0x568`, `0x598`, `0x5B0`, `0x5C8`) [measured].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MarketState {
    /// `Game + 0x564`. Increments once per market cycle, not per frame.
    pub cycle: i32,
    /// `Game + 0x568 + res*4`. The base price. Buy = `2*base + spread`, sell = `base + spread`.
    pub base_price: [i32; NUM_RESOURCES],
    /// `Game + 0x580 + res*4`. The wandering spread, stepped toward `trend_target`.
    pub spread: [i32; NUM_RESOURCES],
    /// `Game + 0x598 + res*4`. Where the spread is currently heading.
    pub trend_target: [i32; NUM_RESOURCES],
    /// `Game + 0x5B0 + res*4`. Per-cycle step, `ceil`-away-from-zero of `delta / duration`.
    pub trend_step: [i32; NUM_RESOURCES],
    /// `Game + 0x5C8 + res*4`. Cycles remaining before a new trend is rolled.
    pub trend_countdown: [i32; NUM_RESOURCES],
}

/// `GameDaemon::calc_market` `0x00732270` — roll a new trend for one resource.
///
/// **This is an RNG consumer on the shared simulation stream** (`GameAccess::game_random`,
/// `[0x00C06184]`). It draws `Random::get(0, 0xFFFF)`:
///
/// * twice if `variance > 0`, else not at all;
/// * once more if `MARKET_TREND_RANGE - 1 > 0`.
///
/// With shipped rules that is **three draws every time**, but the count is genuinely
/// data-dependent, so a reimplementation must branch the same way or the stream position
/// diverges for every other consumer.
///
/// ```text
/// 00732292  v = base_price / 2                      (truncating toward zero)
/// 00732299  v = max(v, MARKET_MIN_VARIANCE)         (cmovg)
/// 0073229c  v = (v + 1) / 2                         (truncating toward zero)
/// 007322bf  a = rng(0,0xFFFF) % (v+1)               if v > 0 else 0
/// 007322e9  b = rng(0,0xFFFF) % (v+1)               if v > 0 else 0
/// 00732300  trend_target = b - (v+1) + a
/// 00732329  r = rng(0,0xFFFF) % MARKET_TREND_RANGE  if RANGE-1 > 0 else 0
/// 00732345  duration = MARKET_MIN_TREND + r
/// 0073234e  delta = trend_target - spread
/// 0073237f  trend_step = ((duration - 1) * sign(delta) + delta) / duration
/// ```
///
/// The last line is a divide that rounds **away from zero**, which is what keeps a trend
/// arriving in exactly `duration` cycles rather than stalling one short.
pub fn calc_market(rules: &EconRules, market: &mut MarketState, rng: &mut Random, res: usize) {
    let mut v = half_toward_zero(market.base_price[res]);
    if rules.market_min_variance() > v {
        v = rules.market_min_variance();
    }
    v = half_toward_zero(v.wrapping_add(1));

    let span = v.wrapping_add(1);
    let a = if v > 0 { rng.get(0, 0xFFFF) % span } else { 0 };
    let b = if v > 0 { rng.get(0, 0xFFFF) % span } else { 0 };
    market.trend_target[res] = b.wrapping_sub(span).wrapping_add(a);

    let range = rules.market_trend_range();
    let r = if range.wrapping_sub(1) > 0 {
        rng.get(0, 0xFFFF) % range
    } else {
        0
    };
    let duration = rules.market_min_trend().wrapping_add(r);

    let delta = market.trend_target[res].wrapping_sub(market.spread[res]);
    market.trend_countdown[res] = duration;
    if duration == 0 {
        // Unreachable with shipped data (MARKET_MIN_TREND = 8). Retail would divide by
        // zero here and fault; we refuse rather than inventing a value.
        market.trend_step[res] = delta;
        return;
    }
    let sign = if delta > 0 { 1 } else { delta >> 31 };
    market.trend_step[res] = (duration
        .wrapping_sub(1)
        .wrapping_mul(sign)
        .wrapping_add(delta))
        / duration;
}

/// `GameDaemon::calc_markets` `0x00732180` — the market tick, called once per frame from
/// `GameDaemon::process_all`.
///
/// Three nested periodicities, all `[measured]`:
///
/// * **frame gate** — runs when `frame == 0 || MARKET_CYCLE_RATE <= 1 ||
///   frame % MARKET_CYCLE_RATE == 0`. Shipped `MARKET_CYCLE_RATE` is 1, so: every frame.
/// * **per-resource gate** — a resource is touched when `cycle == 0` or
///   `(cycle + res) & 7 == 0`, so each resource is serviced every 8 cycles, phase-offset
///   by its own index. This is why the six prices do not move in lockstep.
/// * **equilibrium walk** — when `(cycle + res) & 0xFF == 0`, i.e. every 256 cycles, the
///   base price takes one step toward `MARKET_EQUILIBRIUM`: down by `price /
///   MARKET_EQUILIBRIUM` (or 1 when the rule is 0), up by 1 — or by **2** if a single step
///   would still leave it under `MARKET_BASEMENT`.
pub fn calc_markets(rules: &EconRules, market: &mut MarketState, rng: &mut Random, frame: i32) {
    let rate = rules.market_cycle_rate();
    if frame != 0 && rate > 1 && frame % rate != 0 {
        return;
    }

    for res in 0..NUM_RESOURCES {
        let cycle = market.cycle;
        let phase = cycle.wrapping_add(res as i32);

        // 0x007321CA: `test al, 7` -- the low byte, so an unsigned mod 8.
        if cycle != 0 && (phase as u32) & 7 != 0 {
            continue;
        }

        // 0x007321D4: `test al, al`.
        if (phase as u32) & 0xFF == 0 {
            let price = market.base_price[res];
            let eq = rules.market_equilibrium();
            if price > eq {
                market.base_price[res] = if eq == 0 {
                    price.wrapping_sub(1)
                } else {
                    price.wrapping_sub(price / eq)
                };
            } else if price < eq {
                let up = price.wrapping_add(1);
                market.base_price[res] = if up < rules.market_basement() {
                    price.wrapping_add(2)
                } else {
                    up
                };
            }
        }

        // 0x00732224. The countdown is decremented whenever `cycle != 0`, and reaching
        // zero (or `cycle == 0`, the first pass) rolls a new trend.
        let roll = if cycle == 0 {
            true
        } else {
            market.trend_countdown[res] = market.trend_countdown[res].wrapping_sub(1);
            market.trend_countdown[res] <= 0
        };
        if roll {
            calc_market(rules, market, rng, res);
        }

        // 0x00732244.
        market.spread[res] = market.spread[res].wrapping_add(market.trend_step[res]);
    }

    market.cycle = market.cycle.wrapping_add(1);
}

/// Per-leader gates for [`calc_market_prices`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MarketPriceGates {
    /// `has_tribe_bonus(0x04)` — Nubians. Takes precedence over the amber rare.
    pub nubian: bool,
    /// The amber rare (`leader + 0x6DA5 & 8` or `leader + 0x6DCD & 8`).
    pub amber: bool,
    /// `has_wonder(0x21D)` — the wonder that clamps prices into
    /// `[SUPER_SELL, SUPER_BUY]`.
    pub super_market: bool,
    /// Conquer-the-World only: the missionary/market stack count at
    /// `leader + 0x68FE`. Each stack divides the buy price and raises the sell price.
    pub ctw_stacks: u8,
    /// `has_tribe_bonus(0x0D)` — Russians. With `RUSSIAN_COMMUNISM != 0` and age > 4 this
    /// pins both prices to 100. Shipped `RUSSIAN_COMMUNISM` is 0, so it never fires.
    pub russian: bool,
    /// The leader's age, as `econ + 0xDC` stores it.
    pub age: i32,
}

/// A quoted pair.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MarketPrices {
    /// Wealth paid for 100 units.
    pub buy: i32,
    /// Wealth received for 100 units.
    pub sell: i32,
}

/// `LeaderData::calc_market_prices` `0x006DC2A0`.
///
/// ```text
/// sell = base + spread
/// buy  = base * 2 + spread
/// Nubian:  sell += NUBIAN_MARKET_PRICES; buy -= NUBIAN_MARKET_PRICES
/// else amber: sell += AMBER_MARKET;      buy -= AMBER_MARKET
/// CTW: buy = buy*100/(CTW_MARKET_BONUS_BUY+100), repeated `stacks` times
///      sell += (CTW_MARKET_BONUS_SELL * sell / 100) * stacks
/// sell = max(sell, 1)
/// buy  = max(buy, MARKET_BASEMENT * 2)
/// buy  = max(buy, sell + 10)
/// super market wonder: buy=min(buy,SUPER_BUY) sell=min(sell,SUPER_BUY-1)
///                      buy=max(buy,SUPER_SELL+1) sell=max(sell,SUPER_SELL)
/// Russian communism, age > 4: buy = sell = 100
/// ```
///
/// The `buy >= sell + 10` floor is the house edge and is what makes buy/sell round-trips
/// lossy no matter how the trend wanders.
pub fn calc_market_prices(
    rules: &EconRules,
    market: &MarketState,
    res: usize,
    gates: &MarketPriceGates,
) -> MarketPrices {
    let base = market.base_price[res];
    let spread = market.spread[res];
    let mut sell = base.wrapping_add(spread);
    let mut buy = base.wrapping_mul(2).wrapping_add(spread);

    // 0x006DC2E4. Nubian wins outright; amber is the `else`.
    let discount = if rules.nubian_market_prices() != 0 && gates.nubian {
        Some(rules.nubian_market_prices())
    } else if gates.amber {
        Some(rules.amber_market())
    } else {
        None
    };
    if let Some(d) = discount {
        sell = sell.wrapping_add(d);
        buy = buy.wrapping_sub(d);
    }

    // 0x006DC32F. Repeated divide, not a single one: `(100/125)^n`, truncating each time.
    if gates.ctw_stacks != 0 {
        let n = gates.ctw_stacks as i32;
        for _ in 0..n {
            buy = buy.wrapping_mul(100) / rules.ctw_market_bonus_buy().wrapping_add(100);
        }
        sell = sell
            .wrapping_add((rules.ctw_market_bonus_sell().wrapping_mul(sell) / 100).wrapping_mul(n));
    }

    // 0x006DC393.
    if sell < 1 {
        sell = 1;
    }
    let basement2 = rules.market_basement().wrapping_mul(2);
    if buy < basement2 {
        buy = basement2;
    }
    let floor = sell.wrapping_add(10);
    if buy < floor {
        buy = floor;
    }

    // 0x006DC3B4.
    if gates.super_market {
        let sb = rules.super_buy();
        let ss = rules.super_sell();
        if buy > sb {
            buy = sb;
        }
        if sell > sb.wrapping_sub(1) {
            sell = sb.wrapping_sub(1);
        }
        if buy < ss.wrapping_add(1) {
            buy = ss.wrapping_add(1);
        }
        if sell < ss {
            sell = ss;
        }
    }

    // 0x006DC3E8.
    if rules.russian_communism() != 0 && gates.russian && gates.age > 4 {
        buy = 100;
        sell = 100;
    }

    MarketPrices { buy, sell }
}

/// The trade lot size. Both `Leader::do_buy` and `Leader::do_sell` move exactly this many
/// units per command (`0x006CFC2E`, `0x006CFC94`).
pub const TRADE_LOT: i32 = 100;

/// Outcome of a market command.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TradeResult {
    /// Retail returns 0. The trade happened.
    Done,
    /// Retail returns 1. Insufficient wealth (buy) or insufficient stock (sell); nothing
    /// changed, including the price.
    Refused,
}

/// `Leader::do_buy` `0x006CFBD0`.
///
/// Pays `buy` wealth for [`TRADE_LOT`] units and pushes the **base** price up by
/// `MARKET_SUPPLY_DEMAND`, which moves the buy quote by twice that (buy is `2*base + spread`).
///
/// `demand_counter` is `leader + 0x470`, decremented by 100 and floored at 0 — the AI's
/// "how badly do I still want this" tracker.
pub fn do_buy(
    rules: &EconRules,
    market: &mut MarketState,
    econ: &mut LeaderEcon,
    demand_counter: &mut i32,
    res: usize,
    gates: &MarketPriceGates,
) -> TradeResult {
    let price = calc_market_prices(rules, market, res, gates).buy;
    if econ.stockpile[RES_WEALTH] < price {
        return TradeResult::Refused;
    }
    econ.stockpile[RES_WEALTH] = econ.stockpile[RES_WEALTH].wrapping_sub(price);
    *demand_counter = (*demand_counter).wrapping_sub(TRADE_LOT).max(0);
    econ.stockpile[res] = econ.stockpile[res].wrapping_add(TRADE_LOT);
    market.base_price[res] = market.base_price[res].wrapping_add(rules.market_supply_demand());
    TradeResult::Done
}

/// `Leader::do_sell` `0x006CFC60`.
///
/// Sells [`TRADE_LOT`] units for `sell` wealth and pushes the base price down by
/// `MARKET_SUPPLY_DEMAND`, **floored at zero** (`0x006CFCE9`) — the base price can reach 0
/// even though the quoted prices cannot.
///
/// `supply_counter` is `leader + 0x468 + res*4`.
pub fn do_sell(
    rules: &EconRules,
    market: &mut MarketState,
    econ: &mut LeaderEcon,
    supply_counter: &mut i32,
    res: usize,
    gates: &MarketPriceGates,
) -> TradeResult {
    let price = calc_market_prices(rules, market, res, gates).sell;
    if econ.stockpile[res] < TRADE_LOT {
        return TradeResult::Refused;
    }
    econ.stockpile[res] = econ.stockpile[res].wrapping_sub(TRADE_LOT);
    *supply_counter = (*supply_counter).wrapping_sub(TRADE_LOT).max(0);
    econ.stockpile[RES_WEALTH] = econ.stockpile[RES_WEALTH].wrapping_add(price);
    market.base_price[res] = market.base_price[res]
        .wrapping_sub(rules.market_supply_demand())
        .max(0);
    TradeResult::Done
}

/// adler-32 of the market block as `Game + 0x564 .. 0x5E0`, little-endian.
///
/// The market lives on `Game`, not on a leader, so it is not itself one of the sixteen
/// channels; it reaches the checksum through the **goods** and **leaders** channels by way
/// of the prices leaders trade at. Exposed here so a divergence can be localised to the
/// market rather than hunted through stockpiles.
pub fn market_adler32(m: &MarketState) -> u32 {
    let mut buf = Vec::with_capacity(0x7C);
    buf.extend_from_slice(&m.cycle.to_le_bytes());
    for arr in [
        &m.base_price,
        &m.spread,
        &m.trend_target,
        &m.trend_step,
        &m.trend_countdown,
    ] {
        for v in arr.iter() {
            buf.extend_from_slice(&v.to_le_bytes());
        }
    }
    adler32(1, &buf)
}

// ---------------------------------------------------------------------------------------
// Taxation, tribute, caravans
// ---------------------------------------------------------------------------------------

/// `LeaderData::get_taxation` `0x006D6E20` — the taxation upgrade level, 0..4.
///
/// A descending chain of `has_preq` tests on tech ids `0x30E`, `0x30D`, `0x30C`, `0x30B`;
/// the first hit wins, so pass the four results in that order.
#[inline]
pub fn taxation_level(
    has_preq_30e: bool,
    has_preq_30d: bool,
    has_preq_30c: bool,
    has_preq_30b: bool,
) -> usize {
    if has_preq_30e {
        4
    } else if has_preq_30d {
        3
    } else if has_preq_30c {
        2
    } else if has_preq_30b {
        1
    } else {
        0
    }
}

/// What one city contributes in flat wealth.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CityTaxInputs {
    /// `CityData::num_buildings` `0x00738190`.
    pub num_buildings: i32,
    /// `CityData::count_buildings(0x1B4, 0, 1)` — does the city have a market.
    pub has_market: bool,
    /// `CityData::count_buildings(0x1B5, 0, 1)` — does the city have a temple.
    pub has_temple: bool,
    /// `LeaderData::has_wonder(0x215)` — the Porcelain Tower, which scales the market term.
    pub porcelain: bool,
}

/// `CityData::get_taxes` `0x00737B50` — flat wealth from one city.
///
/// ```text
/// t = VILLAGE_TAXES + num_buildings * BUILDING_TAXES
/// if market: m = MARKET_TAXES; if porcelain: m = (PORCELAIN_MARKET + 100) * m / 100
///            t += m
/// if temple: t += TEMPLE_TAXES
/// ```
///
/// With shipped data only the market term is non-zero: 10 wealth per market, 40 with the
/// Porcelain Tower.
pub fn city_taxes(rules: &EconRules, c: &CityTaxInputs) -> i32 {
    let mut t = rules
        .village_taxes()
        .wrapping_add(c.num_buildings.wrapping_mul(rules.building_taxes()));
    if c.has_market {
        let mut m = rules.market_taxes();
        if c.porcelain {
            m = pct(m, rules.porcelain_market().wrapping_add(100));
        }
        t = t.wrapping_add(m);
    }
    if c.has_temple {
        t = t.wrapping_add(rules.temple_taxes());
    }
    t
}

/// `LeaderData::scale_tribute` `0x006D5240` — how much of a gift actually arrives.
///
/// ```text
/// pct = age * COMMERCE_TRIBUTE + BASE_TRIBUTE
/// if pct < 1:    pct = 1
/// if pct > 100:  return amount unchanged
/// if pct != 100:
///     amount < 20   -> amount = amount*pct           (truncate)
///     amount < 100  -> amount = amount*pct + 50      (round to nearest)
///     otherwise     -> amount = amount*pct + 99      (round up)
///     amount /= 100
/// ```
///
/// The three-way rounding bias is not decoration: a 19-unit gift is rounded *down*, a
/// 99-unit gift to nearest, and a 100-unit gift *up*. Shipped constants give 51% at age 0
/// rising 7 points per age, so tribute becomes lossless at age 7 (51 + 49 = 100) and
/// profitable — the `> 100` early-out — from age 8.
pub fn scale_tribute(rules: &EconRules, age: i32, amount: i32) -> i32 {
    let mut p = age
        .wrapping_mul(rules.commerce_tribute())
        .wrapping_add(rules.base_tribute());
    if p < 1 {
        p = 1;
    } else if p > 100 {
        return amount;
    }
    if p == 100 {
        return amount;
    }
    let scaled = if amount < 20 {
        amount.wrapping_mul(p)
    } else if amount < 100 {
        amount.wrapping_mul(p).wrapping_add(50)
    } else {
        amount.wrapping_mul(p).wrapping_add(99)
    };
    scaled / 100
}

/// Per-leader gates for [`caravan_limit`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CaravanGates {
    /// `has_wonder(0x20F)` — Colossus.
    pub colossus: bool,
    /// `has_wonder(0x21B)` — Taj Mahal.
    pub taj: bool,
    /// The silk rare (`leader + 0x6DA4 & 0x20` or `leader + 0x6DCC & 0x20`).
    pub silk: bool,
    /// `has_tribe_bonus(0x04)` — Nubians.
    pub nubian: bool,
}

/// The hard ceiling at `0x006DCAB4`.
pub const MAX_CARAVANS: i32 = 99;

/// `LeaderData::get_caravan_limit` `0x006DCA50`.
///
/// ```text
/// limit = age + 1
///       + COLOSSUS_CARAVAN (Colossus) + TAJ_CARAVAN (Taj) + SILK_CARAVAN (silk rare)
///       + NUBIAN_CARAVAN_LIMIT (Nubian)
/// limit = min(limit, 99)
/// if apply_city_pair_cap:
///     n = own cities (+ allied cities, with has_preq(0x2AC))
///     limit = min(limit, (n - 1) * n / 2)
/// ```
///
/// The second cap is the real one in practice: caravans run **between** cities, so a
/// player with `n` cities can support at most `C(n, 2)` routes. With three cities that is
/// 3, well under the age-based allowance for most of a match.
///
/// `taj_caravan` is one of the seven fields present in the binary but absent from the
/// shipped `rules.xml` (`docs/derivation/economy.md` §1.4), so it keeps the loader's
/// `-1` default unless a mod supplies it; it is passed in explicitly for that reason.
pub fn caravan_limit(
    rules: &EconRules,
    age: i32,
    gates: &CaravanGates,
    taj_caravan_value: i32,
    city_count: Option<i32>,
) -> i32 {
    let mut limit = age.wrapping_add(1);
    if gates.colossus {
        limit = limit.wrapping_add(rules.colossus_caravan());
    }
    if gates.taj {
        limit = limit.wrapping_add(taj_caravan_value);
    }
    if gates.silk {
        limit = limit.wrapping_add(rules.silk_caravan());
    }
    if gates.nubian {
        limit = limit.wrapping_add(rules.nubian_caravan_limit());
    }
    if limit > MAX_CARAVANS {
        limit = MAX_CARAVANS;
    }
    if let Some(n) = city_count {
        let pairs = n.wrapping_sub(1).wrapping_mul(n) / 2;
        if pairs < limit {
            limit = pairs;
        }
    }
    limit
}

// ---------------------------------------------------------------------------------------
// Checksum surface
// ---------------------------------------------------------------------------------------

/// One leader's economy contribution to the **leaders** channel of `CheckSums::check_all`
/// (`0x00936560`, channel 8: an inline loop over 8 leaders calling `Leader::walk_data`
/// `0x006D6750`).
///
/// `Leader::walk_data` hashes `leader[0x00..0x08]`, then — if `leader[0] & 1` —
/// `leader[0x08..0x6932]`, 26,914 contiguous bytes. The economy block is a *pointer* at
/// `leader + 0x6EB8`, past that range, so it is reached through one of the eight
/// `0x5C`-byte sub-walks at `leader + 0x692C` rather than by the flat run. We have not
/// resolved which, so this is our own framing of the same bytes, not retail's.
///
/// Two things the full channel needs that this function does **not** supply, both of which
/// would silently produce a matching-looking-but-wrong checksum if forgotten:
///
/// * **`LeaderData::anti_att` and `LeaderData::plunder_scale` are `f32` inside the walked
///   region.** They are two of the very few floats in sim state (with `Unit::move_step`'s
///   trig). Their bit patterns are hashed, so they must be reproduced to the bit, not to
///   within an epsilon — and they must be stored as `f32`, not promoted to `f64`.
/// * **`Array<T>` hashes its capacity and its growth hint, not just its live elements.**
///   Any growable container inside walked state therefore needs the engine's own growth
///   schedule; a Rust `Vec`, whose capacity doubles on a different schedule, diverges as
///   soon as one push crosses a boundary — even when every element matches.
pub fn leaders_channel(econs: &[LeaderEcon]) -> u32 {
    let mut a = 1u32;
    for e in econs {
        a = adler32(a, &e.image());
    }
    a
}

/// One resource node, as the **goods** channel sees it (channel 11, `0x00937710`).
///
/// `Good::walk_data` (`0x0066E5D0`) hashes exactly one byte at `Good + 0x20` on top of
/// `GoodData`'s base walk of `[0x08, 0x09)` and `[0x09, 0x18)` — 16 bytes plus a flag. The
/// node's *remaining amount* is inside that base range; which dword it is we have not
/// pinned, so this struct models the fields we can name and states the rest as unknown.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct GoodNode {
    /// `GoodType` index. Rares are `>= 6`.
    pub good_type: i32,
    /// Tile coordinates, `Coord`-packed as the engine stores them.
    pub x: i32,
    pub y: i32,
    /// Remaining yield. **Not derived**: no depletion site has been tied to a field, so
    /// this is our model of the concept, not a recovered offset. Do not compare it against
    /// retail.
    pub remaining: i32,
    /// `Good + 0x20`, the one byte `Good::walk_data` adds.
    pub flags: u8,
}

/// The **goods** channel over a node list.
///
/// Node order is the checksum's order, and the engine's `PtrArray<Good>` order is creation
/// order, so a port must preserve insertion order and must not compact on removal.
pub fn goods_channel(nodes: &[GoodNode]) -> u32 {
    let mut buf = Vec::with_capacity(nodes.len() * 17);
    for n in nodes {
        buf.extend_from_slice(&n.good_type.to_le_bytes());
        buf.extend_from_slice(&n.x.to_le_bytes());
        buf.extend_from_slice(&n.y.to_le_bytes());
        buf.extend_from_slice(&n.remaining.to_le_bytes());
        buf.push(n.flags);
    }
    adler32(1, &buf)
}

// ---------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn shipped() -> EconRules {
        EconRules::shipped()
    }

    // -- rules block ---------------------------------------------------------------------

    #[test]
    fn shipped_slots_land_at_their_engine_offsets() {
        let r = shipped();
        assert_eq!(r.gather_rate(), 450);
        assert_eq!(r.peasant_rate(), 2560, "10 resources at scale 256");
        assert_eq!(r.oil_rate(), 8960, "35 oil at scale 256");
        assert_eq!(r.commerce_cap(0), 70);
        assert_eq!(r.commerce_cap(7), 500);
        assert_eq!(r.market_equilibrium(), 65);
        assert_eq!(r.market_supply_demand(), 3);
        assert_eq!(r.territory_taxes(4), 300);
        assert_eq!(r.merchants_bonus(4), 300);
        assert_eq!(r.fishermen_bonus(3), 200);
        assert_eq!(r.at(636), r.gather_rate(), "byte-offset read agrees");
    }

    #[test]
    fn array_accessors_saturate_rather_than_read_a_neighbour() {
        let r = shipped();
        assert_eq!(r.commerce_cap(99), r.commerce_cap(7));
        assert_eq!(r.territory_taxes(99), r.territory_taxes(4));
    }

    #[test]
    fn from_block_matches_shipped_for_the_slots_we_carry() {
        let mut block = [0i32; RULES_DWORDS];
        for &(off, _, v) in SHIPPED_ECONOMY_SLOTS {
            block[off / 4] = v;
        }
        assert_eq!(EconRules::from_block(&block), shipped());
    }

    // -- integer helpers ------------------------------------------------------------------

    #[test]
    fn helpers_truncate_toward_zero_like_idiv() {
        assert_eq!(pct(7, 50), 3);
        assert_eq!(pct(-7, 50), -3, "idiv truncates toward zero, not down");
        assert_eq!(unscale_8_8(255), 0);
        assert_eq!(unscale_8_8(-255), 0);
        assert_eq!(unscale_8_8(-256), -1);
        assert_eq!(half_toward_zero(-3), -1);
        assert_eq!(half_toward_zero(3), 1);
        // The bias-and-shift form the engine writes is the same as `/ 256`.
        for v in [-1000i32, -257, -256, -1, 0, 1, 255, 256, 1000] {
            assert_eq!(unscale_8_8(v), v / 256, "v = {v}");
        }
    }

    // -- calc_gather scheduling -----------------------------------------------------------

    #[test]
    fn calc_gather_period_is_256_frames_staggered_by_leader() {
        // Clean path: 300-frame floor, then (frame + slot*8) % 256 == 0.
        assert!(
            !calc_gather_due(255, 0, 0, false),
            "under the 300-frame floor"
        );
        assert!(calc_gather_due(512, 0, 0, false));
        assert!(!calc_gather_due(512, 1, 0, false), "slot 1 is offset by 8");
        assert!(calc_gather_due(504, 1, 0, false), "504 + 8 = 512");
        assert!(!calc_gather_due(512, 0, 400, false), "400 + 300 > 512");
    }

    #[test]
    fn dirty_path_is_every_8_frames_and_ignores_the_300_frame_floor() {
        assert!(calc_gather_due(8, 0, 10_000, true));
        assert!(!calc_gather_due(9, 0, 0, true));
        assert!(calc_gather_due(7, 1, 0, true), "slot 1: 1 + 7 = 8");
        assert!(
            !calc_gather_due(0, 0, 0, true),
            "frame 0 is excluded when dirty"
        );
    }

    #[test]
    fn the_two_paths_have_different_periods() {
        let clean: Vec<i32> = (0..2048)
            .filter(|&f| calc_gather_due(f, 0, -10_000, false))
            .collect();
        let dirty: Vec<i32> = (0..2048)
            .filter(|&f| calc_gather_due(f, 0, -10_000, true))
            .collect();
        assert_eq!(clean.len(), 8, "2048 / 256");
        assert_eq!(dirty.len(), 255, "2048 / 8, minus frame 0");
    }

    // -- calc_rare -------------------------------------------------------------------------

    fn one_good(res: usize, amount: i32) -> GoodTypeYield {
        GoodTypeYield {
            res_id: [res as i32, -1],
            amount: [amount, 0],
        }
    }

    #[test]
    fn a_plain_rare_yields_amount_times_sixteen() {
        let r = shipped();
        let ctx = RareContext::default();
        let y = calc_rare(&r, 10, &one_good(RES_METAL, 5), &ctx, false);
        assert_eq!(y[RES_METAL], 80, "5 * 16, no bonus applies");
        assert_eq!(y[RES_FOOD], 0);
    }

    #[test]
    fn merchants_level_replaces_the_baseline_it_does_not_stack() {
        let r = shipped();
        let ctx = RareContext {
            merchants_level: 4, // MERCHANTS_BONUS[4] = 300
            ..Default::default()
        };
        // own_territory = true makes the merchant multiplier apply.
        let y = calc_rare(&r, 10, &one_good(RES_METAL, 5), &ctx, true);
        assert_eq!(y[RES_METAL], 240, "80 * 300 / 100, not 80 * 400 / 100");
    }

    #[test]
    fn a_fish_node_applies_the_merchant_multiplier_to_every_slot_but_food() {
        let r = shipped();
        let ctx = RareContext {
            merchants_level: 4,
            ..Default::default()
        };
        let good = GoodTypeYield {
            res_id: [RES_FOOD as i32, RES_WEALTH as i32],
            amount: [10, 10],
        };
        // good_type 6 is a fish type; own_territory false, so only the fish rule fires.
        let y = calc_rare(&r, 6, &good, &ctx, false);
        // Food keeps mult 100 but takes FISHERMEN_BONUS[0] = 0 -> (0 + 0 + 100)/100.
        assert_eq!(y[RES_FOOD], 160);
        // Wealth takes the merchant multiplier.
        assert_eq!(y[RES_WEALTH], 480, "160 * 300 / 100");
    }

    #[test]
    fn fishermen_bonus_is_food_only() {
        let r = shipped();
        let good = GoodTypeYield {
            res_id: [RES_FOOD as i32, RES_WEALTH as i32],
            amount: [10, 10],
        };
        let ctx = RareContext {
            fishing_level: 3, // 200%
            ..Default::default()
        };
        let y = calc_rare(&r, 6, &good, &ctx, false);
        assert_eq!(y[RES_FOOD], 160 * 300 / 100, "(200 + 0 + 100)% of 160");
        assert_eq!(y[RES_WEALTH], 160, "bonus_a never reaches slot 1..5");
    }

    #[test]
    fn an_untouched_slot_is_left_exactly_alone() {
        // The guard `bonus != 0 || mult > 100` exists so a zero-yield slot is not
        // round-tripped; with wrapping arithmetic that would still be zero, but the guard
        // is observable when a slot holds a value and every bonus is absent.
        let r = shipped();
        let ctx = RareContext::default();
        let good = GoodTypeYield {
            res_id: [RES_OIL as i32, -1],
            amount: [7, 0],
        };
        let y = calc_rare(&r, 10, &good, &ctx, false);
        assert_eq!(y[RES_OIL], 112, "7 * 16, untouched");
    }

    #[test]
    fn an_out_of_range_resource_id_disables_its_term() {
        let r = shipped();
        let ctx = RareContext::default();
        // The engine's guard is unsigned `< 6`, so -1 and 6 are both disabled.
        for bad in [-1, 6, 99] {
            let good = GoodTypeYield {
                res_id: [bad, -1],
                amount: [100, 0],
            };
            let y = calc_rare(&r, 10, &good, &ctx, false);
            assert_eq!(
                y, [0; NUM_RESOURCES],
                "res_id {bad} must contribute nothing"
            );
        }
    }

    // -- crowding --------------------------------------------------------------------------

    #[test]
    fn gatherers_share_a_node_by_integer_division() {
        let y = [100, 50, 3, 0, 7, 1];
        assert_eq!(share_among_gatherers(y, 0), y);
        assert_eq!(share_among_gatherers(y, 1), [50, 25, 1, 0, 3, 0]);
        assert_eq!(share_among_gatherers(y, 2), [33, 16, 1, 0, 2, 0]);
        // Three gatherers on a 100-yield node produce 99, not 100: the loss is real.
        let three: i32 = share_among_gatherers(y, 2)[0] * 3;
        assert_eq!(three, 99);
    }

    // -- per-city yield ----------------------------------------------------------------------

    #[test]
    fn a_plain_city_yields_city_gather_plus_taxes_and_literacy() {
        let r = shipped();
        let c = CityResourceInputs {
            taxes: 10,
            literacy: 3,
            ..Default::default()
        };
        let out = calc_city_resources(&r, Some(&c));
        assert_eq!(out[RES_FOOD], 10 * 16, "CITY_GATHER[food]");
        assert_eq!(out[RES_TIMBER], 10 * 16, "CITY_GATHER[timber]");
        assert_eq!(out[RES_WEALTH], 10 * 16, "taxes, shifted");
        assert_eq!(out[RES_KNOWLEDGE], 3 * 16, "literacy, shifted");
        assert_eq!(out[RES_METAL], 0, "CITY_GATHER[metal] ships as 0");
        assert_eq!(out[RES_OIL], 0);
    }

    #[test]
    fn the_census_contributes_no_knowledge_and_no_city_gather() {
        // The `city < 0` path returns immediately after VILLAGE_TAXES: knowledge only ever
        // enters through CityData::get_literacy, which is city-mode only.
        let r = shipped();
        let out = calc_city_resources(&r, None);
        assert_eq!(out[RES_KNOWLEDGE], 0);
        assert_eq!(out[RES_FOOD], 0, "CITY_GATHER is city-mode only");
        assert_eq!(out[RES_WEALTH], 0, "VILLAGE_TAXES ships as 0");

        let mut r2 = shipped();
        r2.set(804, 7); // village_taxes
        assert_eq!(calc_city_resources(&r2, None)[RES_WEALTH], 7 * 16);
        assert_eq!(calc_city_resources(&r2, None)[RES_KNOWLEDGE], 0);
    }

    #[test]
    fn the_forbidden_city_replaces_city_gather_rather_than_scaling_it() {
        let r = shipped();
        let plain = calc_city_resources(&r, Some(&CityResourceInputs::default()));
        let fc = calc_city_resources(
            &r,
            Some(&CityResourceInputs {
                forbidden_city: true,
                ..Default::default()
            }),
        );
        // CITY_GATHER[food] = 10 is replaced by FORBIDDEN_CITY_BASE_GATHER = 50.
        assert_eq!(plain[RES_FOOD], 160);
        assert_eq!(fc[RES_FOOD], 50 * 16);
        // ...and slots whose CITY_GATHER is zero stay zero: the rule-is-zero guard comes
        // first, so the override cannot introduce income where there was none.
        assert_eq!(fc[RES_METAL], 0);
    }

    #[test]
    fn the_forbidden_city_multiplier_hits_enhancer_income_not_city_gather() {
        // The 25% scale is applied *before* CITY_GATHER is added, so it only ever touches
        // the buildings' contribution. Ordering is the whole content of this test.
        let r = shipped();
        let mut c = CityResourceInputs {
            forbidden_city: true,
            ..Default::default()
        };
        c.enhancer_income[RES_METAL] = 400;
        let out = calc_city_resources(&r, Some(&c));
        assert_eq!(
            out[RES_METAL], 500,
            "400 * 125 / 100, no CITY_GATHER[metal]"
        );
    }

    #[test]
    fn the_hero_and_the_forbidden_city_compose_sequentially() {
        let r = shipped();
        let mut c = CityResourceInputs {
            forbidden_city: true,
            ceo_present: true,
            ..Default::default()
        };
        c.enhancer_income[RES_METAL] = 400;
        let out = calc_city_resources(&r, Some(&c));
        // 400 -> 500 -> 750, with truncation between. Not 400 * 175 / 100 = 700.
        assert_eq!(out[RES_METAL], 750);
    }

    #[test]
    fn german_city_gather_gates_only_the_metal_share() {
        let r = shipped();
        let base = CityResourceInputs {
            german: true,
            metal_available: false,
            ..Default::default()
        };
        let out = calc_city_resources(&r, Some(&base));
        assert_eq!(out[RES_FOOD], 160 + 5 * 16);
        assert_eq!(out[RES_TIMBER], 160 + 5 * 16);
        assert_eq!(out[RES_METAL], 0, "gated on type_avail");

        let out2 = calc_city_resources(
            &r,
            Some(&CityResourceInputs {
                metal_available: true,
                ..base
            }),
        );
        assert_eq!(out2[RES_METAL], 5 * 16);
    }

    #[test]
    fn roman_city_gather_is_wealth_only() {
        let r = shipped();
        let out = calc_city_resources(
            &r,
            Some(&CityResourceInputs {
                roman: true,
                ..Default::default()
            }),
        );
        assert_eq!(out[RES_WEALTH], 10 * 16);
        assert_eq!(out[RES_FOOD], 160, "CITY_GATHER only");
    }

    #[test]
    fn a_city_feeds_calc_gather_through_object_income() {
        // The integration the tech-cities lane was missing: per-city yield summed into
        // Leader::calc_gather's object_income.
        let r = shipped();
        let city = CityResourceInputs {
            taxes: 10,
            literacy: 4,
            ..Default::default()
        };
        let mut inp = GatherInputs::default();
        for _ in 0..3 {
            let y = calc_city_resources(&r, Some(&city));
            for i in 0..NUM_RESOURCES {
                inp.object_income[i] += y[i];
            }
        }
        let g = calc_gather(&r, &inp);
        assert_eq!(g.gross[RES_FOOD], 3 * 160, "three cities of CITY_GATHER");
        assert_eq!(g.gross[RES_KNOWLEDGE], 3 * 64);
    }

    // -- worker rates ---------------------------------------------------------------------

    #[test]
    fn worker_rate_unscales_the_8_8_constants() {
        let r = shipped();
        assert_eq!(worker_rate(&r, false), 10, "PEASANT_RATE 2560 / 256");
        assert_eq!(worker_rate(&r, true), 35, "OIL_RATE 8960 / 256");
    }

    #[test]
    fn scholar_rate_is_indexed_level_minus_one() {
        let r = shipped();
        // The table is 5, 7, 10, 15, 20 and the engine reads `[level - 1]`. Assert the
        // indexing on the raw accessor, where it is unambiguous.
        assert_eq!(r.scholar_rate(0), 5);
        assert_eq!(r.scholar_rate(4), 20);
        // Level 1 must select entry 0, not entry 1. If a port used `[level]` it would read
        // 7 here -- so pin the *selection*, separately from the questionable /16 unscale.
        let sel = |lvl: i32| r.scholar_rate((lvl - 1).max(0) as usize);
        assert_eq!(sel(1), 5);
        assert_eq!(sel(3), 10);
        assert_eq!(sel(5), 20);
    }

    #[test]
    fn scholar_rate_unscale_truncates_to_zero_and_we_do_not_paper_over_it() {
        // Documented unresolved: the engine computes `value * 16 >> 8` on a scale-1
        // constant, which truncates 5 to 0. This test exists so that a later "fix" has to
        // be a deliberate change with evidence, not a silent one.
        let r = shipped();
        assert_eq!(scholar_rate_for_level(&r, 1), 0, "5 * 16 / 256");
        assert_eq!(scholar_rate_for_level(&r, 5), 1, "20 * 16 / 256");
    }

    // -- resource bonuses -------------------------------------------------------------------

    #[test]
    fn global_prosperity_skips_knowledge() {
        let r = shipped();
        let gates = ResourceBonusGates {
            global_prosperity: true,
            ..Default::default()
        };
        let mut out = [100; NUM_RESOURCES];
        calc_resource_bonuses(&r, &gates, &mut out);
        assert_eq!(out[RES_FOOD], 125);
        assert_eq!(out[RES_KNOWLEDGE], 100, "knowledge is excluded");
        assert_eq!(out[RES_OIL], 125);
    }

    #[test]
    fn wonder_multipliers_compose_in_source_order() {
        let r = shipped();
        let gates = ResourceBonusGates {
            colossus: true, // wealth +30
            taj: true,      // wealth +100
            ..Default::default()
        };
        let mut out = [0; NUM_RESOURCES];
        out[RES_WEALTH] = 100;
        calc_resource_bonuses(&r, &gates, &mut out);
        // Sequential with truncation: 100 -> 130 -> 260. Not 100 * 230 / 100.
        assert_eq!(out[RES_WEALTH], 260);
    }

    #[test]
    fn hanging_gardens_is_flat_and_pre_shifted() {
        let r = shipped();
        let gates = ResourceBonusGates {
            hanging_gardens: true,
            ..Default::default()
        };
        let mut out = [0; NUM_RESOURCES];
        calc_resource_bonuses(&r, &gates, &mut out);
        assert_eq!(out[RES_KNOWLEDGE], 800, "50 << 4");
    }

    // -- calc_gather -------------------------------------------------------------------------

    #[test]
    fn a_bare_leader_gathers_the_baseline_only() {
        let r = shipped();
        let inp = GatherInputs::default();
        let g = calc_gather(&r, &inp);
        // BASIC_GATHER is all zeros in shipped data, so a leader with nothing gets nothing.
        assert_eq!(g.gross, [0; NUM_RESOURCES]);
    }

    #[test]
    fn basic_gather_is_shifted_into_sixteenths() {
        let mut r = shipped();
        r.set(588, 3); // basic_gather[food] = 3
        let g = calc_gather(&r, &GatherInputs::default());
        assert_eq!(g.gross[RES_FOOD], 48, "3 << 4");
    }

    #[test]
    fn americans_bonus_lands_on_food_timber_metal_and_wealth() {
        let r = shipped();
        let inp = GatherInputs {
            americans_buildings: Some(3),
            ..Default::default()
        };
        let g = calc_gather(&r, &inp);
        let v = 3 * 2 * 16; // count * AMERICANS_BARRACKS_GATHER * 16
        assert_eq!(g.gross[RES_FOOD], v);
        assert_eq!(g.gross[RES_TIMBER], v);
        assert_eq!(g.gross[RES_METAL], v);
        assert_eq!(g.gross[RES_WEALTH], v);
        assert_eq!(
            g.gross[RES_KNOWLEDGE], 0,
            "the XML text lists four slots, not six"
        );
        assert_eq!(g.gross[RES_OIL], 0);
    }

    #[test]
    fn an_unavailable_resource_drops_its_share_of_the_americans_bonus() {
        let r = shipped();
        let mut inp = GatherInputs {
            americans_buildings: Some(3),
            ..Default::default()
        };
        inp.type_avail[RES_METAL] = false;
        let g = calc_gather(&r, &inp);
        assert_eq!(g.gross[RES_METAL], 0);
        assert_eq!(g.gross[RES_FOOD], 96);
    }

    #[test]
    fn refineries_scale_oil_and_nothing_else() {
        let r = shipped();
        let mut inp = GatherInputs {
            refineries: 3,
            ..Default::default()
        };
        inp.object_income[RES_OIL] = 100;
        inp.object_income[RES_METAL] = 100;
        let g = calc_gather(&r, &inp);
        assert_eq!(g.gross[RES_OIL], 199, "100 * (3*33 + 100) / 100");
        assert_eq!(g.gross[RES_METAL], 100);
    }

    #[test]
    fn inca_wealth_from_metal_is_off_in_shipped_data() {
        let r = shipped();
        let mut inp = GatherInputs {
            inca: true,
            ..Default::default()
        };
        inp.object_income[RES_METAL] = 400;
        let g = calc_gather(&r, &inp);
        assert_eq!(g.gross[RES_WEALTH], 0, "gate is INCA_WEALTH_PER_MINER < 0");

        // Flip the rule negative and the transfer appears.
        let mut r2 = shipped();
        r2.set(1436, -10);
        let g2 = calc_gather(&r2, &inp);
        assert_eq!(g2.gross[RES_WEALTH], 400);
    }

    #[test]
    fn territory_taxes_scale_by_owned_fraction_of_the_map() {
        let r = shipped();
        let inp = GatherInputs {
            taxation_level: 2, // 100%
            territory_tiles: 500,
            total_land_tiles: 2000,
            ..Default::default()
        };
        let g = calc_gather(&r, &inp);
        // 500 * 100 * 16 / 2000
        assert_eq!(g.gross[RES_WEALTH], 400);
    }

    #[test]
    fn zero_land_tiles_disables_the_whole_tax_block() {
        let r = shipped();
        let inp = GatherInputs {
            taxation_level: 4,
            territory_tiles: 500,
            total_land_tiles: 0,
            ..Default::default()
        };
        // No panic, no income: the engine's `test eax,eax; je` guards the divide.
        assert_eq!(calc_gather(&r, &inp).gross[RES_WEALTH], 0);
    }

    #[test]
    fn british_taxation_doubles_the_territory_rate() {
        let r = shipped();
        let base = GatherInputs {
            taxation_level: 2,
            territory_tiles: 1000,
            total_land_tiles: 2000,
            ..Default::default()
        };
        let plain = calc_gather(&r, &base).gross[RES_WEALTH];
        let brit = calc_gather(
            &r,
            &GatherInputs {
                british: true,
                ..base.clone()
            },
        )
        .gross[RES_WEALTH];
        assert_eq!(brit, plain * 2, "BRITISH_TAXATION = 100%");
    }

    #[test]
    fn rares_are_counted_and_summed() {
        let r = shipped();
        let mut inp = GatherInputs::default();
        inp.rares[0] = true;
        inp.rares[5] = true;
        inp.rare_yields[0] = one_good(RES_METAL, 4);
        inp.rare_yields[5] = one_good(RES_METAL, 6);
        let g = calc_gather(&r, &inp);
        assert_eq!(g.gross[RES_METAL], 4 * 16 + 6 * 16);
        assert_eq!(g.rare_counts[0], 1);
        assert_eq!(g.rare_counts[5], 1);
        assert_eq!(g.rare_counts[1], 0);
    }

    #[test]
    fn rare_bit_zero_is_good_type_six_the_fish() {
        // Bit 0 must map to good type 6, which is what makes FISHERMEN_BONUS reachable
        // from the leader-level rare loop at all.
        let r = shipped();
        let mut inp = GatherInputs::default();
        inp.rares[0] = true;
        inp.rare_yields[0] = GoodTypeYield {
            res_id: [RES_WEALTH as i32, -1],
            amount: [10, 0],
        };
        inp.rare_ctx.merchants_level = 4; // 300%
        let g = calc_gather(&r, &inp);
        // own_territory is false in the leader loop, so only the fish branch can apply the
        // merchant multiplier -- and it does, because res_id != food.
        assert_eq!(g.gross[RES_WEALTH], 160 * 3);
    }

    #[test]
    fn coffee_lifts_every_slot_by_ten_percent() {
        let r = shipped();
        let mut inp = GatherInputs {
            coffee: true,
            ..Default::default()
        };
        inp.object_income = [100; NUM_RESOURCES];
        let g = calc_gather(&r, &inp);
        assert_eq!(g.gross, [110; NUM_RESOURCES]);
    }

    #[test]
    fn substitution_moves_an_unavailable_resource_at_its_8_8_rate() {
        let r = shipped();
        let mut inp = GatherInputs::default();
        inp.object_income[RES_OIL] = 1000;
        inp.type_avail[RES_OIL] = false;
        inp.has_preq[RES_OIL] = true;
        inp.substitution[RES_OIL] = Substitution {
            target: RES_WEALTH as i32,
            rate_8_8: 128, // 0.5
        };
        let g = calc_gather(&r, &inp);
        assert_eq!(g.gross[RES_OIL], 0, "the source slot is always zeroed");
        assert_eq!(g.gross[RES_WEALTH], 500);
    }

    #[test]
    fn substitution_with_a_negative_target_just_discards() {
        let r = shipped();
        let mut inp = GatherInputs::default();
        inp.object_income[RES_OIL] = 1000;
        inp.type_avail[RES_OIL] = false;
        inp.has_preq[RES_OIL] = true;
        // Default Substitution has target -1.
        let g = calc_gather(&r, &inp);
        assert_eq!(g.gross[RES_OIL], 0);
        assert_eq!(g.gross.iter().sum::<i32>(), 0);
    }

    #[test]
    fn substitution_needs_both_gates() {
        let r = shipped();
        let mut inp = GatherInputs::default();
        inp.object_income[RES_OIL] = 1000;
        inp.type_avail[RES_OIL] = false;
        inp.has_preq[RES_OIL] = false; // second gate missing
        inp.substitution[RES_OIL] = Substitution {
            target: 2,
            rate_8_8: 256,
        };
        assert_eq!(calc_gather(&r, &inp).gross[RES_OIL], 1000, "left alone");
    }

    // -- caps -------------------------------------------------------------------------------

    #[test]
    fn knowledge_cap_is_hardcoded_999_and_immune_to_every_bonus() {
        let r = shipped();
        let gates = CapGates {
            british: true,
            wonder_additive: [500; NUM_RESOURCES],
            ..Default::default()
        };
        let caps = calc_resource_caps(&r, 7, &gates);
        assert_eq!(caps[RES_KNOWLEDGE], 999);
        assert_ne!(caps[RES_FOOD], 999);
    }

    #[test]
    fn commerce_cap_follows_age() {
        let r = shipped();
        let g = CapGates::default();
        assert_eq!(calc_resource_caps(&r, 0, &g)[RES_FOOD], 70);
        assert_eq!(calc_resource_caps(&r, 4, &g)[RES_FOOD], 260);
        assert_eq!(calc_resource_caps(&r, 7, &g)[RES_FOOD], 500);
        assert_eq!(calc_resource_caps(&r, 99, &g)[RES_FOOD], 500, "clamped");
    }

    #[test]
    fn the_per_resource_civ_branches_are_mutually_exclusive() {
        let r = shipped();
        let gates = CapGates {
            british: true,
            inca: true,
            french: true,
            egyptian: true,
            ..Default::default()
        };
        let caps = calc_resource_caps(&r, 0, &gates);
        let british_only = 70 * 125 / 100; // 87
        assert_eq!(
            caps[RES_METAL], british_only,
            "slots 4 and 5 have no branch"
        );
        assert_eq!(caps[RES_OIL], british_only);
        assert_eq!(caps[RES_WEALTH], british_only * 133 / 100);
        assert_eq!(caps[RES_TIMBER], british_only * 110 / 100);
        assert_eq!(caps[RES_FOOD], british_only * 110 / 100);
    }

    // -- do_gather ----------------------------------------------------------------------------

    #[test]
    fn accumulator_period_is_gather_rate_times_sixteen() {
        assert_eq!(accumulator_period(&shipped()), 7200);
    }

    #[test]
    fn income_below_the_period_accrues_and_eventually_pays_out() {
        let r = shipped();
        let mut econ = LeaderEcon::new();
        econ.commerce_cap = [10_000; NUM_RESOURCES];
        econ.gross[RES_FOOD] = 720; // one tenth of the period per frame
        let ctx = DoGatherContext::default();

        let mut credited = 0;
        for _ in 0..9 {
            credited += do_gather(&r, &mut econ, &ctx)[RES_FOOD].whole;
        }
        assert_eq!(
            credited, 0,
            "nine frames of 720 is 6480, still short of 7200"
        );
        credited += do_gather(&r, &mut econ, &ctx)[RES_FOOD].whole;
        assert_eq!(credited, 1, "the tenth frame crosses");
        assert_eq!(econ.stockpile[RES_FOOD], 1);
        assert_eq!(econ.accumulator[RES_FOOD], 0);
    }

    #[test]
    fn the_carry_loop_conserves_every_sixteenth() {
        // The property that matters: the accumulator loses nothing. Over N frames the
        // stockpile plus the leftover accumulator must account for exactly N * income.
        // Stated as an invariant rather than a hand-computed schedule, because a
        // hand-computed expectation is how this project has been burned before.
        let r = shipped();
        let period = accumulator_period(&r);
        for income in [1, 7199, 7200, 7201, 20_000] {
            let mut econ = LeaderEcon::new();
            econ.commerce_cap = [i32::MAX; NUM_RESOURCES];
            econ.gross[RES_FOOD] = income;
            let ctx = DoGatherContext::default();
            for _ in 0..97 {
                do_gather(&r, &mut econ, &ctx);
            }
            let accounted = econ.stockpile[RES_FOOD] * period + econ.accumulator[RES_FOOD];
            assert_eq!(accounted, 97 * income, "income {income} lost a sixteenth");
            assert!(econ.accumulator[RES_FOOD] < period);
        }
    }

    #[test]
    fn payouts_are_not_evenly_spaced_when_income_does_not_divide_the_period() {
        // Folding the carry into the division would make the schedule regular. It is not:
        // with income one short of the period, the first frame pays nothing and every
        // later frame pays one, so the schedule is offset. That offset is observable and
        // is why the loop is a loop.
        let r = shipped();
        let mut econ = LeaderEcon::new();
        econ.commerce_cap = [i32::MAX; NUM_RESOURCES];
        econ.gross[RES_FOOD] = accumulator_period(&r) - 1;
        let ctx = DoGatherContext::default();
        let paid: Vec<i32> = (0..4)
            .map(|_| do_gather(&r, &mut econ, &ctx)[RES_FOOD].whole)
            .collect();
        assert_eq!(paid[0], 0, "the first frame is short by one sixteenth");
        assert!(paid[1..].iter().all(|&p| p == 1));
    }

    #[test]
    fn negative_income_abandons_the_resource_without_touching_the_stockpile() {
        let r = shipped();
        let mut econ = LeaderEcon::new();
        econ.stockpile[RES_FOOD] = 500;
        econ.accumulator[RES_FOOD] = 4000;
        econ.commerce_cap = [10_000; NUM_RESOURCES];
        econ.gross[RES_FOOD] = 100;
        econ.expense[RES_FOOD] = 900;
        let out = do_gather(&r, &mut econ, &DoGatherContext::default());
        assert_eq!(out[RES_FOOD].whole, 0);
        assert_eq!(out[RES_FOOD].displayed, -800);
        assert_eq!(econ.stockpile[RES_FOOD], 500, "untouched");
        assert_eq!(econ.accumulator[RES_FOOD], 4000, "untouched");
        assert_eq!(econ.capped_flag[RES_FOOD], 0);
    }

    #[test]
    fn income_is_clamped_to_the_commerce_cap_and_the_flag_records_which_cap() {
        let r = shipped();
        let mut econ = LeaderEcon::new();
        econ.commerce_cap = [70; NUM_RESOURCES];
        econ.gross[RES_FOOD] = 5000;
        do_gather(&r, &mut econ, &DoGatherContext::default());
        assert_eq!(econ.displayed[RES_FOOD], 70);
        assert_eq!(econ.capped_flag[RES_FOOD], 1);

        econ.commerce_cap[RES_FOOD] = 0x3E70; // > 0x3E6F
        econ.gross[RES_FOOD] = 100_000;
        do_gather(&r, &mut econ, &DoGatherContext::default());
        assert_eq!(econ.capped_flag[RES_FOOD], 2);
    }

    #[test]
    fn the_16000_ceiling_only_exists_on_the_dutch_path() {
        let r = shipped();
        let mut econ = LeaderEcon::new();
        econ.commerce_cap = [1_000_000; NUM_RESOURCES];
        econ.gross[RES_FOOD] = 100_000;

        let plain = do_gather(&r, &mut econ, &DoGatherContext::default());
        assert_eq!(
            plain[RES_FOOD].displayed, 100_000,
            "no ceiling without Dutch"
        );

        let mut econ2 = LeaderEcon::new();
        econ2.commerce_cap = [1_000_000; NUM_RESOURCES];
        econ2.gross[RES_FOOD] = 100_000;
        let dutch = do_gather(
            &r,
            &mut econ2,
            &DoGatherContext {
                dutch: true,
                ..Default::default()
            },
        );
        assert_eq!(dutch[RES_FOOD].displayed, HARD_INCOME_CEILING);
    }

    #[test]
    fn knowledge_never_takes_the_dutch_path() {
        let r = shipped();
        let mut econ = LeaderEcon::new();
        econ.commerce_cap = [1_000_000; NUM_RESOURCES];
        econ.gross[RES_KNOWLEDGE] = 100_000;
        let out = do_gather(
            &r,
            &mut econ,
            &DoGatherContext {
                dutch: true,
                ..Default::default()
            },
        );
        assert_eq!(
            out[RES_KNOWLEDGE].displayed, 100_000,
            "no ceiling on knowledge"
        );
    }

    #[test]
    fn the_dutch_interest_clamp_shifts_only_the_rule() {
        // economy.md said `(cap + DUTCH_INTEREST_CAP) << 4`; retail shifts the rule alone.
        // With cap 70 the limit is 50*16 + 70 = 870, not (50+70)*16 = 1920.
        let r = shipped();
        let mut econ = LeaderEcon::new();
        econ.commerce_cap = [70; NUM_RESOURCES];
        econ.stockpile[RES_FOOD] = 1_000_000;
        econ.gross[RES_FOOD] = 70;
        let out = do_gather(
            &r,
            &mut econ,
            &DoGatherContext {
                dutch: true,
                ..Default::default()
            },
        );
        assert_eq!(out[RES_FOOD].displayed, 870);
    }

    #[test]
    fn knowledge_difficulty_penalty_has_two_tiers() {
        let r = shipped();
        let run = |difficulty: u8| {
            let mut econ = LeaderEcon::new();
            econ.commerce_cap = [1_000_000; NUM_RESOURCES];
            econ.gross[RES_KNOWLEDGE] = 72_000; // ten periods
            let out = do_gather(
                &r,
                &mut econ,
                &DoGatherContext {
                    difficulty,
                    ..Default::default()
                },
            );
            out[RES_KNOWLEDGE].whole
        };
        assert_eq!(run(4), 10, "difficulty 4 is unpenalised");
        assert_eq!(run(5), 7, "x3/4 -> 54000/7200");
        assert_eq!(run(6), 7);
        assert_eq!(run(7), 5, "/2 -> 36000/7200");
    }

    #[test]
    fn the_display_value_is_taken_before_the_handicap() {
        let r = shipped();
        let mut econ = LeaderEcon::new();
        econ.commerce_cap = [1_000_000; NUM_RESOURCES];
        econ.gross[RES_FOOD] = 7200;
        let out = do_gather(
            &r,
            &mut econ,
            &DoGatherContext {
                gather_handicap: 100,
                ..Default::default()
            },
        );
        assert_eq!(out[RES_FOOD].displayed, 7200, "display is pre-handicap");
        assert_eq!(out[RES_FOOD].whole, 2, "payout is post-handicap");
    }

    #[test]
    fn an_unavailable_resource_is_skipped_entirely() {
        let r = shipped();
        let mut econ = LeaderEcon::new();
        econ.commerce_cap = [1_000_000; NUM_RESOURCES];
        econ.gross = [72_000; NUM_RESOURCES];
        let mut ctx = DoGatherContext::default();
        ctx.type_avail[RES_OIL] = false;
        let out = do_gather(&r, &mut econ, &ctx);
        assert_eq!(out[RES_OIL].whole, 0);
        assert_eq!(econ.stockpile[RES_OIL], 0);
        assert_eq!(
            econ.displayed[RES_OIL], 0,
            "not even the display is written"
        );
        assert_eq!(econ.stockpile[RES_FOOD], 10);
    }

    #[test]
    fn leader_gather_zeroes_expenses_every_frame() {
        let r = shipped();
        let mut econ = LeaderEcon::new();
        econ.expense = [999; NUM_RESOURCES];
        let mut last = 0;
        let mut dirty = false;
        leader_gather(
            &r,
            &mut econ,
            1,
            0,
            &mut last,
            &mut dirty,
            &GatherInputs::default(),
            &CapGates::default(),
            &DoGatherContext::default(),
        );
        assert_eq!(econ.expense, [0; NUM_RESOURCES]);
    }

    #[test]
    fn leader_gather_recomposes_gross_only_when_due_and_clears_the_dirty_bit() {
        let r = shipped();
        let mut econ = LeaderEcon::new();
        econ.gross = [12_345; NUM_RESOURCES];
        let mut last = -10_000;
        let mut dirty = true;
        let inp = GatherInputs::default();

        // Frame 3 is not a multiple of 8: not due, gross survives.
        leader_gather(
            &r,
            &mut econ,
            3,
            0,
            &mut last,
            &mut dirty,
            &inp,
            &CapGates::default(),
            &DoGatherContext::default(),
        );
        assert_eq!(econ.gross[RES_FOOD], 12_345);
        assert!(dirty);

        // Frame 8 is due: gross is recomposed to the baseline (zero) and dirty clears.
        leader_gather(
            &r,
            &mut econ,
            8,
            0,
            &mut last,
            &mut dirty,
            &inp,
            &CapGates::default(),
            &DoGatherContext::default(),
        );
        assert_eq!(econ.gross[RES_FOOD], 0);
        assert!(!dirty);
        assert_eq!(last, 8);
    }

    // -- market -----------------------------------------------------------------------------

    #[test]
    fn calc_market_draws_exactly_three_times_with_shipped_rules() {
        let r = shipped();
        let mut m = MarketState {
            base_price: [65; NUM_RESOURCES],
            ..Default::default()
        };
        let mut rng = Random::new(12345);
        let before = rng.state();
        calc_market(&r, &mut m, &mut rng, 0);
        // Replay the same number of advances on a fresh generator and compare states.
        let mut probe = Random::new(before);
        for _ in 0..3 {
            probe.get(0, 0xFFFF);
        }
        assert_eq!(rng.state(), probe.state(), "exactly three draws");
    }

    #[test]
    fn calc_market_draws_only_once_when_variance_collapses() {
        // variance = (max(base/2, MIN_VARIANCE) + 1)/2; force MIN_VARIANCE to 0 and a base
        // price of 0 and the two variance draws disappear.
        let mut r = shipped();
        r.set(3296, 0); // market_min_variance
        let mut m = MarketState::default();
        let mut rng = Random::new(7);
        let before = rng.state();
        calc_market(&r, &mut m, &mut rng, 0);
        let mut probe = Random::new(before);
        probe.get(0, 0xFFFF); // only the trend-range draw
        assert_eq!(rng.state(), probe.state(), "exactly one draw");
        assert_eq!(m.trend_target[0], -1, "b - (v+1) + a with v = 0");
    }

    #[test]
    fn a_zero_trend_range_removes_the_third_draw() {
        let mut r = shipped();
        r.set(3304, 1); // market_trend_range = 1 -> `range - 1 > 0` is false
        let mut m = MarketState {
            base_price: [65; NUM_RESOURCES],
            ..Default::default()
        };
        let mut rng = Random::new(99);
        let before = rng.state();
        calc_market(&r, &mut m, &mut rng, 0);
        let mut probe = Random::new(before);
        probe.get(0, 0xFFFF);
        probe.get(0, 0xFFFF);
        assert_eq!(rng.state(), probe.state(), "two draws, not three");
        assert_eq!(m.trend_countdown[0], 8, "MARKET_MIN_TREND with no jitter");
    }

    #[test]
    fn the_trend_step_reaches_its_target_in_exactly_duration_cycles() {
        // This is what the round-away-from-zero divide buys.
        let _r = shipped();
        for (delta, duration) in [(7i32, 8i32), (-7, 8), (1, 16), (-1, 16), (100, 9)] {
            let step = {
                let sign = if delta > 0 { 1 } else { delta >> 31 };
                ((duration - 1) * sign + delta) / duration
            };
            let travelled = step * duration;
            assert!(
                travelled.abs() >= delta.abs(),
                "delta {delta} over {duration} cycles must not undershoot (step {step})"
            );
        }
    }

    #[test]
    fn the_base_price_walks_toward_equilibrium_every_256_cycles() {
        let r = shipped();
        let mut m = MarketState {
            base_price: [200; NUM_RESOURCES],
            ..Default::default()
        };
        let mut rng = Random::new(1);
        // Cycle 0 always services every resource and always rolls a trend.
        calc_markets(&r, &mut m, &mut rng, 0);
        assert_eq!(m.cycle, 1);
        // 200 > 65, so slot 0 stepped down by 200/65 = 3.
        assert_eq!(m.base_price[0], 197);
    }

    #[test]
    fn a_low_price_climbs_by_two_while_under_the_basement() {
        let r = shipped();
        let mut m = MarketState::default(); // base 0, below MARKET_BASEMENT = 10
        let mut rng = Random::new(1);
        calc_markets(&r, &mut m, &mut rng, 0);
        assert_eq!(m.base_price[0], 2, "0 + 1 = 1 < 10, so 0 + 2");

        // At 9, +1 = 10 which is not < 10, so it goes up by one only.
        let mut m2 = MarketState {
            base_price: [9; NUM_RESOURCES],
            ..Default::default()
        };
        let mut rng2 = Random::new(1);
        calc_markets(&r, &mut m2, &mut rng2, 0);
        assert_eq!(m2.base_price[0], 10);
    }

    #[test]
    fn resources_are_serviced_on_an_eight_cycle_stagger() {
        let r = shipped();
        let mut m = MarketState {
            base_price: [65; NUM_RESOURCES],
            trend_countdown: [1; NUM_RESOURCES],
            ..Default::default()
        };
        let mut rng = Random::new(4);
        calc_markets(&r, &mut m, &mut rng, 0); // cycle 0 -> everything
                                               // Cycles 1..7 touch nothing: (cycle + res) & 7 == 0 needs cycle + res in {8, 16, ...}
        let before = m;
        for f in 1..8 {
            calc_markets(&r, &mut m, &mut rng, f);
        }
        assert_eq!(m.base_price, before.base_price);
        assert_eq!(m.cycle, 8);
        // At cycle 8 slot 0 is serviced again.
        let spread0 = m.spread[0];
        calc_markets(&r, &mut m, &mut rng, 8);
        assert_ne!(
            (m.spread[0], m.trend_countdown[0]),
            (spread0, before.trend_countdown[0]),
            "slot 0 moved at cycle 8"
        );
    }

    #[test]
    fn market_cycle_rate_gates_the_whole_tick() {
        let mut r = shipped();
        r.set(3308, 5); // market_cycle_rate = 5 frames
        let mut m = MarketState::default();
        let mut rng = Random::new(1);
        calc_markets(&r, &mut m, &mut rng, 3);
        assert_eq!(m.cycle, 0, "frame 3 is not a multiple of 5");
        calc_markets(&r, &mut m, &mut rng, 5);
        assert_eq!(m.cycle, 1);
    }

    #[test]
    fn a_market_run_is_deterministic_for_a_given_seed() {
        let r = shipped();
        let run = || {
            let mut m = MarketState {
                base_price: [65; NUM_RESOURCES],
                ..Default::default()
            };
            let mut rng = Random::new(0x1234_5678);
            for f in 0..2000 {
                calc_markets(&r, &mut m, &mut rng, f);
            }
            (m, rng.state(), market_adler32(&m))
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn market_prices_keep_a_ten_wide_spread() {
        let r = shipped();
        let g = MarketPriceGates::default();
        for base in [0, 1, 10, 65, 200, 1000] {
            let m = MarketState {
                base_price: [base; NUM_RESOURCES],
                ..Default::default()
            };
            let p = calc_market_prices(&r, &m, 0, &g);
            assert!(p.buy >= p.sell + 10, "base {base}: {p:?}");
            assert!(p.sell >= 1);
            assert!(p.buy >= 2 * r.market_basement());
        }
    }

    #[test]
    fn nubian_takes_precedence_over_amber() {
        let r = shipped();
        let m = MarketState {
            base_price: [65; NUM_RESOURCES],
            ..Default::default()
        };
        let both = calc_market_prices(
            &r,
            &m,
            0,
            &MarketPriceGates {
                nubian: true,
                amber: true,
                ..Default::default()
            },
        );
        let nub = calc_market_prices(
            &r,
            &m,
            0,
            &MarketPriceGates {
                nubian: true,
                ..Default::default()
            },
        );
        assert_eq!(both, nub, "the amber branch is the `else`");
        assert_eq!(nub.sell, 65 + 20);
        assert_eq!(nub.buy, 130 - 20);
    }

    #[test]
    fn russian_communism_is_inert_in_shipped_data() {
        let r = shipped();
        let m = MarketState {
            base_price: [65; NUM_RESOURCES],
            ..Default::default()
        };
        let g = MarketPriceGates {
            russian: true,
            age: 7,
            ..Default::default()
        };
        let p = calc_market_prices(&r, &m, 0, &g);
        assert_ne!(
            p.buy, 100,
            "RUSSIAN_COMMUNISM is 0, so the branch never fires"
        );

        let mut r2 = shipped();
        r2.set(1880, 1);
        assert_eq!(
            calc_market_prices(&r2, &m, 0, &g),
            MarketPrices {
                buy: 100,
                sell: 100
            }
        );
    }

    #[test]
    fn buying_costs_wealth_and_pushes_the_price_up() {
        let r = shipped();
        let mut m = MarketState {
            base_price: [65; NUM_RESOURCES],
            ..Default::default()
        };
        let mut econ = LeaderEcon::new();
        econ.stockpile[RES_WEALTH] = 1000;
        let mut demand = 500;
        let res = RES_TIMBER;
        let price = calc_market_prices(&r, &m, res, &MarketPriceGates::default()).buy;
        assert_eq!(
            do_buy(
                &r,
                &mut m,
                &mut econ,
                &mut demand,
                res,
                &MarketPriceGates::default()
            ),
            TradeResult::Done
        );
        assert_eq!(econ.stockpile[RES_WEALTH], 1000 - price);
        assert_eq!(econ.stockpile[res], 100);
        assert_eq!(m.base_price[res], 68, "+MARKET_SUPPLY_DEMAND");
        assert_eq!(demand, 400);
    }

    #[test]
    fn a_refused_trade_changes_nothing_including_the_price() {
        let r = shipped();
        let mut m = MarketState {
            base_price: [65; NUM_RESOURCES],
            ..Default::default()
        };
        let before = m;
        let mut econ = LeaderEcon::new(); // no wealth, no stock
        let mut c = 0;
        assert_eq!(
            do_buy(
                &r,
                &mut m,
                &mut econ,
                &mut c,
                RES_TIMBER,
                &MarketPriceGates::default()
            ),
            TradeResult::Refused
        );
        assert_eq!(
            do_sell(
                &r,
                &mut m,
                &mut econ,
                &mut c,
                RES_TIMBER,
                &MarketPriceGates::default()
            ),
            TradeResult::Refused
        );
        assert_eq!(m, before);
        assert_eq!(econ, LeaderEcon::new());
    }

    #[test]
    fn a_buy_sell_round_trip_loses_wealth() {
        // The `buy >= sell + 10` floor is a house edge; this is the mechanic that makes
        // market arbitrage a losing strategy at every price level.
        let r = shipped();
        let mut m = MarketState {
            base_price: [65; NUM_RESOURCES],
            ..Default::default()
        };
        let mut econ = LeaderEcon::new();
        econ.stockpile[RES_WEALTH] = 10_000;
        let start = econ.stockpile[RES_WEALTH];
        let mut c = 0;
        let g = MarketPriceGates::default();
        do_buy(&r, &mut m, &mut econ, &mut c, RES_TIMBER, &g);
        do_sell(&r, &mut m, &mut econ, &mut c, RES_TIMBER, &g);
        assert_eq!(econ.stockpile[RES_TIMBER], 0, "back to zero stock");
        assert!(econ.stockpile[RES_WEALTH] < start, "round trip is lossy");
    }

    #[test]
    fn selling_floors_the_base_price_at_zero() {
        let r = shipped();
        let mut m = MarketState {
            base_price: [1; NUM_RESOURCES],
            ..Default::default()
        };
        let mut econ = LeaderEcon::new();
        econ.stockpile[RES_TIMBER] = 100;
        let mut c = 0;
        do_sell(
            &r,
            &mut m,
            &mut econ,
            &mut c,
            RES_TIMBER,
            &MarketPriceGates::default(),
        );
        assert_eq!(m.base_price[RES_TIMBER], 0, "1 - 3 clamped to 0");
    }

    // -- taxes, tribute, caravans ---------------------------------------------------------

    #[test]
    fn taxation_level_takes_the_highest_tech() {
        assert_eq!(taxation_level(false, false, false, false), 0);
        assert_eq!(taxation_level(false, false, false, true), 1);
        assert_eq!(taxation_level(false, true, true, true), 3);
        assert_eq!(taxation_level(true, true, true, true), 4);
    }

    #[test]
    fn city_taxes_are_market_only_in_shipped_data() {
        let r = shipped();
        assert_eq!(
            city_taxes(
                &r,
                &CityTaxInputs {
                    num_buildings: 12,
                    ..Default::default()
                }
            ),
            0,
            "VILLAGE_TAXES and BUILDING_TAXES both ship as 0"
        );
        assert_eq!(
            city_taxes(
                &r,
                &CityTaxInputs {
                    has_market: true,
                    ..Default::default()
                }
            ),
            10
        );
        assert_eq!(
            city_taxes(
                &r,
                &CityTaxInputs {
                    has_market: true,
                    porcelain: true,
                    ..Default::default()
                }
            ),
            40,
            "10 * 400 / 100"
        );
    }

    #[test]
    fn tribute_rounding_has_three_regimes() {
        let r = shipped();
        // age 0 -> 51%.
        assert_eq!(scale_tribute(&r, 0, 19), 19 * 51 / 100, "truncate under 20");
        assert_eq!(scale_tribute(&r, 0, 19), 9);
        assert_eq!(
            scale_tribute(&r, 0, 20),
            (20 * 51 + 50) / 100,
            "nearest 20..99"
        );
        assert_eq!(scale_tribute(&r, 0, 20), 10);
        assert_eq!(
            scale_tribute(&r, 0, 100),
            (100 * 51 + 99) / 100,
            "ceiling from 100"
        );
        assert_eq!(scale_tribute(&r, 0, 100), 51);
    }

    #[test]
    fn tribute_becomes_lossless_at_age_seven_and_free_after() {
        let r = shipped();
        // 51 + 7*age; age 7 -> exactly 100, age 8 -> 107 -> early-out, unchanged.
        assert_eq!(scale_tribute(&r, 7, 1000), 1000);
        assert_eq!(scale_tribute(&r, 8, 1000), 1000);
        assert!(scale_tribute(&r, 0, 1000) < 1000);
    }

    #[test]
    fn caravan_limit_is_age_plus_one_until_the_city_pair_cap_bites() {
        let r = shipped();
        let g = CaravanGates::default();
        assert_eq!(caravan_limit(&r, 0, &g, 0, None), 1);
        assert_eq!(caravan_limit(&r, 5, &g, 0, None), 6);
        assert_eq!(caravan_limit(&r, 200, &g, 0, None), 99, "hard ceiling");
        // Three cities support C(3,2) = 3 routes.
        assert_eq!(caravan_limit(&r, 5, &g, 0, Some(3)), 3);
        assert_eq!(caravan_limit(&r, 5, &g, 0, Some(2)), 1);
        assert_eq!(caravan_limit(&r, 5, &g, 0, Some(1)), 0);
        // With enough cities the age limit is the binding one again.
        assert_eq!(caravan_limit(&r, 5, &g, 0, Some(10)), 6);
    }

    #[test]
    fn nubian_caravan_bonus_applies() {
        let r = shipped();
        let g = CaravanGates {
            nubian: true,
            ..Default::default()
        };
        assert_eq!(
            caravan_limit(&r, 3, &g, 0, None),
            5,
            "3 + 1 + NUBIAN_CARAVAN_LIMIT"
        );
    }

    // -- checksum ---------------------------------------------------------------------------

    #[test]
    fn adler32_matches_known_vectors() {
        assert_eq!(adler32(1, b""), 1);
        assert_eq!(adler32(1, b"a"), 0x0062_0062);
        assert_eq!(adler32(1, b"abc"), 0x024D_0127);
        assert_eq!(adler32(1, b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn adler32_handles_buffers_past_the_nmax_boundary() {
        let big = vec![0xFFu8; 5552 * 2 + 17];
        // Reference: the naive one-pass modular form.
        let (mut s1, mut s2) = (1u64, 0u64);
        for &b in &big {
            s1 = (s1 + b as u64) % 65521;
            s2 = (s2 + s1) % 65521;
        }
        assert_eq!(adler32(1, &big), ((s2 << 16) | s1) as u32);
    }

    #[test]
    fn the_econ_image_is_obfuscated_exactly_as_the_engine_stores_it() {
        let mut e = LeaderEcon::new();
        e.stockpile[RES_WEALTH] = 1234;
        e.age = 5;
        let img = e.image();
        let read = |off: usize| u32::from_le_bytes(img[off..off + 4].try_into().unwrap());
        assert_eq!(
            read(econ_offsets::STOCKPILE + 8) ^ obfuscation::STOCKPILE,
            1234
        );
        assert_eq!(read(econ_offsets::AGE) ^ obfuscation::AGE, 5);
        // A zero field is NOT zero in the image: this is the whole point.
        assert_eq!(read(econ_offsets::STOCKPILE), obfuscation::STOCKPILE);
        assert_ne!(read(econ_offsets::GROSS), 0);
    }

    #[test]
    fn the_checksum_notices_a_one_unit_stockpile_difference() {
        let a = LeaderEcon::new();
        let mut b = LeaderEcon::new();
        b.stockpile[RES_OIL] = 1;
        assert_ne!(a.adler32(), b.adler32());
    }

    #[test]
    fn the_leaders_channel_is_order_sensitive() {
        let mut a = LeaderEcon::new();
        a.stockpile[RES_FOOD] = 10;
        let mut b = LeaderEcon::new();
        b.stockpile[RES_FOOD] = 20;
        assert_ne!(leaders_channel(&[a, b]), leaders_channel(&[b, a]));
    }

    #[test]
    fn the_goods_channel_is_order_sensitive() {
        let a = GoodNode {
            good_type: 6,
            x: 1,
            y: 2,
            remaining: 100,
            flags: 1,
        };
        let b = GoodNode {
            good_type: 7,
            x: 3,
            y: 4,
            remaining: 50,
            flags: 0,
        };
        assert_ne!(goods_channel(&[a, b]), goods_channel(&[b, a]));
        assert_eq!(goods_channel(&[a, b]), goods_channel(&[a, b]));
    }

    // -- an end-to-end run --------------------------------------------------------------------

    #[test]
    fn a_full_economy_run_is_deterministic_and_moves_state() {
        let r = shipped();
        let mut econ = LeaderEcon::new();
        econ.age = 2;
        let mut market = MarketState {
            base_price: [65; NUM_RESOURCES],
            ..Default::default()
        };
        let mut rng = Random::new(0xDEAD_BEEFu32 as i32);

        let mut inp = GatherInputs::default();
        inp.object_income = [3000, 2500, 400, 900, 1200, 0];
        inp.taxation_level = 2;
        inp.territory_tiles = 300;
        inp.total_land_tiles = 4000;
        inp.refineries = 1;

        let ctx = DoGatherContext::default();
        let caps = CapGates::default();
        let mut last = -10_000;
        let mut dirty = true;

        for frame in 0..600 {
            calc_markets(&r, &mut market, &mut rng, frame);
            leader_gather(
                &r, &mut econ, frame, 0, &mut last, &mut dirty, &inp, &caps, &ctx,
            );
        }

        // The first payout-bearing frame is frame 8, not frame 0: `calc_gather` is on the
        // dirty schedule (every 8 frames) and frame 0 is excluded, so gross is still zero
        // for frames 0..7. That eight-frame lead-in is a real property of the scheduler
        // and it is exactly the kind of thing a "600 * income / period" expectation gets
        // wrong -- so assert the invariant against the number of income-bearing frames.
        let earning_frames = 600 - 8;
        assert_eq!(econ.commerce_cap[RES_FOOD], 150);
        assert_eq!(econ.capped_flag[RES_FOOD], 1);
        assert_eq!(econ.stockpile[RES_FOOD], earning_frames * 150 / 7200);
        assert_eq!(
            econ.stockpile[RES_KNOWLEDGE],
            earning_frames * 900 / 7200,
            "under its 999 cap"
        );
        assert_eq!(econ.stockpile[RES_OIL], 0, "no oil income");

        let fingerprint = (
            leaders_channel(&[econ]),
            market_adler32(&market),
            rng.state(),
        );

        // Re-run and demand the same bits.
        let mut econ2 = LeaderEcon::new();
        econ2.age = 2;
        let mut market2 = MarketState {
            base_price: [65; NUM_RESOURCES],
            ..Default::default()
        };
        let mut rng2 = Random::new(0xDEAD_BEEFu32 as i32);
        let mut last2 = -10_000;
        let mut dirty2 = true;
        for frame in 0..600 {
            calc_markets(&r, &mut market2, &mut rng2, frame);
            leader_gather(
                &r,
                &mut econ2,
                frame,
                0,
                &mut last2,
                &mut dirty2,
                &inp,
                &caps,
                &ctx,
            );
        }
        assert_eq!(
            fingerprint,
            (
                leaders_channel(&[econ2]),
                market_adler32(&market2),
                rng2.state()
            )
        );
    }
}
