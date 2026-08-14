//! Victory conditions, scoring, diplomacy state and match lifecycle.
//!
//! Lane: `mech:victory-score`. Everything here is a port of named functions in
//! `ron-bin/sbl/rise.pdb`; every constant carries its source address in a comment.
//! Structure came from the instruction stream (`re/decomp-all/*.c` cross-checked
//! against capstone disassembly); rule values came from `ron-data/rules.xml` via
//! `docs/derivation/rules-constants.json`. Nothing here is from community docs.
//!
//! The engine functions this file reproduces:
//!
//! | VA | symbol | here |
//! |---|---|---|
//! | `0x006ec560` | `Leader::compute_score(int)` | [`Leaders::compute_score`] |
//! | `0x006bc500` | `Leader::compute_unit_score` | [`Leaders::compute_unit_score`] |
//! | `0x006bc3f0` | `Leader::compute_build_score` | [`Leaders::compute_build_score`] |
//! | `0x006bc270` | `Leader::compute_economy_score` | [`Leaders::compute_economy_score`] |
//! | `0x006bc1a0` | `Leader::compute_unit_upgrades_score` | [`Leaders::compute_unit_upgrades_score`] |
//! | `0x006bc360` | `Leader::compute_research_score` | [`Leaders::compute_research_score`] |
//! | `0x006bc190` | `Leader::compute_pop_score` | stub in retail: writes 0 |
//! | `0x006bc5e0` | `Leader::compute_explore_score` | stub in retail: writes 0 |
//! | `0x006bc250` | `Leader::compute_combat_score` | empty `ret` in retail |
//! | `0x006bc260` | `Leader::compute_wonder_score` | empty `ret` in retail |
//! | `0x006673b0` | `TypeData::get_score_value(int)` | [`TypeTable::get_score_value`] |
//! | `0x006d6520` | `LeaderData::get_team_score` | [`Leaders::get_team_score`] |
//! | `0x006d5de0` | `LeaderData::get_mvp_score` | [`Leaders::get_mvp_score`] |
//! | `0x006d62e0` | `LeaderData::get_team_terr` | [`Leaders::get_team_terr`] |
//! | `0x006d6400` | `LeaderData::get_team_economic` | [`Leaders::get_team_economic`] |
//! | `0x00594020` | `Game::get_armageddon` | [`Match::get_armageddon`] |
//! | `0x005944f0` | `Game::wonder_timer` | [`Match::wonder_timer`] |
//! | `0x005944b0` | `Game::popwin_timer` | [`Match::popwin_timer`] |
//! | `0x00594530` | `Game::retake_capital` | [`Match::retake_capital`] |
//! | `0x005948a0` | `Game::wonder_winning` | [`Leaders::wonder_winning`] |
//! | `0x00594950` | `Game::is_victory_timer` | [`Leaders::is_victory_timer`] |
//! | `0x005926b0` | `Game::check_victory` | [`Leaders::check_victory`] |
//! | `0x00592aa0` | `Game::defeat_all` | [`Leaders::defeat_all`] |
//! | `0x00730ef0` | `GameDaemon::process_victory` | [`Leaders::process_victory`] |
//! | `0x006ec9b0` | `Leader::victory(int,int)` | [`Leaders::victory`] |
//! | `0x006ecb00` | `Leader::defeat(int,int,int)` | [`Leaders::defeat`] + live cleanup request |
//! | `0x006b8a20` | `Leader::process_elimination` | [`Leaders::process_elimination`] |
//! | `0x006eba50` | `LeaderData::get_diplo` | [`Leaders::get_diplo`] |
//! | `0x006ebaa0` | `LeaderData::is_enemy` | [`Leaders::is_enemy`] |
//! | `0x006edb50` | `LeaderData::is_ally` | [`Leaders::is_ally`] |
//! | `0x006e1200` | `LeaderData::is_peace` | [`Leaders::is_peace`] |
//!
//! # Checksum channel
//!
//! Every field this module owns lives inside **`LeaderData::walk_data` (`0x006d6750`)**,
//! which `CheckSums::check_all` (`0x00936560`) inlines as its **8th channel**, plus
//! `Game::walk_data` (`0x00589600`), which walks the byte range `[0x550, 0x6E4)` of
//! `Game` and therefore covers `frame`, `tick`, `musical_chairs` and `armageddon`.
//! [`Leaders::walk_bytes`] / [`Match::walk_bytes`] emit those ranges in engine field
//! order so a harness can adler32 them the way the engine does.

#![allow(clippy::needless_range_loop)]

// ---------------------------------------------------------------------------
// Integer helpers. The sim is integers; every division below is x86 `idiv`
// semantics (truncate toward zero), which is also Rust's `/` for i32. The
// engine emits these as reciprocal-multiply sequences; the helpers name the
// exact sequence so the rounding contract is visible and testable.
// ---------------------------------------------------------------------------

/// `cdq; sub eax,edx; sar eax,1` — divide by 2, truncating toward zero.
/// Emitted at `0x006bc598` (unit score) and `0x006bc48b` (build score).
#[inline]
pub fn half_trunc(x: i32) -> i32 {
    x.wrapping_sub(x >> 31) >> 1
}

/// `mov eax,0x66666667; imul; sar edx,1` — divide by 5, truncating toward zero.
/// Emitted at `0x006bc59d`, `0x006bc4b7`, `0x006bc220`, `0x006bc36f`.
#[inline]
pub fn div5_trunc(x: i32) -> i32 {
    x / 5
}

/// `mov eax,0x55555556; imul` — divide by 3, truncating toward zero.
/// Emitted at `0x006bc4de` (wonder score).
#[inline]
pub fn div3_trunc(x: i32) -> i32 {
    x / 3
}

/// `(v + (v>>31 & 0xff)) >> 8` — the 8.8 fixed-point rescale in
/// `TypeData::get_score_value` at `0x00667490` / `0x006674a8`.
/// Identical to `v / 256` truncating toward zero.
#[inline]
pub fn rescale_256(v: i32) -> i32 {
    v.wrapping_add((v >> 31) & 0xff) >> 8
}

// ---------------------------------------------------------------------------
// Enums, verbatim from the PDB type stream (schema/types.json).
// ---------------------------------------------------------------------------

/// `VictoryIndex` — the `<CATEGORIES id="victories">` selector, stored in
/// `GameInfo::victory` (`Game+0x38`).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Victory {
    Standard = 0,
    SuddenDeath = 1,
    Conquest = 2,
    Score = 3,
    TimeLimit = 4,
    MusicalChairs = 5,
    Wonder = 6,
    /// The engine name is `VICTORY_POPULATION`; the UI calls it "Territory" and
    /// the option category is `popwins` / "Territory Goal".
    Population = 7,
    Economic = 8,
    TechRace = 9,
    Scenario = 10,
}

impl Victory {
    pub fn from_u8(v: u8) -> Option<Self> {
        use Victory::*;
        Some(match v {
            0 => Standard,
            1 => SuddenDeath,
            2 => Conquest,
            3 => Score,
            4 => TimeLimit,
            5 => MusicalChairs,
            6 => Wonder,
            7 => Population,
            8 => Economic,
            9 => TechRace,
            10 => Scenario,
            _ => return None,
        })
    }
}

/// `VictoryTypeIndex` — what is stored in `LeaderData::victory_type` (`+0x7D8`)
/// and passed as argument 1 of `Leader::victory`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum VictoryType {
    Generic = 0,
    ByWonder = 1,
    ByTerritory = 2,
    ByTechRace = 3,
    ByScore = 4,
    ByEconomy = 5,
    ByTimeLimit = 6,
}

/// `DefeatTypeIndex` — stored in `LeaderData::defeat_type` (`+0x7DC`).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum DefeatType {
    Conquest = 0,
    Capital = 1,
    SuddenDeathCapital = 2,
    SuddenDeath = 3,
    MusicalChairs = 4,
    /// Somebody else won; this player is defeated as a consequence.
    Victory = 5,
    Resign = 6,
    Disconnect = 7,
    Armageddon = 8,
    Scenario = 9,
    Hero = 10,
}

impl DefeatType {
    /// `Leader::defeat`'s first argument arrives as a raw `int` from every caller that is
    /// not this module — `Player::resign` pushes 6 at `0x006EDE3A`, `Player::drop` pushes 7
    /// at `0x006EDFF5`, `Player::leave_game`'s capital arm pushes 1 at `0x006EE0C4`. An
    /// out-of-range value is not a `DefeatTypeIndex` and is refused rather than coerced.
    pub fn from_i32(v: i32) -> Option<Self> {
        use DefeatType::*;
        Some(match v {
            0 => Conquest,
            1 => Capital,
            2 => SuddenDeathCapital,
            3 => SuddenDeath,
            4 => MusicalChairs,
            5 => Victory,
            6 => Resign,
            7 => Disconnect,
            8 => Armageddon,
            9 => Scenario,
            10 => Hero,
            _ => return None,
        })
    }
}

/// `EliminationIndex` — `GameInfo::elimination` (`Game+0x37`).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Elimination {
    Conquest = 0,
    Capital = 1,
    CapitalSudden = 2,
    Sudden = 3,
}

/// `DiploButtonCats` — the value stored in `LeaderData::diplos[j]` (`+0x74 + 4j`)
/// and returned by `LeaderData::get_diplo`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum Diplo {
    War = 0,
    Peace = 1,
    Ally = 2,
}

/// `LeaderData::get_diplo(int)` @ `0x006EBA50`, detached from leader storage so a host
/// adapter can use the retail mutual-minimum rule without copying it. `declared_ab` and
/// `declared_ba` are the two `LeaderData::diplos` cells.
#[inline]
pub const fn effective_diplo(same_player: bool, declared_ab: i32, declared_ba: i32) -> Diplo {
    if same_player {
        Diplo::Ally
    } else if declared_ab == Diplo::War as i32 || declared_ba == Diplo::War as i32 {
        Diplo::War
    } else if declared_ab == Diplo::Ally as i32 && declared_ba == Diplo::Ally as i32 {
        Diplo::Ally
    } else {
        Diplo::Peace
    }
}

/// `LeaderFlagIndex` bits of `LeaderData::leader_flags` (`+0x00`).
pub mod leader_flag {
    pub const VALID: i32 = 1;
    pub const ACTIVE: i32 = 2;
    pub const HUMAN: i32 = 4;
    pub const WON: i32 = 0x20;
    pub const DEFEATED: i32 = 0x40;
    pub const SURVIVED: i32 = 0x80;
    pub const NEW_UNITS: i32 = 0x0080_0000;
    pub const NEW_TECH: i32 = 0x0100_0000;
}

/// `LeaderFlag2Index` bits of `LeaderData::leader_flags2` (`+0x04`).
pub mod leader_flag2 {
    pub const UNIT_AI_OFF: i32 = 2;
    pub const INSTANT_VICTORY: i32 = 0x100;
    pub const INSTANT_DEFEAT: i32 = 0x200;
}

/// Bits of `Game::semaphore` (`BitMask<256>` at `Game+0x814`, bit array at `+0x820`).
/// Named from use sites; the engine's own enum for these is not in the type stream.
pub mod game_sem {
    /// `game[0x820] & 0x04` — networked or recording. Gates command-stream RNG skips.
    pub const NET_OR_RECORDING: u32 = 2;
    /// `game[0x820] & 0x40` — set by `Game::check_victory` at `0x00592838`; "the match is over".
    pub const GAME_OVER: u32 = 6;
    /// `game[0x820] & 0x80` — team scoring on (`LeaderData::get_team_score` averages the team).
    pub const TEAM_SCORING: u32 = 7;
    /// `game[0x821] & 0x02` — the mode in which `Leaders::strategy_all` calls `Game::check_victory`.
    pub const CHECK_VICTORY_MODE: u32 = 9;
    /// `game[0x821] & 0x10` — replay playback.
    pub const PLAYBACK: u32 = 12;
    /// `game[0x822] & 0x02` — scenario/CTW rule selection. The same unnamed engine bit
    /// also selects Tech Race's all-epochs form.
    pub const SCENARIO_RULES: u32 = 17;
    /// `game[0x822] & 0x40` — victory resolution already ran this frame.
    pub const VICTORY_RESOLVED: u32 = 22;
}

/// `UnitFlags::UNITTYPE_NO_RESEARCH`.
pub const UNITTYPE_NO_RESEARCH: i32 = 0x80;

// ---------------------------------------------------------------------------
// TypeIndex ranges. [measured] from the loop bounds of the score functions and
// the range checks devirtualised into TypeData::get_score_value.
// ---------------------------------------------------------------------------

/// First unit `TypeIndex`. Loop bound at `0x006bc533` / `0x006bc1c5`.
pub const UNIT_FIRST: usize = 0x32; // 50
/// One past the last unit index the score loops visit (`cmp esi,0x192`).
pub const UNIT_SCORE_END: usize = 0x192; // 402
/// `TypeData::is_unit_type` accepts `0x32..=0x19D` (`0x00470780`-family range test).
pub const UNIT_TYPE_END: usize = 0x19E; // 414
/// First building `TypeIndex` (`mov edi,0x19e` at `0x006bc41e`).
pub const BUILD_FIRST: usize = 0x19E; // 414
/// One past the last building index (`cmp edi,0x21f`).
pub const BUILD_END: usize = 0x21F; // 543
/// Wonder sub-range inside buildings: `0x20E <= idx < 0x21F` (`0x006bc4a9`).
pub const WONDER_FIRST: usize = 0x20E; // 526
pub const WONDER_END: usize = 0x21F; // 543
/// First tech `TypeIndex` (`mov eax,0x220` at `0x006bc360`-block).
pub const TECH_FIRST: usize = 0x220; // 544
/// One past the last tech index (`cmp esi,0x275`).
pub const TECH_END: usize = 0x275; // 629
/// Spell range used by `get_score_value`'s fourth arm: `0x275..=0x2AB`.
pub const SPELL_FIRST: usize = 0x275; // 629
pub const SPELL_END: usize = 0x2AC; // 684
/// `LeaderData::num_queued` is `unsigned short[806]` at `+0x5A22`, indexed by raw `TypeIndex`.
pub const NUM_TYPES: usize = 806;
/// `LeaderData::num_units` is `unsigned short[352]` at `+0x5762`, indexed by `TypeIndex - 50`.
pub const NUM_UNIT_SLOTS: usize = 352;
/// `LeaderData::num_buildings` is `unsigned short[129]` at `+0x555E`, indexed by `TypeIndex - 414`.
pub const NUM_BUILD_SLOTS: usize = 129;
/// Six primary resources: FOOD, TIMBER, WEALTH, KNOWLEDGE, METAL, OIL (`TypeIndex` 0..5).
pub const NUM_RESOURCES: usize = 6;
/// Player slots. `?leaders@@3VLeaders@@A` at `0x00E3A390`, stride `0x6EEC`, 8 entries.
pub const NUM_LEADERS: usize = 8;

/// XOR key for `LeaderDataEncrypt::bucket[i]` (`xor ecx,0x8221` at `0x006BC2C1`).
/// `LeaderDataEncrypt` stores its ints obfuscated; the XOR **decodes** to the real
/// amount, so it is a storage detail and not part of the score arithmetic.
pub const ENC_KEY_BUCKET: i32 = 0x8221;
/// XOR key for `LeaderDataEncrypt::income[i]` (`xor ecx,0x90236` at `0x006BC305`).
pub const ENC_KEY_INCOME: i32 = 0x0009_0236;

/// Decode one `LeaderDataEncrypt` slot read out of live memory or a save.
#[inline]
pub fn decrypt_field(stored: i32, key: i32) -> i32 {
    stored ^ key
}

// ---------------------------------------------------------------------------
// Rules constants. [measured] ron-data/rules.xml via docs/derivation/rules-constants.json.
// ---------------------------------------------------------------------------

/// The `Constants` fields this lane reads, with their retail values.
#[derive(Copy, Clone, Debug)]
pub struct ScoreConstants {
    /// `Constants+0x354`, `UNIT_COST_FACTOR` = 10.
    pub unit_cost_factor: i32,
    /// `Constants+0x358`, `BUILD_COST_FACTOR` = 10.
    pub build_cost_factor: i32,
    /// `Constants+0x35C`, `TECH_COST_FACTOR` = 10.
    pub tech_cost_factor: i32,
    /// `Constants+0x360`, `SPELL_COST_FACTOR` = 10.
    pub spell_cost_factor: i32,
    /// `Constants+0x37C`, `BUILD_SUPPORT_FACTOR` = 1.
    pub build_support_factor: i32,
    /// `Constants+0x3A4`, `RESEARCH_PREMIUM` = "1/1" parsed at scale 256 -> 256.
    pub research_premium: i32,
    /// `Constants+0xD04`, `RETAKE_CAPITAL` = 3600 frames.
    pub retake_capital: i32,
    /// `Constants+0xD08`, `WONDER_TIMER` = 4500 frames.
    pub wonder_timer: i32,
    /// `Constants+0xD0C`, `WONDER_AGE` = 0 frames.
    pub wonder_age: i32,
    /// `Constants+0xD10`, `POPWIN_TIMER` = 3600 frames.
    pub popwin_timer: i32,
    /// `Constants+0xD14`, `ARMAGEDDON` = 4 nukes.
    pub armageddon: i32,
    /// `Constants+0xD18`, `ARMAGEDDON_PER_NATION` = 1 nuke.
    pub armageddon_per_nation: i32,
    /// `Constants+0xD1C`, `ARMAGEDDON_PER_TEAM` = 2 nukes.
    pub armageddon_per_team: i32,
}

impl Default for ScoreConstants {
    fn default() -> Self {
        ScoreConstants {
            unit_cost_factor: 10,
            build_cost_factor: 10,
            tech_cost_factor: 10,
            spell_cost_factor: 10,
            build_support_factor: 1,
            research_premium: 256,
            retake_capital: 3600,
            wonder_timer: 4500,
            wonder_age: 0,
            popwin_timer: 3600,
            armageddon: 4,
            armageddon_per_nation: 1,
            armageddon_per_team: 2,
        }
    }
}

/// The `<CATEGORIES>` option tables the victory checks index into. Each entry is
/// `Category::data[0]` (`Category+0x3C`, stride `0x58`).
#[derive(Clone, Debug)]
pub struct VictoryOptions {
    /// `?scores@@3VCategories@@A` `0x00E802D0`, `<CATEGORIES id="scores">`.
    pub scores: Vec<i32>,
    /// `?time_limits@@3VCategories@@A` `0x00E80328`, minutes.
    pub time_limits: Vec<i32>,
    /// `?chairs@@3VCategories@@A` `0x00E80378`, minutes between musical-chairs culls.
    pub chairs: Vec<i32>,
    /// `?wonderwins@@3VCategories@@A` `0x00E803D8`, wonder points.
    pub wonderwins: Vec<i32>,
    /// `?popwins@@3VCategories@@A` `0x00E80430`, percent of world territory.
    pub popwins: Vec<i32>,
    /// `?econwins@@3VCategories@@A` `0x00E804B0`, average income.
    pub econwins: Vec<i32>,
    /// `?map_sizes@@3VCategories@@A` `0x00E7FC98`. Only index 3 ("Standard", 70) is
    /// read by the timers — it is the fixed reference size, not `info.map_size`.
    pub map_sizes: Vec<i32>,
}

impl Default for VictoryOptions {
    fn default() -> Self {
        VictoryOptions {
            scores: vec![
                1000, 2000, 3000, 4000, 5000, 6000, 7000, 8000, 9000, 10000, 15000, 20000,
            ],
            time_limits: vec![15, 30, 45, 60, 90, 120, 180, 240, 240],
            chairs: vec![2, 5, 8, 10, 15, 20, 25, 30],
            wonderwins: vec![1, 2, 3, 4, 6, 8, 10, 12, 14, 16, 20, 24, 9999],
            popwins: vec![30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80, 90, 100],
            econwins: vec![100, 200, 300, 400, 500, 600, 700, 800, 900],
            map_sizes: vec![40, 50, 60, 70, 80, 90, 100],
        }
    }
}

/// The map-size category index the wonder/territory/capital timers scale against.
/// `mov esi,[eax+0x144]` where `eax = map_sizes.list` — `0x144 = 3*0x58 + 0x3C`.
pub const MAP_SIZE_REFERENCE_INDEX: usize = 3;

// ---------------------------------------------------------------------------
// Static rules the score needs.
// ---------------------------------------------------------------------------

/// Which `get_score_value` arm a `TypeIndex` takes. In retail these are the four
/// virtuals `TypeData::is_unit_type` / `is_build_type` / `is_tech_type` /
/// `is_spell_type` at vtable `+0x0C/+0x14/+0x28/+0x48`; the base implementations
/// are pure `TypeIndex` range tests (`0x004707E0`, `0x004707C0`, `0x004706C0`,
/// `0x00470590`) and the compiler devirtualises them.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum TypeKind {
    Unit,
    Build,
    Tech,
    Spell,
    /// Everything else falls through to the `tech_cost_factor` arm.
    #[default]
    Other,
}

/// One row of the static type table: only the fields the score reads.
///
/// `costs` holds the **raw `ron-data` numbers**, not resource amounts.
/// `unitrules.xml`'s own comment says *"COST = Base cost of unit (multiply by 10)"*,
/// and `Type::sum_rules_cost` (`0x006680E0`) — a dead-code twin of
/// `get_score_value`'s `param == 0` path, and the function whose *name* settles this —
/// computes `cost_factor * sum(costs)` as the rules-level total cost. So
/// `unit_cost_factor = 10` **is** that ×10, and `get_score_value(t, 0)` is literally
/// the type's total resource cost.
///
/// Worked example, `Citizen` (`COST 2f`, `SUPPORT 1f`):
/// `base = 10 * 2 = 20`, `step = 1`, and `n` citizens contribute
/// `(20n + ((n-1)*1*n)/2) / 5` — 4 points for one, 118 for twenty.
#[derive(Clone, Debug, Default)]
pub struct TypeRow {
    /// `TypeData::costs[6]` at `+0x18`, in raw rules units.
    pub costs: [i32; NUM_RESOURCES],
    pub kind: TypeKind,
    /// `TypeData::is_wonder_type` (`0x00470780`): `0x20E <= idx < 0x21F`.
    pub is_wonder: bool,
    /// `ObjectTypeData::support_cost[2]` at `+0x270`.
    pub support_cost: [i32; 2],
    /// `ObjectTypeData::attack` at `+0x1E8`; non-zero => counted into `score_units_2`.
    pub attack: i32,
    /// `UnitTypeData::unit_flags` at `+0x2B4`.
    pub unit_flags: i32,
    /// `UnitTypeData::research_premium_cost` at `+0x2E0`.
    pub research_premium_cost: i32,
}

/// The static type table plus the constants `get_score_value` multiplies by.
#[derive(Clone, Debug)]
pub struct TypeTable {
    pub rows: Vec<TypeRow>,
    pub constants: ScoreConstants,
}

impl TypeTable {
    /// An empty table with the retail range/kind assignment already filled in, so
    /// a caller only has to drop in costs and support values.
    pub fn with_default_kinds(constants: ScoreConstants) -> Self {
        let mut rows = vec![TypeRow::default(); NUM_TYPES];
        for (idx, row) in rows.iter_mut().enumerate() {
            row.kind = if (UNIT_FIRST..UNIT_TYPE_END).contains(&idx) {
                TypeKind::Unit
            } else if (BUILD_FIRST..BUILD_END).contains(&idx) {
                TypeKind::Build
            } else if (TECH_FIRST..TECH_END).contains(&idx) {
                TypeKind::Tech
            } else if (SPELL_FIRST..SPELL_END).contains(&idx) {
                TypeKind::Spell
            } else {
                TypeKind::Other
            };
            row.is_wonder = (WONDER_FIRST..WONDER_END).contains(&idx);
        }
        TypeTable { rows, constants }
    }

    /// `TypeData::get_score_value(int)` @ `0x006673B0`.
    ///
    /// `param != 0` is the "research premium" form used only by
    /// `compute_unit_upgrades_score`; `param == 0` is the plain cost form.
    ///
    /// The `0x42/0x43/0x44 -> 0x32` substitution at `0x006673C6` remaps the three
    /// alternate citizen `TypeIndex` values onto the base citizen for cost lookup,
    /// but only in the `param == 0` form. The *kind* dispatch still uses the
    /// original index; only `costs` and `research_premium_cost` use the remap.
    pub fn get_score_value(&self, idx: usize, param: i32) -> i32 {
        let mut i2 = idx;
        if param == 0 && (idx == 0x42 || idx == 0x43 || idx == 0x44) {
            i2 = UNIT_FIRST;
        }
        let src = &self.rows[i2];
        // The engine sums costs[5]..costs[0]; i32 addition is associative so the
        // order is immaterial, but it wraps, so use wrapping adds.
        let mut cost: i32 = 0;
        for c in src.costs.iter().rev() {
            cost = cost.wrapping_add(*c);
        }
        let k = &self.constants;
        match self.rows[idx].kind {
            TypeKind::Unit => {
                if param == 0 {
                    k.unit_cost_factor.wrapping_mul(cost)
                } else {
                    let v = k
                        .research_premium
                        .wrapping_mul(k.unit_cost_factor)
                        .wrapping_mul(cost);
                    let v = rescale_256(v);
                    let v = v.wrapping_mul(src.research_premium_cost);
                    rescale_256(v)
                }
            }
            TypeKind::Build => k.build_cost_factor.wrapping_mul(cost),
            TypeKind::Tech => k.tech_cost_factor.wrapping_mul(cost),
            TypeKind::Spell => k.spell_cost_factor.wrapping_mul(cost),
            TypeKind::Other => k.tech_cost_factor.wrapping_mul(cost),
        }
    }
}

// ---------------------------------------------------------------------------
// Match-level state (the Game fields this lane reads/writes).
// ---------------------------------------------------------------------------

/// The `GameInfo` bytes that select victory behaviour. `GameInfo` sits at
/// `Game+0x0C`; the byte offsets below are relative to `GameInfo`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct MatchOptions {
    /// `GameInfo+0x18` (`Game+0x24`). 7 selects the mode where a slot can be neutral.
    pub team_style: u8,
    /// `GameInfo+0x1E` (`Game+0x2A`).
    pub game_rules: u8,
    /// `GameInfo+0x1F` (`Game+0x2B`). Stored zero-based; scenario builtins expose 1..=6.
    pub difficulty: u8,
    /// `GameInfo+0x21` (`Game+0x2D`). Indexes `<CATEGORIES id="startingresources">`.
    pub starting_resources: u8,
    /// `GameInfo+0x24` (`Game+0x30`). `Leader::init` uses 3 for the treaty base and
    /// any value >= 1 as the shared-vision fallback.
    pub reveal_map: u8,
    /// `GameInfo+0x26` (`Game+0x32`). Compared with `LeaderData::starting_age` while
    /// selecting the initial raw non-team relation.
    pub rush_rules: u8,
    /// `GameInfo+0x28` (`Game+0x34`). Primary starting age, capped at 7.
    pub starting_technology: u8,
    /// `GameInfo+0x29` (`Game+0x35`). Team-zero secondary starting age, capped at 7.
    pub starting_technology2: u8,
    /// `GameInfo+0x2A` (`Game+0x36`). Final age count used by the ordinary Tech Race arm.
    pub ending_technology: u8,
    /// `GameInfo+0x2B` (`Game+0x37`). [`Elimination`].
    pub elimination: u8,
    /// `GameInfo+0x2C` (`Game+0x38`). [`Victory`].
    pub victory: u8,
    /// `GameInfo+0x2D` (`Game+0x39`). Index into `wonderwins`.
    pub wonderwin: u8,
    /// `GameInfo+0x2E` (`Game+0x3A`). Index into `scores`.
    pub score_goal: u8,
    /// `GameInfo+0x2F` (`Game+0x3B`). Index into `popwins` — the territory percentage.
    pub popwin: u8,
    /// `GameInfo+0x30` (`Game+0x3C`). Index into `time_limits`.
    pub time_limit: u8,
    /// `GameInfo+0x31` (`Game+0x3D`). Index into `chairs`.
    pub chairs: u8,
    /// `GameInfo+0x32` (`Game+0x3E`). Index into `econwins`.
    pub econwin: u8,
}

/// The `Game` and `World` scalars this lane needs.
#[derive(Clone, Debug)]
pub struct Match {
    pub options: MatchOptions,
    pub constants: ScoreConstants,
    pub victory_options: VictoryOptions,
    /// `Game::frame` `+0x550`. Incremented at `0x005924BF`.
    pub frame: i32,
    /// `Game::tick` `+0x560` — game seconds; `frame % 15 == 0` bumps it (`0x005924E1`).
    pub tick: i32,
    /// `Game::starting[6]` `+0x600` — the per-resource starting bucket the economy
    /// score subtracts off.
    pub starting: [i32; NUM_RESOURCES],
    /// `Game::on_team[8]` `+0x5E0` — musical chairs counts survivors per team here.
    pub on_team: [i32; NUM_LEADERS],
    /// `Game::num_nations` `+0x6A0`.
    pub num_nations: i32,
    /// `Game::num_sides` `+0x6A8`.
    pub num_sides: i32,
    /// `Game::musical_chairs` `+0x6C4` — frame stamp of the last cull; also reset by
    /// `Leader::defeat` at `0x006ECB94`.
    pub musical_chairs: i32,
    /// `Game::armageddon` `+0x6E0` — nukes detonated so far.
    pub armageddon: i32,
    /// `Game::semaphore` `BitMask<256>` `+0x814`, bit array at `+0x820`.
    pub semaphore: u32,
    /// `World::xs` `+0x00` — map edge length in tiles; the timers scale on it.
    pub world_xs: i32,
    /// `World::land_size` `+0x78` — denominator of the territory score.
    pub world_land_size: i32,
}

impl Default for Match {
    fn default() -> Self {
        Match {
            options: MatchOptions::default(),
            constants: ScoreConstants::default(),
            victory_options: VictoryOptions::default(),
            frame: 0,
            tick: 0,
            starting: [0; NUM_RESOURCES],
            on_team: [0; NUM_LEADERS],
            num_nations: 1,
            num_sides: 1,
            musical_chairs: 0,
            armageddon: 0,
            semaphore: 0,
            world_xs: 70,
            world_land_size: 1,
        }
    }
}

impl Match {
    #[inline]
    pub fn sem(&self, bit: u32) -> bool {
        self.semaphore & (1u32 << bit) != 0
    }
    #[inline]
    pub fn set_sem(&mut self, bit: u32) {
        self.semaphore |= 1u32 << bit;
    }
    #[inline]
    pub fn clear_sem(&mut self, bit: u32) {
        self.semaphore &= !(1u32 << bit);
    }

    /// The simulation-visible tail of `Game::process_end_game` @ `0x00591CE0`.
    ///
    /// `Game::do_frame` calls that function only while `Game+0x822 & 0x40` is set.
    /// After the statistics, leaderboard and menu work, retail clears that exact bit.
    /// Those other calls are product/presentation effects; consuming the one-shot
    /// [`game_sem::VICTORY_RESOLVED`] latch is the state transition owned by the
    /// headless simulation. Returns whether the latch was consumed.
    pub fn process_end_game(&mut self) -> bool {
        if !self.sem(game_sem::VICTORY_RESOLVED) {
            return false;
        }
        self.clear_sem(game_sem::VICTORY_RESOLVED);
        true
    }

    /// `Game::get_armageddon` @ `0x00594020`. The nuke count at which the match ends
    /// in a universal defeat. Scales with the number of nations and teams, then with
    /// the starting-resources option.
    pub fn get_armageddon(&self) -> i32 {
        let k = &self.constants;
        let mut v = k
            .armageddon_per_team
            .wrapping_mul(self.num_sides)
            .wrapping_add(k.armageddon_per_nation.wrapping_mul(self.num_nations))
            .wrapping_add(k.armageddon);
        // startingresources: 7 = Deathmatch, 5 = x10, 6 = x20, 11 = Variable High,
        // 12 = Random, 8 = Infinite. [measured, 0x00594052..0x0059407B]
        match self.options.starting_resources {
            7 => v = v.wrapping_mul(3),
            5 | 6 | 11 | 12 => v = v.wrapping_mul(2),
            _ => {}
        }
        if self.options.starting_resources == 8 && v < 100 {
            v = 100;
        }
        v
    }

    /// True when the armageddon clock has run out. When this holds, every score
    /// component is forced to zero and `Game::defeat_all` runs.
    #[inline]
    pub fn armageddon_reached(&self) -> bool {
        self.armageddon >= self.get_armageddon()
    }

    #[inline]
    fn map_scaled(&self, base: i32) -> i32 {
        // `Game::wonder_timer` / `popwin_timer` / `retake_capital` all share this body.
        if base <= 0 {
            return 0;
        }
        let s = self.victory_options.map_sizes[MAP_SIZE_REFERENCE_INDEX];
        let n = self.world_xs.wrapping_mul(base).wrapping_add(half_trunc(s));
        let v = n / s;
        if v < 1 {
            1
        } else {
            v
        }
    }

    /// `Game::wonder_timer` @ `0x005944F0`.
    pub fn wonder_timer(&self) -> i32 {
        self.map_scaled(self.constants.wonder_timer)
    }
    /// `Game::popwin_timer` @ `0x005944B0`.
    pub fn popwin_timer(&self) -> i32 {
        self.map_scaled(self.constants.popwin_timer)
    }
    /// `Game::retake_capital` @ `0x00594530`.
    pub fn retake_capital(&self) -> i32 {
        self.map_scaled(self.constants.retake_capital)
    }

    /// The `Game` bytes `CheckSums::check_all` covers through `Game::walk_data`
    /// (`0x00589600` walks `[0x550, 0x6E4)` then `[0x814, 0x81C)`), restricted to the
    /// fields this lane owns and emitted in engine field order.
    pub fn walk_bytes(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.frame.to_le_bytes());
        out.extend_from_slice(&self.tick.to_le_bytes());
        for v in &self.on_team {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for v in &self.starting {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&self.num_nations.to_le_bytes());
        out.extend_from_slice(&self.num_sides.to_le_bytes());
        out.extend_from_slice(&self.musical_chairs.to_le_bytes());
        out.extend_from_slice(&self.armageddon.to_le_bytes());
        out.extend_from_slice(&self.semaphore.to_le_bytes());
    }
}

// ---------------------------------------------------------------------------
// Per-leader state.
// ---------------------------------------------------------------------------

/// `LeaderDataEncrypt` (`LeaderData::data_encrypted`, `+0x6EB8`). Only the two
/// arrays the economy score reads. Retail stores these XOR-obfuscated and decodes
/// on every read ([`ENC_KEY_BUCKET`] / [`ENC_KEY_INCOME`]); these fields hold the
/// **decoded** values, which is what the arithmetic operates on. Use
/// [`decrypt_field`] when populating them from live memory.
#[derive(Clone, Debug, Default)]
pub struct EncryptedEconomy {
    /// `LeaderDataEncrypt::bucket[6]` at `+0x00`, decoded.
    pub bucket: [i32; NUM_RESOURCES],
    /// `LeaderDataEncrypt::income[6]` at `+0x94`, decoded.
    pub income: [i32; NUM_RESOURCES],
}

/// The `LeaderData` fields this lane reads and writes.
#[derive(Clone, Debug)]
pub struct LeaderState {
    /// `+0x00` `leader_flags`.
    pub leader_flags: i32,
    /// `+0x04` `leader_flags2`.
    pub leader_flags2: i32,
    /// `+0x08` `who` — the slot index; also the phase of the score throttle.
    pub who: i32,
    /// `+0x10` `defeated_by`.
    pub defeated_by: i32,

    // --- the score block, `+0x18 .. +0x48`, zeroed wholesale by `Leader::reset_score`
    //     (`0x006E37F0`). Field names are verbatim from the PDB.
    /// `+0x18` `score` — the total. This is the number the end-of-match screen shows.
    pub score: i32,
    /// `+0x1C` `score_explored` — always 0 in retail (`compute_explore_score` is a stub).
    pub score_explored: i32,
    /// `+0x20` `score_territory`.
    pub score_territory: i32,
    /// `+0x24` `score_units`.
    pub score_units: i32,
    /// `+0x28` `score_units_2` — military subset; **not** part of the total.
    pub score_units_2: i32,
    /// `+0x2C` `score_buildings`.
    pub score_buildings: i32,
    /// `+0x30` `score_economy`.
    pub score_economy: i32,
    /// `+0x34` `score_pop` — always 0 in retail (`compute_pop_score` is a stub).
    pub score_pop: i32,
    /// `+0x38` `score_unit_upgrades`.
    pub score_unit_upgrades: i32,
    /// `+0x3C` `score_research`.
    pub score_research: i32,
    /// `+0x40` `score_wonders`.
    pub score_wonders: i32,
    /// `+0x44` `score_combat` — never written by the compute path in retail.
    pub score_combat: i32,

    /// `+0x50` `multi_diff`. `DropControl::process_drop` state 3 writes 3 here
    /// (`mov dword ptr [leader + 0x50], 3`, `0x0095967A`) as it clears
    /// [`leader_flag::HUMAN`]: the dropped player's leader continues under AI control.
    pub multi_diff: i32,

    /// `+0x74` `diplos[8]`.
    pub diplos: [i32; NUM_LEADERS],
    /// Exact fields initialized by the remaining `Leader::init` diplomacy loop from
    /// `+0x94..+0x394`, plus `ally_mask` at `+0x6929`.
    pub init_diplomacy: super::leader_init_diplomacy_loop::LeaderInitDiplomacyRow,

    /// `+0x440` `popwin_stamp`.
    pub popwin_stamp: i32,
    /// `+0x444` `popwin_timer`.
    pub popwin_timer: i32,
    /// `+0x448` `wonderwin_stamp`.
    pub wonderwin_stamp: i32,
    /// `+0x44C` `wonderwin_timer`.
    pub wonderwin_timer: i32,
    /// `+0x414` `lost_capital_stamp`.
    pub lost_capital_stamp: i32,
    /// `+0x418` `lost_capital_timer`.
    pub lost_capital_timer: i32,

    /// `+0x788` `production_step`, the persistent 12-stage production-AI cursor.
    pub production_step: i32,
    /// `+0x78C` `prod_script_run`, set once the production script has been entered.
    pub prod_script_run: i32,
    /// `+0x790` `script_step`, the persistent production-script cursor.
    pub script_step: i32,

    /// `+0x7D8` `victory_type`.
    pub victory_type: i32,
    /// `+0x7DC` `defeat_type`.
    pub defeat_type: i32,

    /// `+0x7E4` `pop_cap`, the effective population/control cap.
    pub population_cap: i32,
    /// `+0x7EC` `misery`, cleared by every `Leader::calc_pop_cap` invocation.
    pub misery: i32,

    /// `+0x7F8` `give_att_disabled` — nonzero disables attrition dealt to enemies.
    pub give_attrition_disabled: i32,
    /// `+0x7FC` `take_att_disabled` — nonzero disables attrition received from enemies.
    pub take_attrition_disabled: i32,
    /// `+0x800` `neutral_attrition`, clamped by the scenario setter to `0..=1000`.
    pub neutral_attrition: i32,
    /// `+0x804` `disable_building_attrition`.
    pub building_attrition_disabled: i32,

    /// `+0x824` `cities_captured`, incremented by `Cities::capture_city` before the
    /// center swap is attempted.
    pub cities_captured: i32,
    /// `+0x828` `cities_lost`, incremented by `Cities::capture_city` before the center
    /// swap is attempted.
    pub cities_lost: i32,

    /// `+0x940` `control`, included in production AI's effective-population value.
    pub control: i32,

    /// `+0x555E` `num_buildings[129]`, indexed by `TypeIndex - 414`.
    pub num_buildings: Vec<u16>,
    /// `+0x5762` `num_units[352]`, indexed by `TypeIndex - 50`.
    pub num_units: Vec<u16>,
    /// `+0x5A22` `num_queued[806]`, indexed by raw `TypeIndex`.
    pub num_queued: Vec<u16>,

    /// `+0x6C80` `tech_at_start` `BitMask<806>` (bit array at `+0x6C8C`).
    pub tech_at_start: Vec<u8>,
    /// `+0x6D98` `rare` `BitMask<44>`; its popcount times 25 is an economy term.
    pub rare: u64,

    /// `+0x9D8` `territory` — owned tiles; numerator of the territory score.
    pub territory: i32,
    /// `+0x9E0` `effective_pop`, the most recent queued-units plus control result.
    pub effective_pop: i32,

    /// `+0xA68` `strategy[64]`. `Army::do_mustering` reads the row selected by
    /// `ArmyData::reg`; unlike production-AI host answers, this is persistent LeaderData.
    pub strategy: [u16; super::army_do_mustering::MUSTER_STRATEGY_REGIONS],

    pub economy: EncryptedEconomy,

    /// `LeaderData::has_tech(TypeIndex)` @ `0x006E0C80` — indexed by raw `TypeIndex`.
    pub has_tech: Vec<bool>,
    /// `LeaderData::researching(TypeIndex,-1,0,which)` @ `0x006DB510`. Index 0 is the
    /// `which = 0` call used by `compute_research_score`; index 1 is the `which = 1`
    /// call used by `compute_unit_upgrades_score`.
    pub researching: [Vec<bool>; 2],
    /// `LeaderData::type_avail(TypeIndex, 1)` @ `0x006E33A0`, for the six resources.
    pub resource_avail: [bool; NUM_RESOURCES],
    /// `LeaderData::get_economic()` @ `0x006D6490` — the per-player income figure the
    /// Economic victory averages over the team.
    pub economic: i32,
    /// Exact setup-time result of `LeaderData::has_preq(0x2B0)`, consumed by the
    /// recovered `Leader::init` shared-vision arm.
    pub has_preq_2b0: bool,
    /// `LeaderData::has_preq(0x2B9)` in the Wonder-victory block at
    /// `0x00730FFA..0x00731010`. Any valid member of the qualifying alliance with this
    /// prerequisite bypasses the Standard-mode Wonder countdown.
    pub has_preq_2b9: bool,
}

impl Default for LeaderState {
    fn default() -> Self {
        LeaderState {
            leader_flags: 0,
            leader_flags2: 0,
            who: 0,
            defeated_by: -1,
            score: 0,
            score_explored: 0,
            score_territory: 0,
            score_units: 0,
            score_units_2: 0,
            score_buildings: 0,
            score_economy: 0,
            score_pop: 0,
            score_unit_upgrades: 0,
            score_research: 0,
            score_wonders: 0,
            score_combat: 0,
            multi_diff: 0,
            diplos: [Diplo::War as i32; NUM_LEADERS],
            init_diplomacy: super::leader_init_diplomacy_loop::LeaderInitDiplomacyRow::default(),
            popwin_stamp: 0,
            popwin_timer: 0,
            wonderwin_stamp: 0,
            wonderwin_timer: 0,
            lost_capital_stamp: 0,
            lost_capital_timer: 0,
            production_step: 0,
            prod_script_run: 0,
            script_step: 0,
            victory_type: 0,
            defeat_type: 0,
            population_cap: 0,
            misery: 0,
            give_attrition_disabled: 0,
            take_attrition_disabled: 0,
            neutral_attrition: 0,
            building_attrition_disabled: 0,
            cities_captured: 0,
            cities_lost: 0,
            control: 0,
            num_buildings: vec![0; NUM_BUILD_SLOTS],
            num_units: vec![0; NUM_UNIT_SLOTS],
            num_queued: vec![0; NUM_TYPES],
            tech_at_start: vec![0; NUM_TYPES.div_ceil(8)],
            rare: 0,
            territory: 0,
            effective_pop: 0,
            strategy: [0; super::army_do_mustering::MUSTER_STRATEGY_REGIONS],
            economy: EncryptedEconomy::default(),
            has_tech: vec![false; NUM_TYPES],
            researching: [vec![false; NUM_TYPES], vec![false; NUM_TYPES]],
            resource_avail: [true; NUM_RESOURCES],
            economic: 0,
            has_preq_2b0: false,
            has_preq_2b9: false,
        }
    }
}

impl LeaderState {
    #[inline]
    pub fn flag(&self, bit: i32) -> bool {
        self.leader_flags & bit != 0
    }
    /// `(leader_flags & 3) == 3` — the "in the game and playing" test the engine
    /// opens almost every leader loop with.
    #[inline]
    pub fn is_active(&self) -> bool {
        self.leader_flags & (leader_flag::VALID | leader_flag::ACTIVE)
            == (leader_flag::VALID | leader_flag::ACTIVE)
    }
    /// `(leader_flags & 0x43) == 3` — active and not yet defeated.
    #[inline]
    pub fn is_alive(&self) -> bool {
        self.leader_flags & (leader_flag::VALID | leader_flag::ACTIVE | leader_flag::DEFEATED)
            == (leader_flag::VALID | leader_flag::ACTIVE)
    }
    #[inline]
    pub fn tech_at_start(&self, idx: usize) -> bool {
        self.tech_at_start[idx >> 3] & (1u8 << (idx & 7)) != 0
    }
    #[inline]
    pub fn set_tech_at_start(&mut self, idx: usize) {
        self.tech_at_start[idx >> 3] |= 1u8 << (idx & 7);
    }

    /// `Leader::reset_score` @ `0x006E37F0` — zeroes `+0x18 .. +0x48`.
    pub fn reset_score(&mut self) {
        self.score = 0;
        self.score_explored = 0;
        self.score_territory = 0;
        self.score_units = 0;
        self.score_units_2 = 0;
        self.score_buildings = 0;
        self.score_economy = 0;
        self.score_pop = 0;
        self.score_unit_upgrades = 0;
        self.score_research = 0;
        self.score_wonders = 0;
        self.score_combat = 0;
    }

    /// The subset of `LeaderData::walk_data` (`0x006D6750`) this lane owns, in
    /// engine field order. `walk_data` walks `[0,8)` then `[8, 26922)`, so every
    /// field below is inside checksum channel 8.
    pub fn walk_bytes(&self, out: &mut Vec<u8>) {
        for v in [
            self.leader_flags,
            self.leader_flags2,
            self.who,
            self.defeated_by,
            self.score,
            self.score_explored,
            self.score_territory,
            self.score_units,
            self.score_units_2,
            self.score_buildings,
            self.score_economy,
            self.score_pop,
            self.score_unit_upgrades,
            self.score_research,
            self.score_wonders,
            self.score_combat,
            // `+0x50`, between `score_combat` `+0x44` and `diplos` `+0x74`.
            self.multi_diff,
        ] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for v in &self.diplos {
            out.extend_from_slice(&v.to_le_bytes());
        }
        self.init_diplomacy.walk_prefix_bytes(out);
        for v in [
            self.popwin_stamp,
            self.popwin_timer,
            self.wonderwin_stamp,
            self.wonderwin_timer,
            self.lost_capital_stamp,
            self.lost_capital_timer,
            self.production_step,
            self.prod_script_run,
            self.script_step,
            self.victory_type,
            self.defeat_type,
            self.population_cap,
            self.misery,
            self.give_attrition_disabled,
            self.take_attrition_disabled,
            self.neutral_attrition,
            self.building_attrition_disabled,
            self.cities_captured,
            self.cities_lost,
            self.control,
            self.territory,
            self.effective_pop,
        ] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for value in self.strategy {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.push(self.init_diplomacy.ally_mask);
    }
}

// ---------------------------------------------------------------------------
// The eight-slot leader table plus the whole-match logic that spans slots.
// ---------------------------------------------------------------------------

/// What a victory check decided this frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MatchEvent {
    /// `Leader::victory(type, instant)` fired for `who`.
    Victory {
        who: usize,
        victory_type: VictoryType,
    },
    /// `Leader::defeat(type, arg, instant)` fired for `who`.
    Defeat { who: usize, defeat_type: DefeatType },
    /// `Game::defeat_all` — the armageddon clock ran out.
    ArmageddonAll,
    /// `Game::check_victory` set the game-over semaphore.
    GameOver,
}

/// The eight `Leader` slots (`?leaders@@3VLeaders@@A` `0x00E3A390`, stride `0x6EEC`).
#[derive(Clone, Debug)]
pub struct Leaders {
    pub slots: Vec<LeaderState>,
    pub types: TypeTable,
    /// Canonical external `Player[8]` setup image retained by the Sim after the one-shot
    /// frame-zero team transaction.  It is deliberately outside `LeaderData::walk_data`.
    pub setup_owner: super::player_setup::PlayerSetupOwner,
    /// Events emitted by the last `process_victory` / `check_victory` call.
    pub events: Vec<MatchEvent>,
    /// Owners whose concrete `Build` queues must receive retail's terminal
    /// `Build::clean_queue(0)` sweep. Unlike [`Self::events`], this accumulator is not
    /// cleared at the head of `process_victory`: step 11 and step 12 may both resolve
    /// leaders before the live object adapter gets a chance to drain the requests.
    terminal_queue_cleanup: u8,
    /// Owners whose live Unit band must receive the deterministic defeated-player
    /// cleanup at `0x006ECC1C..0x006ECCAF`. This is separate from production cleanup:
    /// victory closes Build queues too, but only defeat kills planes / closes Unit orders.
    defeat_unit_cleanup: u8,
}

impl Leaders {
    pub fn new(types: TypeTable) -> Self {
        let mut slots = Vec::with_capacity(NUM_LEADERS);
        for who in 0..NUM_LEADERS {
            let mut l = LeaderState::default();
            l.who = who as i32;
            slots.push(l);
        }
        Leaders {
            slots,
            types,
            setup_owner: super::player_setup::PlayerSetupOwner::default(),
            events: Vec::new(),
            terminal_queue_cleanup: 0,
            defeat_unit_cleanup: 0,
        }
    }

    /// Drain the owner mask for concrete, no-refund terminal queue cleanup.
    ///
    /// `Leader::victory` and `Leader::defeat` each walk every live owned Build and call
    /// `Build::clean_queue(0)`. The victory lane owns *when* that traversal is requested;
    /// the object/production adapter owns the actual Build rows. Requests accumulate
    /// until drained so recursive allied victories and enemy defeats cannot overwrite
    /// one another, nor can step 12 erase a step-11 resolution.
    pub fn take_terminal_queue_cleanup(&mut self) -> u8 {
        std::mem::take(&mut self.terminal_queue_cleanup)
    }

    #[inline]
    fn request_terminal_queue_cleanup(&mut self, who: usize) {
        debug_assert!(who < NUM_LEADERS);
        self.terminal_queue_cleanup |= 1u8 << who;
    }

    /// Drain the owner mask for `Leader::defeat`'s concrete Unit-band transaction.
    ///
    /// Requests deliberately accumulate independently of Build cleanup. Recursive enemy
    /// defeats can happen inside one victory call, and the live object adapter must see
    /// every owner exactly once even if a later defeated-player callback re-enters the
    /// terminal state machine.
    pub fn take_defeat_unit_cleanup(&mut self) -> u8 {
        std::mem::take(&mut self.defeat_unit_cleanup)
    }

    #[inline]
    fn request_defeat_unit_cleanup(&mut self, who: usize) {
        debug_assert!(who < NUM_LEADERS);
        self.defeat_unit_cleanup |= 1u8 << who;
    }

    /// Restore requests which the live adapter could not preflight yet. Kept crate-local:
    /// only the deterministic object bridge may defer a terminal sweep.
    pub(crate) fn defer_defeat_unit_cleanup(&mut self, owners: u8) {
        self.defeat_unit_cleanup |= owners;
    }

    /// Snapshot the two object-store requests without consuming them. Transaction and
    /// persistence adapters use this to prove that no terminal side effect is lost.
    pub(crate) fn pending_cleanup_masks(&self) -> (u8, u8) {
        (self.terminal_queue_cleanup, self.defeat_unit_cleanup)
    }

    // -- diplomacy -----------------------------------------------------------

    /// `LeaderData::get_diplo(int j)` @ `0x006EBA50`. The pairwise state is the
    /// *mutual minimum*: war if either side declares it, ally only if both do.
    pub fn get_diplo(&self, i: usize, j: usize) -> Diplo {
        effective_diplo(i == j, self.slots[i].diplos[j], self.slots[j].diplos[i])
    }

    /// `LeaderData::is_enemy(int j)` @ `0x006EBAA0`. Note this is *not*
    /// `get_diplo == War` on self: a leader is never its own enemy.
    pub fn is_enemy(&self, i: usize, j: usize) -> bool {
        effective_diplo(i == j, self.slots[i].diplos[j], self.slots[j].diplos[i]) == Diplo::War
    }

    /// `LeaderData::is_ally(int j)` @ `0x006EDB50`. Self counts as an ally.
    pub fn is_ally(&self, i: usize, j: usize) -> bool {
        effective_diplo(i == j, self.slots[i].diplos[j], self.slots[j].diplos[i]) == Diplo::Ally
    }

    /// `LeaderData::is_peace(int j)` @ `0x006E1200`.
    pub fn is_peace(&self, i: usize, j: usize) -> bool {
        effective_diplo(i == j, self.slots[i].diplos[j], self.slots[j].diplos[i]) == Diplo::Peace
    }

    /// `Leader::set_diplo(int j, int state)` @ `0x006EC6A0`, state part only.
    /// The retail function additionally re-targets armies, drops shared vision and
    /// emits chat; those live in other lanes.
    pub fn set_diplo(&mut self, i: usize, j: usize, state: Diplo) {
        self.slots[i].diplos[j] = state as i32;
    }

    // -- score ---------------------------------------------------------------

    /// `Leader::compute_unit_score` @ `0x006BC500`.
    ///
    /// Writes `score_units` and `score_units_2`. `score_units_2` is the subset with
    /// non-zero `attack` — the military score — and is deliberately *excluded* from
    /// the total.
    pub fn compute_unit_score(&mut self, m: &Match, who: usize) {
        self.slots[who].score_units = 0;
        self.slots[who].score_units_2 = 0;
        if m.armageddon_reached() {
            return;
        }
        let mut units = 0i32;
        let mut units2 = 0i32;
        for t in UNIT_FIRST..UNIT_SCORE_END {
            let l = &self.slots[who];
            let n = l.num_queued[t] as i32 + l.num_units[t - UNIT_FIRST] as i32;
            if n == 0 {
                continue;
            }
            let base = self.types.get_score_value(t, 0);
            let row = &self.types.rows[t];
            let step = row.support_cost[1].wrapping_add(row.support_cost[0]);
            let extra = half_trunc(n.wrapping_sub(1).wrapping_mul(step).wrapping_mul(n));
            let v = div5_trunc(base.wrapping_mul(n).wrapping_add(extra));
            units = units.wrapping_add(v);
            if row.attack != 0 {
                units2 = units2.wrapping_add(v);
            }
        }
        self.slots[who].score_units = units;
        self.slots[who].score_units_2 = units2;
    }

    /// `Leader::compute_build_score` @ `0x006BC3F0`.
    ///
    /// Writes both `score_buildings` and `score_wonders` — wonders take `/3`
    /// instead of `/5`, so a wonder is worth 5/3 of an equally-costly building.
    pub fn compute_build_score(&mut self, m: &Match, who: usize) {
        self.slots[who].score_buildings = 0;
        self.slots[who].score_wonders = 0;
        if m.armageddon_reached() {
            return;
        }
        let bsf = self.types.constants.build_support_factor;
        let mut builds = 0i32;
        let mut wonders = 0i32;
        for t in BUILD_FIRST..BUILD_END {
            let l = &self.slots[who];
            let n = l.num_queued[t] as i32 + l.num_buildings[t - BUILD_FIRST] as i32;
            if n == 0 {
                continue;
            }
            let base = self.types.get_score_value(t, 0);
            let row = &self.types.rows[t];
            let step = row.support_cost[1]
                .wrapping_add(row.support_cost[0])
                .wrapping_mul(bsf);
            let extra = half_trunc(n.wrapping_sub(1).wrapping_mul(step).wrapping_mul(n));
            let v = base.wrapping_mul(n).wrapping_add(extra);
            if row.is_wonder {
                wonders = wonders.wrapping_add(div3_trunc(v));
            } else {
                builds = builds.wrapping_add(div5_trunc(v));
            }
        }
        self.slots[who].score_buildings = builds;
        self.slots[who].score_wonders = wonders;
    }

    /// `Leader::compute_economy_score` @ `0x006BC270`.
    ///
    /// Two terms per resource plus a flat 25 per rare resource controlled:
    ///  * stockpile above the starting bucket, `/20`, clamped to `[0, 100]`
    ///  * income, `/80`, clamped **above** at 200 with **no lower clamp** — a
    ///    negative income really does subtract from the score.
    pub fn compute_economy_score(&mut self, m: &Match, who: usize) {
        self.slots[who].score_economy = 0;
        if m.armageddon_reached() {
            return;
        }
        let mut acc = 0i32;
        {
            let l = &self.slots[who];
            for i in 0..NUM_RESOURCES {
                if !l.resource_avail[i] {
                    continue;
                }
                let a = l.economy.bucket[i].wrapping_sub(m.starting[i]);
                let mut v = a / 20;
                if v < 0 {
                    v = 0;
                } else if v > 100 {
                    v = 100;
                }
                acc = acc.wrapping_add(v);

                let b = l.economy.income[i];
                let mut w = b / 80;
                if w > 200 {
                    w = 200;
                }
                acc = acc.wrapping_add(w);
            }
            acc = acc.wrapping_add((l.rare.count_ones() as i32).wrapping_mul(25));
        }
        self.slots[who].score_economy = acc;
    }

    /// `Leader::compute_unit_upgrades_score` @ `0x006BC1A0`.
    ///
    /// Sums the *research-premium* score value of every unit type the leader has
    /// unlocked or is researching, skipping ones it started with and ones flagged
    /// `UNITTYPE_NO_RESEARCH`.
    pub fn compute_unit_upgrades_score(&mut self, m: &Match, who: usize) {
        self.slots[who].score_unit_upgrades = 0;
        if m.armageddon_reached() {
            return;
        }
        let mut acc = 0i32;
        for t in UNIT_FIRST..UNIT_SCORE_END {
            let l = &self.slots[who];
            if !(l.has_tech[t] || l.researching[1][t]) {
                continue;
            }
            if l.tech_at_start(t) {
                continue;
            }
            if self.types.rows[t].unit_flags & UNITTYPE_NO_RESEARCH != 0 {
                continue;
            }
            acc = acc.wrapping_add(div5_trunc(self.types.get_score_value(t, 1)));
        }
        self.slots[who].score_unit_upgrades = acc;
    }

    /// `Leader::compute_research_score` @ `0x006BC360`.
    pub fn compute_research_score(&mut self, m: &Match, who: usize) {
        self.slots[who].score_research = 0;
        if m.armageddon_reached() {
            return;
        }
        let mut acc = 0i32;
        for t in TECH_FIRST..TECH_END {
            let l = &self.slots[who];
            if !(l.has_tech[t] || l.researching[0][t]) {
                continue;
            }
            if l.tech_at_start(t) {
                continue;
            }
            acc = acc.wrapping_add(div5_trunc(self.types.get_score_value(t, 0)));
        }
        self.slots[who].score_research = acc;
    }

    /// `Leader::compute_score(int force)` @ `0x006EC560`.
    ///
    /// **This is the RL reward.** Called once per active leader per frame from
    /// `Leaders::strategy_all` (`0x006ED45B`) with `force = 0`, and from
    /// `Game::defeat_all` (`0x00592ADB`) with `force = 1`.
    ///
    /// The throttle at `0x006EC56C` means the total only refreshes when
    /// `(who + frame) % 10 == 0` — a 10-frame round-robin phase-offset by slot,
    /// the same shape as `Objects::process_all`'s `(frame + i) % 10`. It is
    /// bypassed on frame 0, when forced, and once the game-over semaphore is set.
    pub fn compute_score(&mut self, m: &Match, who: usize, force: i32) {
        if m.frame != 0
            && force == 0
            && !m.sem(game_sem::GAME_OVER)
            && (self.slots[who].who.wrapping_add(m.frame)) % 10 != 0
        {
            return;
        }

        self.slots[who].score_explored = 0; // compute_explore_score, a stub

        if self.slots[who].leader_flags & (leader_flag::NEW_UNITS | leader_flag::NEW_TECH) != 0 {
            self.compute_unit_score(m, who);
            self.slots[who].leader_flags &= !leader_flag::NEW_UNITS;
        }
        self.compute_build_score(m, who);
        self.compute_economy_score(m, who);

        self.slots[who].score_pop = 0; // compute_pop_score, a stub

        if self.slots[who].leader_flags & leader_flag::NEW_TECH != 0 {
            self.compute_unit_upgrades_score(m, who);
            self.compute_research_score(m, who);
            self.slots[who].leader_flags &= !leader_flag::NEW_TECH;
        }

        // score_territory is recomputed unconditionally, before the armageddon gate.
        let terr = self.slots[who].territory.wrapping_mul(1000) / m.world_land_size;
        self.slots[who].score_territory = terr;

        if m.armageddon_reached() {
            self.slots[who].reset_score();
            return;
        }

        let l = &mut self.slots[who];
        // Addition order is verbatim from 0x006EC66D..0x006EC68A.
        let mut acc = l.score_combat;
        acc = acc.wrapping_add(l.score_wonders);
        acc = acc.wrapping_add(l.score_research);
        acc = acc.wrapping_add(l.score_unit_upgrades);
        acc = acc.wrapping_add(l.score_economy);
        acc = acc.wrapping_add(l.score_buildings);
        acc = acc.wrapping_add(l.score_units);
        acc = acc.wrapping_add(terr);
        acc = acc.wrapping_add(l.score_pop);
        acc = acc.wrapping_add(l.score_explored);
        l.score = acc;
    }

    /// `LeaderData::get_team_score` @ `0x006D6520`. Without the team-scoring
    /// semaphore this is just the player's own `score`; with it, the team average.
    pub fn get_team_score(&self, m: &Match, who: usize) -> i32 {
        if !m.sem(game_sem::TEAM_SCORING) {
            return self.slots[who].score;
        }
        let mut sum = 0i32;
        let mut n = 0i32;
        for i in 0..NUM_LEADERS {
            let take = if i == who {
                true
            } else {
                self.slots[i].flag(leader_flag::VALID) && self.is_team(m, who, i)
            };
            if take {
                n += 1;
                sum = sum.wrapping_add(self.slots[i].score);
            }
        }
        sum / n.max(1)
    }

    /// `LeaderData::get_mvp_score` @ `0x006D5DE0`. Combat is weighted x3 and the
    /// two "turtle" components are halved; the winner doubles, and a survivor of a
    /// finished match gets x5/4.
    pub fn get_mvp_score(&self, m: &Match, who: usize) -> i32 {
        let l = &self.slots[who];
        if l.score == 0 {
            return 0;
        }
        let v = l
            .score_combat
            .wrapping_mul(3)
            .wrapping_sub(l.score_economy / 2)
            .wrapping_sub(l.score_buildings / 2)
            .wrapping_add(l.score_territory)
            .wrapping_add(l.score);
        if l.flag(leader_flag::WON) {
            return v.wrapping_mul(2);
        }
        if m.sem(game_sem::GAME_OVER) && l.flag(leader_flag::SURVIVED) {
            let x = v.wrapping_mul(5);
            return x.wrapping_add((x >> 31) & 3) >> 2;
        }
        v
    }

    /// `LeaderData::is_team(int j, int)` @ `0x006EBD30`, reduced to the branch the
    /// score paths take: same-team means allied (or self).
    pub fn is_team(&self, _m: &Match, i: usize, j: usize) -> bool {
        self.is_ally(i, j)
    }

    /// `LeaderData::get_team_terr` @ `0x006D62E0` — the *sum* of `territory` over
    /// self plus allies (not an average).
    pub fn get_team_terr(&self, _m: &Match, who: usize) -> i32 {
        let mut sum = 0i32;
        for i in 0..NUM_LEADERS {
            if i == who {
                sum = sum.wrapping_add(self.slots[i].territory);
            } else if self.slots[i].flag(leader_flag::VALID) && self.is_ally(who, i) {
                sum = sum.wrapping_add(self.slots[i].territory);
            }
        }
        sum
    }

    /// `LeaderData::get_team_economic` @ `0x006D6400` — the *average* of
    /// `LeaderData::get_economic` over self plus team-mates.
    pub fn get_team_economic(&self, m: &Match, who: usize) -> i32 {
        let mut sum = 0i32;
        let mut n = 0i32;
        for i in 0..NUM_LEADERS {
            let take = if i == who {
                true
            } else {
                self.slots[i].flag(leader_flag::VALID) && self.is_team(m, who, i)
            };
            if take {
                n += 1;
                sum = sum.wrapping_add(self.slots[i].economic);
            }
        }
        sum / n.max(1)
    }

    // -- victory conditions --------------------------------------------------

    /// `Game::wonder_winning` @ `0x005948A0`. Returns the slot whose wonder points
    /// meet the `wonderwins` threshold, or `None`. A tie between non-allies is
    /// resolved to "nobody"; a tie between allies goes to the higher wonder value.
    pub fn wonder_winning(
        &self,
        m: &Match,
        wonder_net: &[i32],
        wonder_value: &[i32],
    ) -> Option<usize> {
        let threshold = m.victory_options.wonderwins[m.options.wonderwin as usize];
        let mut best = 0i32;
        let mut ret: Option<usize> = None;
        for i in 0..NUM_LEADERS {
            if !self.slots[i].is_active() {
                continue;
            }
            let w = wonder_net[i];
            if w < best {
                continue;
            }
            if w == best {
                let Some(cur) = ret else { continue };
                if self.is_ally(i, cur) {
                    if wonder_value[i] > wonder_value[cur] {
                        ret = Some(i);
                    }
                    continue;
                }
                ret = None;
            }
            if w < threshold {
                continue;
            }
            best = w;
            ret = Some(i);
        }
        ret
    }

    /// `Game::is_victory_timer` @ `0x00594950`. True when a wonder or territory
    /// countdown is running under Standard victory — the UI clock.
    pub fn is_victory_timer(&self, m: &Match) -> bool {
        if m.options.victory != Victory::Standard as u8 {
            return false;
        }
        self.slots
            .iter()
            .any(|l| l.is_active() && (l.wonderwin_timer != 0 || l.popwin_timer != 0))
    }

    /// `Game::defeat_all` @ `0x00592AA0` — the armageddon resolution. Every active,
    /// undefeated leader is defeated with `DEFEAT_ARMAGEDDON`, then every leader's
    /// score is force-recomputed (which zeroes it, because the clock has run out).
    pub fn defeat_all(&mut self, m: &mut Match) {
        for who in 0..NUM_LEADERS {
            if self.slots[who].flag(leader_flag::VALID) {
                if self.slots[who].leader_flags & (leader_flag::ACTIVE | leader_flag::DEFEATED)
                    == leader_flag::ACTIVE
                {
                    self.slots[who].leader_flags |= leader_flag::SURVIVED;
                    self.defeat(m, who, DefeatType::Armageddon, -1, 0);
                }
            }
            self.compute_score(m, who, 1);
        }
        self.events.push(MatchEvent::ArmageddonAll);
    }

    /// `Leader::victory(int victory_type, int instant)` @ `0x006EC9B0`, state part.
    ///
    /// Marks the winner, cleans every production queue owned by that leader, then
    /// walks every other active leader: allies win with the same type (recursively),
    /// everyone else is defeated with `DEFEAT_VICTORY`. Retail's defeated-player
    /// path calls `Game::check_victory`; [`Self::defeat`] does the same, so a Wonder
    /// win reaches both game-over semaphores instead of stopping at leader flags.
    pub fn victory(&mut self, m: &mut Match, who: usize, vt: VictoryType, instant: i32) {
        if self.slots[who].leader_flags & (leader_flag::WON | leader_flag::DEFEATED) != 0 {
            return;
        }
        self.slots[who].leader_flags |= leader_flag::WON;
        if instant == 0 {
            self.slots[who].leader_flags2 &= !leader_flag2::INSTANT_VICTORY;
        } else {
            self.slots[who].leader_flags2 |= leader_flag2::INSTANT_VICTORY;
        }
        self.slots[who].victory_type = vt as i32;
        // `0x006ECA04..0x006ECA5C`: every live Build owned by the winner receives
        // `Build::clean_queue(0)`. `num_queued` is the aggregate side effect of those
        // per-building queues and therefore becomes exactly zero after the sweep.
        self.slots[who].num_queued.fill(0);
        self.request_terminal_queue_cleanup(who);
        self.events.push(MatchEvent::Victory {
            who,
            victory_type: vt,
        });

        for other in 0..NUM_LEADERS {
            if other == who || !self.slots[other].is_active() {
                continue;
            }
            self.slots[other].leader_flags |= leader_flag::SURVIVED;
            if self.is_ally(other, who) {
                self.victory(m, other, vt, instant);
            } else {
                self.defeat(m, other, DefeatType::Victory, -1, 0);
            }
        }
    }

    /// `Leader::defeat(int defeat_type, int arg, int instant)` @ `0x006ECB00`,
    /// state part. Retail first cleans the player's production queues, then kills owned
    /// true planes and closes every other live Unit's orders. The concrete object mutation
    /// is requested here and executed by the live adapter before the tick continues. It
    /// then calls `Game::check_victory`; that terminal transition is executable here.
    pub fn defeat(&mut self, m: &mut Match, who: usize, dt: DefeatType, by: i32, instant: i32) {
        self.slots[who].defeated_by = by;
        if self.slots[who].flag(leader_flag::DEFEATED) {
            return;
        }
        if instant == 0 {
            self.slots[who].leader_flags2 &= !leader_flag2::INSTANT_DEFEAT;
        } else {
            self.slots[who].leader_flags2 |= leader_flag2::INSTANT_DEFEAT;
        }
        self.slots[who].leader_flags =
            (self.slots[who].leader_flags & !leader_flag::ACTIVE) | leader_flag::DEFEATED;
        self.slots[who].leader_flags2 |= leader_flag2::UNIT_AI_OFF;
        // `mov [game+0x6c4], [game+0x550]` at 0x006ECB94 — any defeat restarts the
        // musical-chairs interval.
        m.musical_chairs = m.frame;
        self.slots[who].defeat_type = dt as i32;
        self.slots[who].num_queued.fill(0);
        self.request_terminal_queue_cleanup(who);
        self.request_defeat_unit_cleanup(who);
        self.events.push(MatchEvent::Defeat {
            who,
            defeat_type: dt,
        });
        // `Leader::defeat` `0x006ECDD8..0x006ECDE8`: unless the game-over bit was
        // already set, retail calls `Game::check_victory` after the cleanup. This is
        // what turns a completed Wonder victory into a terminal match.
        if !m.sem(game_sem::GAME_OVER) {
            self.check_victory(m);
        }
    }

    /// `Leader::process_elimination` @ `0x006B8A20` — the capital-loss timer.
    /// Only runs under `ELIMINATION_CAPITAL`.
    pub fn process_elimination(&mut self, m: &mut Match, who: usize) {
        if m.options.elimination != Elimination::Capital as u8 {
            return;
        }
        if self.slots[who].lost_capital_timer == 0 {
            return;
        }
        let elapsed = m.frame.wrapping_sub(self.slots[who].lost_capital_stamp);
        if m.retake_capital().wrapping_sub(elapsed) > 0 {
            return;
        }
        self.defeat(m, who, DefeatType::Capital, self.slots[who].who, 0);
    }

    /// `Game::check_victory` @ `0x005926B0` — the "last team standing" check.
    ///
    /// Returns `true` if the match is over. The engine calls it from
    /// `Leaders::strategy_all` (`0x006ED483`) only when semaphore bit 9 is set.
    pub fn check_victory(&mut self, m: &mut Match) -> bool {
        if m.sem(game_sem::GAME_OVER) {
            return true;
        }
        // Every alive leader must be mutually allied with the first one found.
        let mut first: Option<usize> = None;
        for i in 0..NUM_LEADERS {
            if !self.slots[i].is_alive() {
                continue;
            }
            match first {
                None => first = Some(i),
                Some(f) => {
                    if !self.is_ally(i, f) {
                        return false;
                    }
                }
            }
        }
        m.set_sem(game_sem::GAME_OVER);
        self.events.push(MatchEvent::GameOver);

        if !m.armageddon_reached() {
            let mut winner: Option<usize> = None;
            for i in 0..NUM_LEADERS {
                if !self.slots[i].is_active() {
                    continue;
                }
                self.slots[i].leader_flags |= leader_flag::SURVIVED;
                if self.slots[i].flag(leader_flag::DEFEATED) {
                    continue;
                }
                match winner {
                    None => winner = Some(i),
                    Some(w) => {
                        if !self.slots[i].flag(leader_flag::WON) && !self.is_ally(i, w) {
                            m.set_sem(game_sem::VICTORY_RESOLVED);
                            return true;
                        }
                    }
                }
            }
            if let Some(w) = winner {
                self.victory(m, w, VictoryType::Generic, 0);
            }
        }
        m.set_sem(game_sem::VICTORY_RESOLVED);
        true
    }

    /// `GameDaemon::process_victory` @ `0x00730EF0` — the per-frame victory sweep,
    /// called from `GameDaemon::process_all` (step 12 of `Game::do_frame`).
    ///
    /// The condition blocks run in this exact order, and several are active under
    /// more than one `info.victory` setting:
    ///
    /// 1. armageddon clock -> `Game::defeat_all`
    /// 2. wonder, under `Standard | SuddenDeath | Wonder`
    /// 3. territory, under `Standard | SuddenDeath | Population`
    /// 4. score goal, under `Score`
    /// 5. time limit, under `TimeLimit`
    /// 6. musical chairs, under `MusicalChairs`
    /// 7. economic, under `Economic`
    ///
    /// `wonder_net` / `wonder_value` are the per-slot outputs of
    /// `LeaderData::get_wonder_net` (`0x006EBB10`) and `get_wonder_value`
    /// (`0x006EBB90`), which are owned by [`super::wonders`]. The live tick supplies
    /// verified values from that registry; other callers remain responsible for these
    /// slices and must not substitute zeroes for active records.
    pub fn process_victory(&mut self, m: &mut Match, wonder_net: &[i32], wonder_value: &[i32]) {
        self.events.clear();

        // 1. Armageddon.
        if m.armageddon_reached() {
            self.defeat_all(m);
        }

        let v = m.options.victory;
        let standardish = v == Victory::Standard as u8 || v == Victory::SuddenDeath as u8;

        // 2. Wonder victory.
        if standardish || v == Victory::Wonder as u8 {
            let winner = self.wonder_winning(m, wonder_net, wonder_value);
            for who in 0..NUM_LEADERS {
                if !self.slots[who].is_active() {
                    continue;
                }
                let qualifies = match winner {
                    None => false,
                    Some(w) => who == w || self.is_ally(who, w),
                };
                if !qualifies {
                    // Cancel a running countdown.
                    if self.slots[who].wonderwin_timer != 0 {
                        self.slots[who].wonderwin_timer = 0;
                        self.slots[who].wonderwin_stamp =
                            self.slots[who].wonderwin_stamp.wrapping_sub(m.frame);
                    }
                    continue;
                }
                let countdown_bypassed = (0..NUM_LEADERS).any(|member| {
                    self.slots[member].flag(leader_flag::VALID)
                        && (member == who || self.is_ally(who, member))
                        && self.slots[member].has_preq_2b9
                });
                let standard_multiplayer_countdown = v == Victory::Standard as u8
                    && (m.sem(game_sem::NET_OR_RECORDING) || m.num_nations > 1)
                    && !countdown_bypassed;
                if !standard_multiplayer_countdown {
                    // Dedicated Wonder, Sudden Death, Standard solo, and the
                    // prerequisite-0x2B9 alliance bypass are immediate.
                    self.victory(
                        m,
                        who,
                        VictoryType::ByWonder,
                        if countdown_bypassed { 1 } else { 0 },
                    );
                    break;
                }
                if self.slots[who].wonderwin_timer == 0 {
                    self.slots[who].wonderwin_timer = 1;
                    self.slots[who].wonderwin_stamp =
                        self.slots[who].wonderwin_stamp.wrapping_add(m.frame);
                } else {
                    let elapsed = m.frame.wrapping_sub(self.slots[who].wonderwin_stamp);
                    if m.wonder_timer().wrapping_sub(elapsed) <= 0 {
                        self.victory(m, who, VictoryType::ByWonder, 0);
                        break;
                    }
                }
            }
        }

        // 3. Territory victory.
        if (standardish || v == Victory::Population as u8) && m.world_land_size != 0 {
            let pct = m.victory_options.popwins[m.options.popwin as usize];
            for who in 0..NUM_LEADERS {
                if !self.slots[who].is_active() {
                    continue;
                }
                let mine = self.get_team_terr(m, who).wrapping_mul(100) / m.world_land_size;
                let mut rivals_over = 0;
                for other in 0..NUM_LEADERS {
                    if other == who || !self.slots[other].is_active() || self.is_ally(who, other) {
                        continue;
                    }
                    if self.get_team_terr(m, other).wrapping_mul(100) / m.world_land_size >= pct {
                        rivals_over += 1;
                    }
                }
                if mine < pct || rivals_over != 0 {
                    if self.slots[who].popwin_timer != 0 {
                        self.slots[who].popwin_timer = 0;
                        self.slots[who].popwin_stamp =
                            self.slots[who].popwin_stamp.wrapping_sub(m.frame);
                    }
                    continue;
                }
                if v == Victory::Population as u8 {
                    self.victory(m, who, VictoryType::ByTerritory, 0);
                    break;
                }
                if self.slots[who].popwin_timer == 0 {
                    self.slots[who].popwin_timer = 1;
                    self.slots[who].popwin_stamp =
                        self.slots[who].popwin_stamp.wrapping_add(m.frame);
                } else {
                    let elapsed = m.frame.wrapping_sub(self.slots[who].popwin_stamp);
                    if m.popwin_timer().wrapping_sub(elapsed) <= 0 {
                        self.victory(m, who, VictoryType::ByTerritory, 0);
                        break;
                    }
                }
            }
        }

        // 4. Score victory.
        if v == Victory::Score as u8 {
            let goal = m.victory_options.scores[m.options.score_goal as usize];
            let mut best = 0i32;
            let mut best_who = 0usize;
            for who in 0..NUM_LEADERS {
                if !self.slots[who].is_active() {
                    continue;
                }
                let s = self.get_team_score(m, who);
                if best < s {
                    best = s;
                    best_who = who;
                }
            }
            if goal <= best {
                self.victory(m, best_who, VictoryType::ByScore, 0);
            }
        }

        // 5. Time limit. `time_limits` is minutes; the engine compares
        //    `Game::tick` (game seconds) against `minutes * 60`.
        if v == Victory::TimeLimit as u8 {
            let limit_min = if (m.options.time_limit as usize) < m.victory_options.time_limits.len()
            {
                m.victory_options.time_limits[m.options.time_limit as usize]
            } else {
                300
            };
            if m.tick >= limit_min.wrapping_mul(60) {
                // Highest team score wins.
                let mut best = 0i32;
                let mut best_who = 0usize;
                for who in 0..NUM_LEADERS {
                    if !self.slots[who].is_active() {
                        continue;
                    }
                    let s = self.get_team_score(m, who);
                    if best < s {
                        best = s;
                        best_who = who;
                    }
                }
                self.victory(m, best_who, VictoryType::ByTimeLimit, 0);
            }
        }

        // 6. Musical chairs: every `chairs[i]` minutes (900 frames per minute),
        //    the team with the fewest members loses its lowest-scoring player.
        if v == Victory::MusicalChairs as u8 && m.frame != 0 {
            let interval = m.victory_options.chairs[m.options.chairs as usize].wrapping_mul(900);
            if m.frame.wrapping_sub(m.musical_chairs) >= interval {
                m.musical_chairs = m.frame;
                self.musical_chairs_cull(m);
            }
        }

        // 7. Economic victory.
        if v == Victory::Economic as u8 {
            let goal = m.victory_options.econwins[m.options.econwin as usize];
            let mut best = 0i32;
            let mut best_who = 0usize;
            for who in 0..NUM_LEADERS {
                if !self.slots[who].is_active() {
                    continue;
                }
                let e = self.get_team_economic(m, who);
                if best < e {
                    best = e;
                    best_who = who;
                }
            }
            if goal <= best {
                self.victory(m, best_who, VictoryType::ByEconomy, 0);
            }
        }
    }

    /// The musical-chairs elimination pass at `0x00731CB8`.
    ///
    /// Free-for-all (`team_style` in {0, 8, 11}) or a single remaining team: the
    /// globally lowest `score` is eliminated, and an exact tie eliminates nobody.
    /// Otherwise the lowest scorer *within each team* is eliminated.
    fn musical_chairs_cull(&mut self, m: &mut Match) {
        let ffa = matches!(m.options.team_style, 0 | 8 | 11);
        if ffa {
            let mut lowest = i32::MAX;
            let mut who: Option<usize> = None;
            let mut tie = true;
            for i in 0..NUM_LEADERS {
                if !self.slots[i].is_active() {
                    continue;
                }
                let s = self.slots[i].score;
                if s <= lowest {
                    tie = s == lowest && who != Some(i) && who.is_some();
                    lowest = s;
                    who = Some(i);
                }
            }
            if !tie {
                if let Some(w) = who {
                    self.defeat(m, w, DefeatType::MusicalChairs, -1, 0);
                }
            }
        } else {
            for team in 0..4usize {
                let mut lowest = 99_999_999i32;
                let mut who: Option<usize> = None;
                let mut tie = true;
                for i in 0..NUM_LEADERS {
                    if !self.slots[i].is_active() {
                        continue;
                    }
                    if self.team_of(i) != team as i32 {
                        continue;
                    }
                    let s = self.slots[i].score;
                    if s <= lowest {
                        tie = s == lowest && who.is_some() && who != Some(i);
                        lowest = s;
                        who = Some(i);
                    }
                }
                if !tie {
                    if let Some(w) = who {
                        self.defeat(m, w, DefeatType::MusicalChairs, -1, 0);
                    }
                }
            }
        }
    }

    /// `LeaderData::get_team` @ `0x006EC040`, projected from the Sim-owned PlayerSetup
    /// image after the atomic setup transaction. Unconfigured leaders default to `who`.
    pub fn team_of(&self, who: usize) -> i32 {
        self.setup_owner.team_of(who)
    }

    /// Emit the whole leader table in `CheckSums::check_all` channel-8 order.
    pub fn walk_bytes(&self, out: &mut Vec<u8>) {
        for l in &self.slots {
            l.walk_bytes(out);
        }
    }
}

/// `adler32` @ `0x00A46830`, the checksum `CheckSums::check_all` folds each channel
/// with. Provided so a replay harness can hash [`Leaders::walk_bytes`] the same way.
/// Re-exported from [`crate::checksum`], the crate's only implementation.
pub use crate::checksum::adler32;

// ---------------------------------------------------------------------------
// Tests. These pin the *arithmetic contract* read out of the instruction
// stream. They are not differential tests against retail — see the report.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn division_helpers_truncate_toward_zero() {
        // `cdq; sub eax,edx; sar eax,1` is /2 toward zero, not an arithmetic shift.
        assert_eq!(half_trunc(7), 3);
        assert_eq!(half_trunc(-7), -3);
        assert_eq!(-7i32 >> 1, -4, "a bare shift would round the wrong way");
        assert_eq!(half_trunc(0), 0);
        assert_eq!(div5_trunc(-9), -1);
        assert_eq!(div3_trunc(-8), -2);
        // (v + (v>>31 & 0xff)) >> 8
        assert_eq!(rescale_256(511), 1);
        assert_eq!(rescale_256(-511), -1);
        assert_eq!(rescale_256(-256), -1);
        assert_eq!(rescale_256(-255), 0);
    }

    fn table() -> TypeTable {
        TypeTable::with_default_kinds(ScoreConstants::default())
    }

    #[test]
    fn type_kind_ranges_match_the_binary() {
        let t = table();
        assert_eq!(t.rows[0x32].kind, TypeKind::Unit);
        assert_eq!(t.rows[0x19D].kind, TypeKind::Unit);
        assert_eq!(t.rows[0x19E].kind, TypeKind::Build);
        assert_eq!(t.rows[0x21E].kind, TypeKind::Build);
        // 0x21F is in neither the building nor the tech range: building is
        // `idx < 0x21F` and tech is `idx > 0x21F`.
        assert_eq!(t.rows[0x21F].kind, TypeKind::Other);
        assert_eq!(t.rows[0x220].kind, TypeKind::Tech);
        assert_eq!(t.rows[0x274].kind, TypeKind::Tech);
        assert_eq!(t.rows[0x275].kind, TypeKind::Spell);
        assert!(!t.rows[0x20D].is_wonder);
        assert!(t.rows[0x20E].is_wonder);
        assert!(t.rows[0x21E].is_wonder);
        assert!(!t.rows[0x21F].is_wonder);
    }

    #[test]
    fn get_score_value_plain_form_is_factor_times_total_cost() {
        let mut t = table();
        t.rows[0x40].costs = [50, 0, 0, 0, 0, 0];
        assert_eq!(t.get_score_value(0x40, 0), 500);
        t.rows[0x1A0].costs = [30, 20, 0, 0, 0, 0];
        assert_eq!(t.get_score_value(0x1A0, 0), 500);
        t.rows[0x230].costs = [0, 0, 100, 40, 0, 0];
        assert_eq!(t.get_score_value(0x230, 0), 1400);
    }

    #[test]
    fn get_score_value_remaps_alternate_citizens_only_when_param_is_zero() {
        let mut t = table();
        t.rows[UNIT_FIRST].costs = [50, 0, 0, 0, 0, 0];
        t.rows[0x43].costs = [999, 0, 0, 0, 0, 0];
        t.rows[0x43].research_premium_cost = 256;
        assert_eq!(
            t.get_score_value(0x43, 0),
            500,
            "remapped to the base citizen"
        );
        // param != 0 uses the type's own costs.
        // 256 * 10 * 999 = 2557440; >>8 = 9990; * 256 = 2557440; >>8 = 9990.
        assert_eq!(t.get_score_value(0x43, 1), 9990);
    }

    #[test]
    fn research_premium_form_is_two_chained_8_8_rescales() {
        let mut t = table();
        t.rows[0x60].costs = [40, 0, 0, 0, 0, 0];
        t.rows[0x60].research_premium_cost = 128; // 0.5 in 8.8
                                                  // 256*10*40 = 102400 -> >>8 = 400 -> *128 = 51200 -> >>8 = 200
        assert_eq!(t.get_score_value(0x60, 1), 200);
    }

    fn one_leader(m: &Match) -> Leaders {
        let mut t = table();
        // A unit type costing 50 food, no support cost, with an attack value.
        t.rows[0x40].costs = [50, 0, 0, 0, 0, 0];
        t.rows[0x40].attack = 5;
        // A building costing 100 timber with support_cost {2,3}.
        t.rows[0x1A0].costs = [0, 100, 0, 0, 0, 0];
        t.rows[0x1A0].support_cost = [2, 3];
        // A wonder costing 200 wealth.
        t.rows[0x20E].costs = [0, 0, 200, 0, 0, 0];
        let mut ls = Leaders::new(t);
        ls.slots[0].leader_flags = leader_flag::VALID | leader_flag::ACTIVE;
        let _ = m;
        ls
    }

    #[test]
    fn unit_score_quadratic_support_term() {
        let m = Match::default();
        let mut ls = one_leader(&m);
        // 4 units of a 50-food type with zero support cost.
        ls.slots[0].num_units[0x40 - UNIT_FIRST] = 4;
        ls.compute_unit_score(&m, 0);
        // base = 500; v = (500*4 + 0)/5 = 400
        assert_eq!(ls.slots[0].score_units, 400);
        assert_eq!(ls.slots[0].score_units_2, 400, "attack != 0 => military");

        // Now give it support cost {1,1} => step 2.
        ls.types.rows[0x40].support_cost = [1, 1];
        ls.compute_unit_score(&m, 0);
        // extra = ((4-1)*2)*4 = 24 -> /2 = 12 ; v = (2000 + 12)/5 = 402
        assert_eq!(ls.slots[0].score_units, 402);
    }

    /// Real rules values: `ron-data/unitrules.xml` `Citizen` has `COST 2f`,
    /// `SUPPORT 1f`. This is the only test here fed by shipped data rather than a
    /// synthetic fixture, so it is the one that pins the *magnitude* of the reward.
    #[test]
    fn citizen_contribution_from_shipped_rules_values() {
        let m = Match::default();
        let mut t = table();
        const CITIZEN: usize = 0x32;
        t.rows[CITIZEN].costs = [2, 0, 0, 0, 0, 0]; // 2f
        t.rows[CITIZEN].support_cost = [1, 0]; // 1f support
        t.rows[CITIZEN].attack = 4;
        let mut ls = Leaders::new(t);
        ls.slots[0].leader_flags = leader_flag::VALID | leader_flag::ACTIVE;

        assert_eq!(ls.types.get_score_value(CITIZEN, 0), 20, "10 * 2f");

        ls.slots[0].num_units[CITIZEN - UNIT_FIRST] = 1;
        ls.compute_unit_score(&m, 0);
        assert_eq!(ls.slots[0].score_units, 4);

        ls.slots[0].num_units[CITIZEN - UNIT_FIRST] = 20;
        ls.compute_unit_score(&m, 0);
        // (20*20 + ((20-1)*1*20)/2) / 5 = (400 + 190)/5 = 118
        assert_eq!(ls.slots[0].score_units, 118);
    }

    #[test]
    fn build_score_splits_wonders_off_at_a_third() {
        let m = Match::default();
        let mut ls = one_leader(&m);
        ls.slots[0].num_buildings[0x1A0 - BUILD_FIRST] = 2;
        ls.slots[0].num_buildings[0x20E - BUILD_FIRST] = 1;
        ls.compute_build_score(&m, 0);
        // building: base = 1000, n = 2, step = (3+2)*1 = 5
        //   extra = ((2-1)*5)*2 = 10 -> /2 = 5 ; v = 2000 + 5 = 2005 ; /5 = 401
        assert_eq!(ls.slots[0].score_buildings, 401);
        // wonder: base = 2000, n = 1, step = 0 ; v = 2000 ; /3 = 666
        assert_eq!(ls.slots[0].score_wonders, 666);
    }

    #[test]
    fn economy_score_clamps_stockpile_but_only_caps_income() {
        let m = Match::default();
        let mut ls = one_leader(&m);
        // stockpile 4000 -> /20 = 200 -> clamped to 100
        ls.slots[0].economy.bucket[0] = 4000;
        // income -800 -> /80 = -10, no lower clamp
        ls.slots[0].economy.income[0] = -800;
        // the obfuscation round-trips
        assert_eq!(
            decrypt_field(decrypt_field(4000, ENC_KEY_BUCKET), ENC_KEY_BUCKET),
            4000
        );
        ls.slots[0].resource_avail = [true, false, false, false, false, false];
        ls.compute_economy_score(&m, 0);
        assert_eq!(ls.slots[0].score_economy, 90);

        // rare resources: flat 25 each
        ls.slots[0].rare = 0b1011;
        ls.compute_economy_score(&m, 0);
        assert_eq!(ls.slots[0].score_economy, 90 + 75);
    }

    #[test]
    fn score_total_excludes_score_units_2() {
        let m = Match::default();
        let mut ls = one_leader(&m);
        ls.slots[0].leader_flags |= leader_flag::NEW_UNITS | leader_flag::NEW_TECH;
        ls.slots[0].num_units[0x40 - UNIT_FIRST] = 4;
        ls.slots[0].territory = 0;
        ls.compute_score(&m, 0, 1);
        assert_eq!(ls.slots[0].score_units, 400);
        assert_eq!(ls.slots[0].score_units_2, 400);
        assert_eq!(
            ls.slots[0].score, 400,
            "the military mirror is not double counted"
        );
    }

    #[test]
    fn territory_score_is_per_mille_of_land() {
        let mut m = Match::default();
        m.world_land_size = 4000;
        let mut ls = one_leader(&m);
        ls.slots[0].territory = 613;
        ls.compute_score(&m, 0, 1);
        assert_eq!(ls.slots[0].score_territory, 613 * 1000 / 4000);
        assert_eq!(ls.slots[0].score, 153);
    }

    #[test]
    fn score_throttle_is_a_ten_frame_round_robin_phased_by_slot() {
        let mut m = Match::default();
        let mut ls = one_leader(&m);
        ls.slots[0].leader_flags |= leader_flag::NEW_UNITS;
        ls.slots[0].num_units[0x40 - UNIT_FIRST] = 4;

        m.frame = 3; // (0 + 3) % 10 != 0 -> no refresh
        ls.compute_score(&m, 0, 0);
        assert_eq!(ls.slots[0].score, 0);

        m.frame = 10; // (0 + 10) % 10 == 0 -> refresh
        ls.compute_score(&m, 0, 0);
        assert_eq!(ls.slots[0].score, 400);

        // slot 3 refreshes on frames where (3 + frame) % 10 == 0
        ls.slots[3].who = 3;
        ls.slots[3].leader_flags =
            leader_flag::VALID | leader_flag::ACTIVE | leader_flag::NEW_UNITS;
        ls.slots[3].num_units[0x40 - UNIT_FIRST] = 4;
        m.frame = 10;
        ls.compute_score(&m, 3, 0);
        assert_eq!(ls.slots[3].score, 0);
        m.frame = 17;
        ls.compute_score(&m, 3, 0);
        assert_eq!(ls.slots[3].score, 400);
    }

    #[test]
    fn game_over_semaphore_bypasses_the_throttle() {
        let mut m = Match::default();
        m.frame = 3;
        m.set_sem(game_sem::GAME_OVER);
        let mut ls = one_leader(&m);
        ls.slots[0].leader_flags |= leader_flag::NEW_UNITS;
        ls.slots[0].num_units[0x40 - UNIT_FIRST] = 4;
        ls.compute_score(&m, 0, 0);
        assert_eq!(ls.slots[0].score, 400);
    }

    #[test]
    fn armageddon_threshold_scales_with_nations_teams_and_resources() {
        let mut m = Match::default();
        m.num_nations = 6;
        m.num_sides = 2;
        // 4 + 1*6 + 2*2 = 14
        assert_eq!(m.get_armageddon(), 14);
        m.options.starting_resources = 7; // Deathmatch
        assert_eq!(m.get_armageddon(), 42);
        m.options.starting_resources = 6; // x20
        assert_eq!(m.get_armageddon(), 28);
        m.options.starting_resources = 8; // Infinite -> floor of 100
        assert_eq!(m.get_armageddon(), 100);
    }

    #[test]
    fn armageddon_zeroes_every_score_component() {
        let mut m = Match::default();
        m.num_nations = 1;
        m.num_sides = 1;
        m.armageddon = m.get_armageddon();
        let mut ls = one_leader(&m);
        ls.slots[0].leader_flags |= leader_flag::NEW_UNITS | leader_flag::NEW_TECH;
        ls.slots[0].num_units[0x40 - UNIT_FIRST] = 40;
        ls.slots[0].territory = 500;
        ls.compute_score(&m, 0, 1);
        assert_eq!(ls.slots[0].score, 0);
        assert_eq!(ls.slots[0].score_units, 0);
        assert_eq!(ls.slots[0].score_territory, 0);
    }

    #[test]
    fn map_scaled_timers_use_the_standard_map_as_the_reference() {
        let mut m = Match::default();
        m.world_xs = 70; // Standard
        assert_eq!(m.wonder_timer(), 4500);
        assert_eq!(m.popwin_timer(), 3600);
        m.world_xs = 140; // twice the reference edge
        assert_eq!(m.wonder_timer(), 9000);
        m.world_xs = 40; // Tiny: (40*4500 + 35)/70
        assert_eq!(m.wonder_timer(), (40 * 4500 + 35) / 70);
    }

    #[test]
    fn diplomacy_is_the_mutual_minimum() {
        let m = Match::default();
        let mut ls = one_leader(&m);
        ls.set_diplo(0, 1, Diplo::Ally);
        ls.set_diplo(1, 0, Diplo::Peace);
        assert_eq!(
            ls.get_diplo(0, 1),
            Diplo::Peace,
            "one-sided alliance is peace"
        );
        assert!(!ls.is_ally(0, 1));
        assert!(ls.is_peace(0, 1));
        assert!(!ls.is_enemy(0, 1));

        ls.set_diplo(1, 0, Diplo::War);
        assert_eq!(
            ls.get_diplo(0, 1),
            Diplo::War,
            "one-sided war is war for both"
        );
        assert!(ls.is_enemy(0, 1));
        assert!(ls.is_enemy(1, 0));
        assert!(!ls.is_peace(0, 1));

        ls.set_diplo(1, 0, Diplo::Ally);
        assert_eq!(ls.get_diplo(0, 1), Diplo::Ally);
        assert!(ls.is_ally(0, 1) && ls.is_ally(1, 0));

        // self
        assert_eq!(ls.get_diplo(2, 2), Diplo::Ally);
        assert!(ls.is_ally(2, 2));
        assert!(!ls.is_enemy(2, 2));
        assert!(!ls.is_peace(2, 2));
    }

    #[test]
    fn check_victory_waits_until_only_one_alliance_remains() {
        let mut m = Match::default();
        let mut ls = one_leader(&m);
        for i in 0..3 {
            ls.slots[i].leader_flags = leader_flag::VALID | leader_flag::ACTIVE;
        }
        assert!(!ls.check_victory(&mut m), "three mutual enemies: no winner");

        // 0 and 1 ally, 2 is defeated -> game over, 0 wins, 1 wins as an ally.
        ls.set_diplo(0, 1, Diplo::Ally);
        ls.set_diplo(1, 0, Diplo::Ally);
        ls.slots[2].leader_flags |= leader_flag::DEFEATED;
        ls.slots[2].leader_flags &= !leader_flag::ACTIVE;
        assert!(ls.check_victory(&mut m));
        assert!(m.sem(game_sem::GAME_OVER));
        assert!(ls.slots[0].flag(leader_flag::WON));
        assert!(ls.slots[1].flag(leader_flag::WON));
    }

    #[test]
    fn victory_defeats_every_non_ally_and_wins_for_allies() {
        let mut m = Match::default();
        let mut ls = one_leader(&m);
        for i in 0..4 {
            ls.slots[i].leader_flags = leader_flag::VALID | leader_flag::ACTIVE;
            ls.slots[i].num_queued[0x40 + i] = (i + 1) as u16;
        }
        ls.set_diplo(0, 1, Diplo::Ally);
        ls.set_diplo(1, 0, Diplo::Ally);
        ls.victory(&mut m, 0, VictoryType::ByWonder, 0);
        assert!(ls.slots[0].flag(leader_flag::WON));
        assert!(ls.slots[1].flag(leader_flag::WON));
        assert_eq!(ls.slots[1].victory_type, VictoryType::ByWonder as i32);
        assert!(ls.slots[2].flag(leader_flag::DEFEATED));
        assert_eq!(ls.slots[2].defeat_type, DefeatType::Victory as i32);
        assert!(ls.slots[3].flag(leader_flag::DEFEATED));
        assert!(ls.slots[..4]
            .iter()
            .all(|leader| leader.num_queued.iter().all(|queued| *queued == 0)));
        assert_eq!(ls.take_terminal_queue_cleanup(), 0b0000_1111);
        assert_eq!(ls.take_terminal_queue_cleanup(), 0);
        assert_eq!(ls.take_defeat_unit_cleanup(), 0b0000_1100);
        assert_eq!(ls.take_defeat_unit_cleanup(), 0);
        assert!(m.sem(game_sem::GAME_OVER));
        assert!(m.sem(game_sem::VICTORY_RESOLVED));
    }

    #[test]
    fn wonder_victory_expiry_executes_terminal_cleanup() {
        let mut m = Match::default();
        m.options.victory = Victory::Standard as u8;
        m.num_nations = 2;
        m.world_xs = 70;
        let mut ls = one_leader(&m);
        ls.slots[1].leader_flags = leader_flag::VALID | leader_flag::ACTIVE;
        ls.slots[0].num_queued[0x20e] = 2;
        ls.slots[1].num_queued[0x1a0] = 3;

        m.frame = 10;
        ls.process_victory(&mut m, &[1, 0, 0, 0, 0, 0, 0, 0], &[1, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(ls.slots[0].wonderwin_timer, 1);
        assert!(!m.sem(game_sem::GAME_OVER));

        m.frame = 10 + m.wonder_timer();
        ls.process_victory(&mut m, &[1, 0, 0, 0, 0, 0, 0, 0], &[1, 0, 0, 0, 0, 0, 0, 0]);

        assert!(ls.slots[0].flag(leader_flag::WON));
        assert!(ls.slots[1].flag(leader_flag::DEFEATED));
        assert!(ls.slots[0].num_queued.iter().all(|queued| *queued == 0));
        assert!(ls.slots[1].num_queued.iter().all(|queued| *queued == 0));
        assert!(m.sem(game_sem::GAME_OVER));
        assert!(m.sem(game_sem::VICTORY_RESOLVED));
    }

    #[test]
    fn wonder_countdown_gate_distinguishes_modes_network_and_prerequisite() {
        let net = [1, 0, 0, 0, 0, 0, 0, 0];

        let mut solo = Match::default();
        solo.options.victory = Victory::Standard as u8;
        let mut solo_leaders = one_leader(&solo);
        solo_leaders.slots[1].leader_flags = leader_flag::VALID | leader_flag::ACTIVE;
        solo_leaders.process_victory(&mut solo, &net, &net);
        assert!(solo_leaders.slots[0].flag(leader_flag::WON));
        assert_eq!(solo_leaders.slots[0].wonderwin_timer, 0);

        let mut recorded = Match::default();
        recorded.options.victory = Victory::Standard as u8;
        recorded.set_sem(game_sem::NET_OR_RECORDING);
        let mut recorded_leaders = one_leader(&recorded);
        recorded_leaders.slots[1].leader_flags = leader_flag::VALID | leader_flag::ACTIVE;
        recorded_leaders.process_victory(&mut recorded, &net, &net);
        assert!(!recorded_leaders.slots[0].flag(leader_flag::WON));
        assert_eq!(recorded_leaders.slots[0].wonderwin_timer, 1);

        let mut sudden = Match::default();
        sudden.options.victory = Victory::SuddenDeath as u8;
        sudden.num_nations = 2;
        let mut sudden_leaders = one_leader(&sudden);
        sudden_leaders.slots[1].leader_flags = leader_flag::VALID | leader_flag::ACTIVE;
        sudden_leaders.process_victory(&mut sudden, &net, &net);
        assert!(sudden_leaders.slots[0].flag(leader_flag::WON));
        assert_eq!(
            sudden_leaders.slots[0].leader_flags2 & leader_flag2::INSTANT_VICTORY,
            0
        );

        let mut bypass = Match::default();
        bypass.options.victory = Victory::Standard as u8;
        bypass.num_nations = 2;
        let mut bypass_leaders = one_leader(&bypass);
        bypass_leaders.slots[1].leader_flags = leader_flag::VALID | leader_flag::ACTIVE;
        bypass_leaders.slots[0].has_preq_2b9 = true;
        bypass_leaders.process_victory(&mut bypass, &net, &net);
        assert!(bypass_leaders.slots[0].flag(leader_flag::WON));
        assert_ne!(
            bypass_leaders.slots[0].leader_flags2 & leader_flag2::INSTANT_VICTORY,
            0
        );
        assert_eq!(bypass_leaders.slots[0].wonderwin_timer, 0);
    }

    #[test]
    fn defeat_restarts_the_musical_chairs_interval() {
        let mut m = Match::default();
        m.frame = 1234;
        let mut ls = one_leader(&m);
        ls.defeat(&mut m, 0, DefeatType::Resign, -1, 0);
        assert_eq!(m.musical_chairs, 1234);
        assert!(!ls.slots[0].is_active());
        assert!(ls.slots[0].flag(leader_flag::DEFEATED));
        assert_eq!(ls.take_terminal_queue_cleanup(), 1);
        assert_eq!(ls.take_defeat_unit_cleanup(), 1);
    }

    #[test]
    fn capital_elimination_waits_for_the_scaled_retake_timer() {
        let mut m = Match::default();
        m.options.elimination = Elimination::Capital as u8;
        m.world_xs = 70;
        let mut ls = one_leader(&m);
        ls.slots[0].lost_capital_timer = 1;
        ls.slots[0].lost_capital_stamp = 100;

        m.frame = 100 + 3599;
        ls.process_elimination(&mut m, 0);
        assert!(!ls.slots[0].flag(leader_flag::DEFEATED));

        m.frame = 100 + 3600;
        ls.process_elimination(&mut m, 0);
        assert!(ls.slots[0].flag(leader_flag::DEFEATED));
        assert_eq!(ls.slots[0].defeat_type, DefeatType::Capital as i32);
    }

    #[test]
    fn score_victory_fires_at_the_configured_goal() {
        let mut m = Match::default();
        m.options.victory = Victory::Score as u8;
        m.options.score_goal = 0; // 1000
        let mut ls = one_leader(&m);
        ls.slots[0].leader_flags = leader_flag::VALID | leader_flag::ACTIVE;
        ls.slots[1].leader_flags = leader_flag::VALID | leader_flag::ACTIVE;
        ls.slots[0].score = 999;
        ls.process_victory(&mut m, &[0; 8], &[0; 8]);
        assert!(!ls.slots[0].flag(leader_flag::WON));
        ls.slots[0].score = 1000;
        ls.process_victory(&mut m, &[0; 8], &[0; 8]);
        assert!(ls.slots[0].flag(leader_flag::WON));
        assert_eq!(ls.slots[0].victory_type, VictoryType::ByScore as i32);
    }

    #[test]
    fn territory_victory_needs_the_countdown_under_standard_rules() {
        let mut m = Match::default();
        m.options.victory = Victory::Standard as u8;
        m.options.popwin = 4; // 50%
        m.world_land_size = 1000;
        m.world_xs = 70; // popwin_timer = 3600
        let mut ls = one_leader(&m);
        ls.slots[0].leader_flags = leader_flag::VALID | leader_flag::ACTIVE;
        ls.slots[1].leader_flags = leader_flag::VALID | leader_flag::ACTIVE;
        ls.slots[0].territory = 600; // 60%
        ls.slots[1].territory = 100;

        m.frame = 10;
        ls.process_victory(&mut m, &[0; 8], &[0; 8]);
        assert_eq!(ls.slots[0].popwin_timer, 1, "countdown armed, not won");
        assert!(!ls.slots[0].flag(leader_flag::WON));

        m.frame = 10 + 3599;
        ls.process_victory(&mut m, &[0; 8], &[0; 8]);
        assert!(!ls.slots[0].flag(leader_flag::WON));

        m.frame = 10 + 3600;
        ls.process_victory(&mut m, &[0; 8], &[0; 8]);
        assert!(ls.slots[0].flag(leader_flag::WON));
        assert_eq!(ls.slots[0].victory_type, VictoryType::ByTerritory as i32);
    }

    #[test]
    fn territory_countdown_cancels_when_a_rival_also_qualifies() {
        let mut m = Match::default();
        m.options.victory = Victory::Standard as u8;
        m.options.popwin = 0; // 30%
        m.world_land_size = 1000;
        let mut ls = one_leader(&m);
        ls.slots[0].leader_flags = leader_flag::VALID | leader_flag::ACTIVE;
        ls.slots[1].leader_flags = leader_flag::VALID | leader_flag::ACTIVE;
        ls.slots[0].territory = 400;
        ls.slots[1].territory = 400;
        m.frame = 10;
        ls.process_victory(&mut m, &[0; 8], &[0; 8]);
        assert_eq!(ls.slots[0].popwin_timer, 0);
        assert_eq!(ls.slots[1].popwin_timer, 0);
    }

    #[test]
    fn mvp_score_weights_combat_and_doubles_the_winner() {
        let mut m = Match::default();
        let mut ls = one_leader(&m);
        let l = &mut ls.slots[0];
        l.score = 1000;
        l.score_combat = 100;
        l.score_economy = 200;
        l.score_buildings = 300;
        l.score_territory = 50;
        // 100*3 - 100 - 150 + 50 + 1000 = 1100
        assert_eq!(ls.get_mvp_score(&m, 0), 1100);
        ls.slots[0].leader_flags |= leader_flag::WON;
        assert_eq!(ls.get_mvp_score(&m, 0), 2200);
        ls.slots[0].leader_flags &= !leader_flag::WON;
        ls.slots[0].leader_flags |= leader_flag::SURVIVED;
        m.set_sem(game_sem::GAME_OVER);
        assert_eq!(ls.get_mvp_score(&m, 0), 1100 * 5 / 4);
    }

    #[test]
    fn team_score_averages_only_with_the_team_semaphore() {
        let mut m = Match::default();
        let mut ls = one_leader(&m);
        for i in 0..2 {
            ls.slots[i].leader_flags = leader_flag::VALID | leader_flag::ACTIVE;
        }
        ls.set_diplo(0, 1, Diplo::Ally);
        ls.set_diplo(1, 0, Diplo::Ally);
        ls.slots[0].score = 1000;
        ls.slots[1].score = 500;
        assert_eq!(ls.get_team_score(&m, 0), 1000);
        m.set_sem(game_sem::TEAM_SCORING);
        assert_eq!(ls.get_team_score(&m, 0), 750);
    }

    #[test]
    fn walk_bytes_is_stable_and_hashable() {
        let m = Match::default();
        let ls = one_leader(&m);
        let mut a = Vec::new();
        ls.walk_bytes(&mut a);
        m.walk_bytes(&mut a);
        let h1 = adler32(1, &a);
        let mut b = Vec::new();
        ls.walk_bytes(&mut b);
        m.walk_bytes(&mut b);
        assert_eq!(a, b);
        assert_eq!(h1, adler32(1, &b));
        // adler32 of the empty message with seed 1 is 1
        assert_eq!(adler32(1, &[]), 1);
        // known vector: adler32("Wikipedia") == 0x11E60398
        assert_eq!(adler32(1, b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn production_ai_and_attrition_words_are_walked_in_leaderdata_order() {
        let mut leader = LeaderState::default();
        leader.production_step = 0x0102_0304;
        leader.prod_script_run = 0x1112_1314;
        leader.script_step = 0x2122_2324;
        leader.victory_type = 0x3132_3334;
        leader.give_attrition_disabled = 0x1122_3344;
        leader.take_attrition_disabled = 0x2132_4354;
        leader.neutral_attrition = 0x3142_5364;
        leader.building_attrition_disabled = 0x4152_6374;
        leader.cities_captured = 0x4556_6778;
        leader.cities_lost = 0x495A_6B7C;
        leader.control = 0x4D5E_6F70;
        leader.territory = 0x5162_7384;
        leader.effective_pop = 0x5566_7788;
        leader.strategy[0] = 0x1234;
        leader.strategy[63] = 0xabcd;

        let mut walked = Vec::new();
        leader.walk_bytes(&mut walked);
        // The three adjacent +0x788..+0x790 words precede `victory_type`. Seven compact
        // words then separate that marker from the +0x7F8 attrition start.
        let ally_mask = walked.len() - 1;
        let strategy_start =
            ally_mask - crate::systems::army_do_mustering::MUSTER_STRATEGY_REGIONS * 2;
        let attrition_start = strategy_start - 9 * 4;
        let production_start = attrition_start - 7 * 4;
        let mut production_expected = Vec::new();
        for value in [
            leader.production_step,
            leader.prod_script_run,
            leader.script_step,
            leader.victory_type,
        ] {
            production_expected.extend_from_slice(&value.to_le_bytes());
        }
        assert_eq!(
            &walked[production_start..production_start + production_expected.len()],
            production_expected.as_slice()
        );

        // The later canonical AI words retain their real PDB order around `territory`:
        // control +0x940, territory +0x9D8, effective_pop +0x9E0, then strategy +0xA68.
        let tail = &walked[attrition_start..strategy_start];
        let mut expected = Vec::new();
        for value in [
            leader.give_attrition_disabled,
            leader.take_attrition_disabled,
            leader.neutral_attrition,
            leader.building_attrition_disabled,
            leader.cities_captured,
            leader.cities_lost,
            leader.control,
            leader.territory,
            leader.effective_pop,
        ] {
            expected.extend_from_slice(&value.to_le_bytes());
        }
        assert_eq!(tail, expected.as_slice());
        let mut strategy_expected = Vec::new();
        for value in leader.strategy {
            strategy_expected.extend_from_slice(&value.to_le_bytes());
        }
        assert_eq!(
            &walked[strategy_start..ally_mask],
            strategy_expected.as_slice()
        );
    }
}
