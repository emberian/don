//! The arena: one world model, several players, headless and deterministic.
//!
//! # Why this exists
//!
//! `don-ai` had **two** world models and they had never met. [`crate::game::Game`] is the
//! order-driven economic world the transcribed `economic.bhs` plays — real shipped costs,
//! real tech tree, `don-sim`'s derived gather loop, and **no map, no opponent, no
//! combat**. [`crate::optimum::econ::World`] is a scheduling abstraction the build-order
//! optimiser searches over — servers and completion times, not entities at all. The
//! optimiser-derived player `CapFirst` reaches its boom 46 s earlier than the shipped
//! order *in the model it was tuned on*, which is not a result about a game.
//!
//! This module is the arena those two claims have to survive. It keeps everything the
//! economic game got right (shipped costs, shipped tech tree, `don-sim`'s
//! `resource_tick`/`credit_resource`, the `Order`/`OrderResult` tri-state) and adds the
//! four things a *game* needs and neither model had: **a map, an opponent, units that
//! move, and damage**.
//!
//! # What is derived and what is ours
//!
//! Derived, and called rather than reimplemented:
//!
//! | thing | source |
//! |---|---|
//! | resource accumulation | `don_sim::mechanics::{resource_tick, credit_resource, commerce_cap}` |
//! | damage | `don_sim::mechanics::damage` — the whole 31-step chain |
//! | damage scaling / hit points | `don_sim::systems::combat::{scale_damage_unit, scale_damage_build, HitPoints}` |
//! | range tests | `don_sim::systems::combat::{in_attack_range, below_min_range, vector_dist_between}` |
//! | recharge | `don_sim::systems::combat::{recharge_frames, AttackCycle}` |
//! | unit orders / route search / turning | `don_sim::systems::{order_dispatch, movement}` |
//! | per-guy occupancy / unit collision response | `don_sim::systems::collision` |
//! | idle unit target acquisition | `don_sim::systems::target::find_auto_target` |
//! | balance percentages | `schema/live/balance-real.bin`, the real 493x493 table |
//! | unit and building stats | `schema/live/live-tables-*.tsv`, live reads |
//! | rules constants | `ron-data/rules.xml` |
//!
//! Ours, and each one a **model choice, not a fidelity claim**, marked `MODEL n` at its
//! site:
//!
//! 3. Gather slots come from the terrain under the building, capped at the numbers the
//!    shipped script's own arithmetic implies (Farm 1, Camp 5).
//! 6. No water, naval, air or diplomacy. Supply source traversal, siege reload selection,
//!    isolated French/Versailles healing, the 32-frame attrition reset/friendly return and
//!    due-frame attrition mutation are wired into the live unit band. Non-friendly period
//!    selection and other healing families remain explicit blockers.
//!
//! Construction no longer fabricates a builder-frame countdown.  Arena persists the
//! recovered `BuildData` and `(who,o,uid)` order identity, installs `BUILD_AT` through
//! `don-sim`, and runs the retail unit-then-building object bands. The first missing retail
//! world transaction is recorded on the site and stops progress before any state is
//! guessed. This is integration of recovered Tier-C
//! structure, not a promotion of its fidelity tier.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use don_sim::balance::BalanceTable;
use don_sim::command::QueuePos;
use don_sim::mechanics::{
    commerce_cap, credit_resource, damage_traced, get_armor, get_attack, resource_period,
    resource_tick, CommerceCapGates, DamageInput, DamagePredicates, EconomyRules,
    ResourceTickInput,
};
use don_sim::objects::{BANDED_SLOTS, BUILD_BAND_BASE, OWNER_SLOTS, WALL_BAND_BASE};
use don_sim::rng::Random;
use don_sim::systems::casters_animals::{
    CLOAK_OBJECT_FLAG, CLOAK_SECONDARY_OBJECT_FLAG, CLOAK_TYPE_FLAG, CLOAK_WHILE_IDLE_TYPE_FLAG,
};
use don_sim::systems::collision::{
    self, CollCheck, CollGuy, UnitRow as CollisionUnit, UnitTable as CollisionUnits,
};
use don_sim::systems::combat::{
    below_min_range, circle_table, in_attack_range, poor_target, scale_damage_build,
    scale_damage_unit, vector_dist_between, AttackCycle, CircleTable, CombatConstants,
    DamageOutcome, HitPoints, PoorTargetInput, RANGE_UNITS_PER_TILE,
};
use don_sim::systems::construction::{
    self, BuildOrderRegions, BuildOrderTarget, ConstructionFrame, ObjectKey,
};
use don_sim::systems::construction_builder::{self, PreflightInput, PreflightPlan};
use don_sim::systems::fight::{plan_direct_land_volley, AimMode, UnitVolleyInput, UnitVolleyPlan};
use don_sim::systems::gather_lifecycle::OrdinaryGatherKind;
use don_sim::systems::groups_guys::{GuyEnv, UnitGuys, UnitTypeStats};
use don_sim::systems::held_target::{
    attack_distance, AttackDistanceInput, AttackDistanceMode, ObjectFootprint,
};
use don_sim::systems::movement::{PathFinder, PathUnit, UnitWorld, UCELL};
use don_sim::systems::order_dispatch::{
    self, ArmResult, AttackOutcome, DispatchCoverage, GatherOutcome, KillReason, OrderRec,
    TargetState, UnitWork, WorkWorld,
};
use don_sim::systems::production::{self, BuildData, ConstructTimeGates, ProdRules};
use don_sim::systems::target::{
    self, AutoTargetAdapter, AutoTargetCandidate, AutoTargetDistanceFacts, AutoTargetQuery,
    AutoTargetStep, CompareTargetInput, ObjRef, TargetRow, TargetWorld,
};

use super::cmd::{Cmd, EntId};
use super::gather_runtime::{
    ArenaGatherRuntime, GatherCapacityAuthority, GatherObjectKey, GatherPrerequisiteRefusal,
};
use super::map::{Map, Spatial, Terrain};
use super::retail_systems::{
    self, ArenaAttritionRecomputeHost, ArenaReloadSupplyHost, ArenaSupplyAttritionHost,
    ArenaSupplyHealingHost, AttritionRecomputeTransaction, HeroRadiusFacts, HeroRegistryRecord,
    ReloadSupplyState, SupplyAttritionTransaction, SupplyAttritionUnitState,
    SupplyHealingTransaction, SupplyRadiusFacts, SupplyRegistryRecord, SupplySearchObject,
    RESUPPLIED_THIS_TICK, SUPPORT_REGISTRY_ACTIVE,
};
use super::types::{Roster, TypeRow, Types};
use crate::orders::OrderResult;
use crate::rules::NRES;

/// Simulation frames per second, as everywhere else in `don-ai`: `rules.xml` defines
/// `JOB_TIME` in fifteenths of a second and `was_city_attacked` divides a frame delta by
/// 15 to get seconds.
pub const FPS: i64 = 15;

/// Half a tile, in world units — where an entity stands inside its tile.
const HALF: i32 = RANGE_UNITS_PER_TILE / 2;

/// `TypeIndex` constants the arena refers to by name. Every one is checked against the
/// loaded table by [`Ids::resolve`], so a renamed or renumbered type is a loud failure
/// rather than a silently different game.
#[derive(Clone, Copy, Debug)]
pub struct Ids {
    pub citizen: i32,
    pub small_city: i32,
    pub farm: i32,
    pub camp: i32,
    pub mine: i32,
    pub library: i32,
    pub market: i32,
    pub barracks: i32,
    pub temple: i32,
    pub tower: i32,
    pub university: i32,
    pub classical_age: i32,
    pub city_state: i32,
    pub barter: i32,
    pub written_word: i32,
    pub art_of_war: i32,
}

impl Ids {
    pub fn resolve(t: &Types) -> Result<Ids, String> {
        let find = |name: &str, building: bool| -> Result<i32, String> {
            t.rows
                .values()
                .filter(|r| r.name == name && r.kind_building == building)
                .map(|r| r.id)
                .min()
                .ok_or_else(|| {
                    format!(
                        "no {} type named {name:?}",
                        if building { "building" } else { "type" }
                    )
                })
        };
        Ok(Ids {
            citizen: find("Citizen", false)?,
            small_city: find("Small City", true)?,
            farm: find("Farm", true)?,
            camp: find("Woodcutter's Camp", true)?,
            mine: find("Mine", true)?,
            library: find("Library", true)?,
            market: find("Market", true)?,
            barracks: find("Barracks", true)?,
            temple: find("Temple", true)?,
            tower: find("Tower", true)?,
            university: find("University", true)?,
            classical_age: find("Classical Age", false)?,
            city_state: find("City State", false)?,
            barter: find("Barter", false)?,
            written_word: find("Written Word", false)?,
            art_of_war: find("The Art of War", false)?,
        })
    }
}

fn ordinary_gather_kind(ids: &Ids, type_id: i32) -> Option<OrdinaryGatherKind> {
    if type_id == ids.farm {
        Some(OrdinaryGatherKind::Farm)
    } else if type_id == ids.camp {
        Some(OrdinaryGatherKind::Camp)
    } else if type_id == ids.mine {
        Some(OrdinaryGatherKind::Mine)
    } else {
        None
    }
}

fn gather_key(e: &Ent) -> GatherObjectKey {
    GatherObjectKey {
        owner: e.who,
        o: e.object_o,
    }
}

/// Why a placement is illegal. Returned rather than a bool because a bot that cannot tell
/// "no room here" from "wrong terrain" cannot search for a site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceErr {
    OffMap,
    Terrain,
    Occupied,
    NoCityInRange,
    TooCloseToCity,
    NoResource,
}

/// What an entity is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Job {
    Idle,
    MoveTo { x: i32, y: i32 },
    Attack { target: EntId },
    Gather { target: EntId },
    Work { target: EntId },
}

/// The first mandatory retail transaction which prevented a construction activation.
///
/// This is persistent, observable simulation state rather than a log string.  Arena may
/// not turn an absent world host into a successful callback with an empty receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstructionRefusal {
    /// `BuildTypeData::blocked_site` needs terrain, territory, city/dock and adjacency
    /// stores which Arena does not yet materialise in the retail layout.
    MissingBlockedSiteTransaction,
    /// The exact unit-side plan requested the temporary Group/reswarm transaction.  The
    /// arena movement host has not yet ported `action_swarm_around(BUILD_AT)`.
    MissingReswarmTransaction,
    /// `Unit::set_anim` and its complete Guy/group receipt are not available. Facing is
    /// not committed unless that ordered animation transaction is also available.
    MissingBuilderAnimationTransaction,
    /// The target reached the activation threshold, but the full `Build::activate` world
    /// graph (leaders/cities/terrain/events/RNG/checksums) is unavailable.
    MissingActivationTransaction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MoveProgress {
    Working,
    Arrived,
    Failed,
}

/// One queued production item.
#[derive(Clone, Copy, Debug)]
pub struct QueueItem {
    pub type_id: i32,
    pub frames_left: i32,
}

/// One object. Units and buildings are the same struct because in the engine they are the
/// same class hierarchy under `Object`, and `get_damage` reads both through it.
#[derive(Clone, Debug)]
pub struct Ent {
    pub id: EntId,
    pub who: u8,
    /// `ObjectData::o`: the owner-local index in retail's banded object array. Units
    /// occupy `[0, 2000)` and buildings `[2000, 3000)` (`Objects::init` 0x0065EA80).
    /// `EntId` remains Arena's dense storage handle and must never substitute for this.
    pub object_o: i16,
    pub type_id: i32,
    /// World units (tile * 192 + 96).
    pub x: i32,
    pub y: i32,
    pub hp: HitPoints,
    pub alive: bool,
    pub building: bool,
    /// A building still under construction is not active: it does not gather, produce,
    /// or satisfy a `WHERE`.
    pub complete: bool,
    /// Compatibility observation derived from `BuildData::{constr_time,job_counter}`.
    /// It is never decremented as an independent construction model.
    pub build_left: i32,
    /// `ObjectData::uid` (`+0x30`): `Object::init` 0x00647750 copies and increments the
    /// owner's wrapping 16-bit counter at `Objects +0x1D4`. Object slots do not recycle
    /// in this host, but the token remains independent from both `EntId` and `object_o`.
    pub object_uid: u16,
    /// The full recovered construction record for buildings. Units store `None`.
    pub build: Option<BuildData>,
    /// Exact target payload installed by `Unit::add_build_order` for an incomplete site.
    /// Repair orders remain on their separate arena path and therefore store `None`.
    pub build_order: Option<BuildOrderTarget>,
    /// Fail-closed reason for a site whose next mandatory world transaction is absent.
    pub construction_refusal: Option<ConstructionRefusal>,
    pub job: Job,
    pub cycle: AttackCycle,
    /// 32-bit turn units. Set when the entity moves or fires.
    pub facing: i32,
    /// `UnitData +0xB1`: the persistent per-instance combat stance.  This is initialised
    /// through retail's type-class/player-preference switch, not from an arena-wide combat
    /// policy. Buildings retain zero because ordinary `find_new_target` is a Unit path.
    pub stance: i8,
    /// Which of its owner's cities this building belongs to, or `NONE`.
    pub city: EntId,
    /// Gatherer buildings: seated citizens and the terrain-derived cap.
    pub workers: i32,
    pub worker_cap: i32,
    /// Which resource slot a gatherer yields into.
    pub gather_res: usize,
    pub queue: Vec<QueueItem>,
    /// For a citizen: the building it is seated at or building.
    pub assigned_to: EntId,
    pub last_damaged: i64,
    pub spawn_frame: i64,
    /// `UnitData::attrition` `+0x9E`, the due period in frames. It is real per-unit state;
    /// zero disables the due branch. Arena executes the 32-frame universal reset and
    /// friendly-territory return; ordinary spawns retain zero because non-friendly period
    /// selection still requires unavailable diplomacy/leader/object-graph authority.
    pub attrition_period: i16,
    /// The persistent retail `UnitData` slice consumed by `Unit::do_move`: order queue,
    /// waypoint stack, parked A* state, body, collision bookkeeping, and movement masks.
    /// Buildings have no unit-motion state.
    pub motion: Option<UnitWork>,
    /// Retail allocates `squad_size + crew_size` `GuyData` records per unit.  Keeping the
    /// real records is required by collision, which stamps each live squad guy separately.
    pub guys: UnitGuys,
}

impl Ent {
    pub fn tile(&self) -> (i32, i32) {
        (self.x / RANGE_UNITS_PER_TILE, self.y / RANGE_UNITS_PER_TILE)
    }
    pub fn hits_left(&self) -> i32 {
        self.hp.hits_left()
    }
}

/// One player.
#[derive(Clone, Debug)]
pub struct PlayerState {
    pub who: u8,
    pub tribe: u8,
    pub name: String,
    pub stock: [i32; NRES],
    /// The engine's per-resource accumulator, `econ[0x18 + res*4]`.
    pub acc: [i32; NRES],
    /// Cumulative gathered, for the timeout scoreboard.
    pub gathered: [i64; NRES],
    pub techs: BTreeSet<i32>,
    pub alive: bool,
    pub defeat_frame: Option<i64>,
    pub orders_ok: u32,
    pub orders_refused: u32,
    pub orders_invalid: u32,
    /// Tiles ever seen.
    pub explored: Vec<bool>,
    /// Tiles currently in line of sight.
    pub visible: Vec<bool>,
    /// Last known state of an enemy entity. Cleared when the tile is visible and empty.
    pub memory: BTreeMap<EntId, Sighting>,
    pub damage_dealt: i64,
    pub damage_taken: i64,
    pub kills: u32,
    pub losses: u32,
    /// `leaders[who] & 4`. Arena players are bot-controlled leaders; target comparison and
    /// the Unit constructor both read this bit.
    pub leader_ai: bool,
    /// Retail player option `+0x04`, copied into stance-type-1 units for an AI leader.
    pub stance_type_1: i8,
    /// Retail player option `+0x0C`, copied into stance-type-0 units.
    pub stance_type_0: i8,
    /// Retail player option byte `+0x1C`. Bits 3 and 4 initialise stance types 3 and 2.
    pub leader_option_flags: u8,
}

/// What a player remembers about an enemy object.
#[derive(Clone, Copy, Debug)]
pub struct Sighting {
    pub id: EntId,
    pub who: u8,
    pub type_id: i32,
    pub tx: i32,
    pub ty: i32,
    pub frame: i64,
    pub building: bool,
}

/// Model knobs. Everything the arena has to choose because retail derives it from
/// something we do not have.
#[derive(Clone, Copy, Debug)]
pub struct ArenaParams {
    /// See [`crate::game::ModelParams::gather_period_shift`] — the single open
    /// calibration number in the economy. `0` is the setting that makes wall-clock
    /// economy behave the way the shipped game plays; `4` is the literal derived value
    /// and produces an economy far too slow to play. **Neither is verified.**
    pub gather_period_shift: u32,
    /// Whether `LeaderData::get_gather_handicap`'s income bonus is applied. **Off**, and
    /// that is a deliberate choice for this lane: an AI that is good because it is richer
    /// is not an AI. See the report.
    pub difficulty_income_bonus: i32,
    /// Frames between fog recomputations.
    pub fog_period: i64,
    /// Minimum tile separation between city centres. Ours: `CITY_CENTER_RADIUS`, the
    /// radius inside which a city owns its buildings, so two cities at that spacing have
    /// touching rather than overlapping build areas.
    pub min_city_sep: i32,
    /// Construction authority boundary. The playable arena remains explicitly research
    /// modelled until the mandatory world transactions exist; claim-bearing runs select
    /// `FailClosedRetail` and stop at the first absent callback.
    pub construction_mode: ConstructionMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstructionMode {
    /// Use recovered local state/arithmetic, but retain Arena's incomplete flags-only
    /// start/activation so development matches remain playable. This is not fidelity.
    ResearchModel,
    /// Refuse before any missing builder/placement/lifecycle world transaction.
    FailClosedRetail,
}

impl Default for ArenaParams {
    fn default() -> Self {
        ArenaParams {
            gather_period_shift: 0,
            difficulty_income_bonus: 0,
            fog_period: 15,
            min_city_sep: 20,
            construction_mode: ConstructionMode::ResearchModel,
        }
    }
}

/// One thing worth recording.
#[derive(Clone, Debug)]
pub struct Event {
    pub frame: i64,
    pub who: u8,
    pub text: String,
}

/// The arena.
pub struct World {
    pub types: Types,
    pub ids: Ids,
    pub roster: Vec<Roster>,
    pub map: Map,
    pub spatial: Spatial,
    pub ents: Vec<Ent>,
    pub players: Vec<PlayerState>,
    pub frame: i64,
    pub balance: BalanceTable,
    pub combat: CombatConstants,
    pub econ: EconomyRules,
    /// Shipped production constants consumed by the recovered construction kernels.
    pub prod_rules: ProdRules,
    pub params: ArenaParams,
    pub log: Vec<Event>,
    pub logging: bool,
    /// Flank levels observed, indexed 0..=2. A measurement, not a control.
    pub flank_hist: [u64; 3],
    pub shots: u64,
    /// Diagnostic trace of the most recent `Objects::process_all` pass. It records dense
    /// Arena handles in the exact order their owner-local slots were processed and is not
    /// checksummed simulation state.
    pub last_object_process_order: Vec<EntId>,
    /// `(player, verb, result)` -> count. A bot that spends half its commands on orders
    /// the world refuses is broken in a way no score line shows, so the arena counts them
    /// by verb rather than in one bucket.
    pub rejects: BTreeMap<(u8, &'static str, &'static str), u32>,
    /// Retail has one shared pathfinder singleton and one main simulation RNG.  Both are
    /// persistent here so a failed search consumes the same stream later call sites use.
    pathfinder: PathFinder,
    game_random: Random,
    pub movement_coverage: DispatchCoverage,
    collision_world: don_sim::systems::map_terrain::World,
    collision_check: CollCheck,
    collision_units: CollisionUnits,
    /// Exact persistent `(who,o,uid)` ordinary-gather state. The built-in map generator
    /// fails its retail prerequisite preflight and continues only through the separately
    /// labelled MODEL 3 gameplay path below.
    gather_runtime: ArenaGatherRuntime,
    /// `World::wdata`'s target-acquisition chains and checksummed near/targeted fields.
    target_world: TargetWorld,
    target_circle: CircleTable,
    /// Owner-local walked registries used by `Supplies::find_supply` and
    /// `HeroesData::find_hero`. Slots are stable and inactive holes are reused before the
    /// arrays grow, matching the retail init/close transactions.
    supply_records: Vec<Vec<SupplyRegistryRecord>>,
    hero_records: Vec<Vec<HeroRegistryRecord>>,
    age_techs: Vec<i32>,
    /// Per-owner source for `ObjectData::uid`. `Objects::clear`/`Objects::init` zero the
    /// ten counters and `Object::init` 0x00647750 increments the selected `u16` counter.
    next_object_uid: Vec<u16>,
}

fn target_ref(e: &Ent) -> ObjRef {
    ObjRef::new(e.object_o, e.who as i16)
}

fn object_ref_index(ents: &[Ent], r: ObjRef) -> Option<usize> {
    ents.iter()
        .position(|e| e.alive && e.object_o == r.o && i16::from(e.who) == r.who)
}

fn target_row(e: &Ent, t: &TypeRow, ids: &Ids) -> TargetRow {
    TargetRow {
        flags8: 1 | if e.type_id == ids.small_city { 0x20 } else { 0 },
        x: e.x,
        y: e.y,
        damage: e.hp.damage,
        full: 0,
        is_unit: !e.building,
        is_build: e.building,
        is_building: e.building,
        is_wonder: t.kind_building && (0x20E..0x21F).contains(&t.id),
        ..TargetRow::default()
    }
}

/// `Unit::init` `0x00612100`, after `UnitTypeData::get_stance_type` `0x0061D350`.
fn initial_unit_stance(player: &PlayerState, t: &TypeRow) -> i8 {
    match t.stance_type() {
        0 => player.stance_type_0,
        1 if player.leader_ai => player.stance_type_1,
        // The human type-1 arm also depends on GameData +0x2D. Arena leaders are AI, so
        // accepting a human leader here without that setting would fabricate a stance.
        1 => panic!("human stance-type-1 construction needs the live GameData +0x2D option"),
        2 => i8::from(player.leader_option_flags & 0x10 == 0),
        3 => i8::from(player.leader_option_flags & 0x08 == 0),
        // `get_stance_type == -1` takes no constructor write; the freshly zeroed Unit
        // instance therefore retains stance zero.
        -1 => 0,
        other => unreachable!("get_stance_type returned impossible category {other}"),
    }
}

fn type_is(types: &Types, mut type_id: i32, ancestor: i32) -> bool {
    for _ in 0..types.rows.len().saturating_add(1) {
        if type_id == ancestor {
            return true;
        }
        let Some(row) = types.get(type_id) else {
            return false;
        };
        if row.from < 0 || row.from == type_id {
            return false;
        }
        type_id = row.from;
    }
    false
}

fn root_type<'a>(types: &'a Types, mut type_id: i32) -> Option<&'a TypeRow> {
    for _ in 0..types.rows.len().saturating_add(1) {
        let row = types.get(type_id)?;
        if row.from < 0 || row.from == type_id {
            return Some(row);
        }
        type_id = row.from;
    }
    None
}

fn type_chain_has_build_flag(types: &Types, mut type_id: i32, flag: u32) -> bool {
    for _ in 0..types.rows.len().saturating_add(1) {
        let Some(row) = types.get(type_id) else {
            return false;
        };
        if row.build_flags & flag != 0 {
            return true;
        }
        if row.from < 0 || row.from == type_id {
            return false;
        }
        type_id = row.from;
    }
    false
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArenaSupplyHostError {
    InvalidOwner(i32),
    MissingObject { who: i32, o: i32 },
    MissingMotion { who: i32, o: i32 },
    MissingType(i32),
    StaleUnitMasks2 { expected: u32, found: u32 },
    StaleDamage { expected: i32, found: i32 },
    StaleUnitMasks { expected: u32, found: u32 },
    StaleAttritionPeriod { expected: i16, found: i16 },
    UnsupportedUberDamage { type_id: i32, uber_size: i32 },
    UnsupportedUberHealing { type_id: i32, uber_size: i32 },
    PositionOutsideWorld { x: i32, y: i32 },
}

/// A direct view of the live Arena object tables for one retail supply/attrition
/// transaction. No result is cached across the transaction's mutation boundary.
struct ArenaSupplyHost<'a> {
    ents: &'a mut [Ent],
    types: &'a Types,
    players: &'a [PlayerState],
    supply_records: &'a [Vec<SupplyRegistryRecord>],
    hero_records: &'a [Vec<HeroRegistryRecord>],
    territory: &'a don_sim::systems::map_terrain::World,
    frame: i64,
}

impl ArenaSupplyHost<'_> {
    fn owner(&self, who: i32) -> Result<usize, ArenaSupplyHostError> {
        let owner = usize::try_from(who).map_err(|_| ArenaSupplyHostError::InvalidOwner(who))?;
        if owner >= self.players.len() {
            return Err(ArenaSupplyHostError::InvalidOwner(who));
        }
        Ok(owner)
    }

    fn object_index(&self, who: i32, o: i32) -> Result<Option<usize>, ArenaSupplyHostError> {
        self.owner(who)?;
        let Ok(o) = i16::try_from(o) else {
            return Ok(None);
        };
        Ok(self
            .ents
            .iter()
            .position(|ent| i32::from(ent.who) == who && ent.object_o == o))
    }

    fn completed_owned_type(&self, who: i32, type_id: i32) -> bool {
        self.ents.iter().any(|ent| {
            ent.alive && ent.complete && i32::from(ent.who) == who && ent.type_id == type_id
        })
    }

    fn upgrade_count(&self, who: i32, range: std::ops::RangeInclusive<i32>) -> i32 {
        let Ok(owner) = self.owner(who) else {
            return 0;
        };
        range
            .filter(|type_id| self.players[owner].techs.contains(type_id))
            .count() as i32
    }
}

impl ArenaSupplyAttritionHost for ArenaSupplyHost<'_> {
    type Error = ArenaSupplyHostError;

    fn unit_state(
        &self,
        who: i32,
        o: i32,
    ) -> Result<Option<SupplyAttritionUnitState>, Self::Error> {
        let Some(index) = self.object_index(who, o)? else {
            return Ok(None);
        };
        let ent = &self.ents[index];
        if !ent.alive || ent.building {
            return Ok(None);
        }
        let motion = ent
            .motion
            .as_ref()
            .ok_or(ArenaSupplyHostError::MissingMotion { who, o })?;
        let ty = self
            .types
            .get(ent.type_id)
            .ok_or(ArenaSupplyHostError::MissingType(ent.type_id))?;
        Ok(Some(SupplyAttritionUnitState {
            unit: retail_systems::SupplyUnitKey {
                who,
                o,
                x: ent.x,
                y: ent.y,
            },
            unit_id: ent.object_o,
            attrition_period: ent.attrition_period,
            damage: ent.hp.damage,
            unit_masks: motion.unit_masks,
            unit_masks2: motion.unit_masks2,
            is_supply: ty.unit_flags2 & 0x40 != 0,
            // `get_bonus(0x42)` devirtualizes to ObjectTypeData::is(TypeIndex 66, 0).
            militia: type_is(self.types, ent.type_id, 0x42),
            domain: ty.domain,
            type_308: ty.uber_size,
            // Arena objects have no captain/child object links. The isolated object shape
            // is exact for uber_size == 1; damage below fails closed for every other shape.
            curr_uber_size: 1,
        }))
    }

    fn supply_records(&self, who: i32) -> Result<&[SupplyRegistryRecord], Self::Error> {
        let owner = self.owner(who)?;
        Ok(&self.supply_records[owner])
    }

    fn hero_records(&self, who: i32) -> Result<&[HeroRegistryRecord], Self::Error> {
        let owner = self.owner(who)?;
        Ok(&self.hero_records[owner])
    }

    fn support_object(&self, who: i32, o: i32) -> Result<Option<SupplySearchObject>, Self::Error> {
        let Some(index) = self.object_index(who, o)? else {
            return Ok(None);
        };
        let ent = &self.ents[index];
        Ok(Some(SupplySearchObject {
            active: ent.alive,
            is_unit: !ent.building,
            x: ent.x,
            y: ent.y,
        }))
    }

    fn supply_radius_facts(&self, who: i32) -> Result<SupplyRadiusFacts, Self::Error> {
        self.owner(who)?;
        let rules = don_sim::systems::borders_fog::AttritionRules::default();
        Ok(SupplyRadiusFacts {
            supply_radius: rules.supply_radius,
            supply_radius_upgrade: rules.supply_radius_upgrade,
            // `LeaderData::get_supply_upgrade` counts TypeIndexes 0x2FB..0x2FD.
            supply_upgrade: self.upgrade_count(who, 0x2FB..=0x2FD),
            has_terra_cotta: self.completed_owned_type(who, 0x211),
            terra_cotta_range: 0,
        })
    }

    fn owned_type_count(&self, who: i32, type_id: i32) -> Result<i32, Self::Error> {
        self.owner(who)?;
        Ok(self
            .ents
            .iter()
            .filter(|ent| ent.alive && i32::from(ent.who) == who && ent.type_id == type_id)
            .count() as i32)
    }

    fn object_is(
        &self,
        who: i32,
        o: i32,
        type_id: i32,
        relation_set: i32,
    ) -> Result<bool, Self::Error> {
        let Some(index) = self.object_index(who, o)? else {
            return Err(ArenaSupplyHostError::MissingObject { who, o });
        };
        let actual = self.ents[index].type_id;
        // All hero identities consumed by this transaction are concrete live TypeIndexes.
        // The loaded FROM chain supplies relation-set zero ancestry. Relation-set one is
        // used only for the six concrete patriot ids and has no additional Arena aliases.
        Ok(actual == type_id || (relation_set == 0 && type_is(self.types, actual, type_id)))
    }

    fn hero_radius_facts(&self, who: i32) -> Result<HeroRadiusFacts, Self::Error> {
        self.owner(who)?;
        Ok(HeroRadiusFacts {
            // `LeaderData::get_general_upgrade` counts TypeIndexes 0x305..0x307.
            general_upgrade: self.upgrade_count(who, 0x305..=0x307),
            general_radius: 6,
            parmenio_radius_adjust_256: 384,
            wellington_radius_percent: 200,
            kutosov_radius_percent: 300,
            has_terra_cotta: self.completed_owned_type(who, 0x211),
            terra_cotta_range: 0,
            military_patriot_radius_bonus: 3,
            economic_patriot_radius_bonus: 1,
        })
    }

    fn write_unit_masks2(
        &mut self,
        who: i32,
        o: i32,
        before: u32,
        after: u32,
    ) -> Result<(), Self::Error> {
        let index = self
            .object_index(who, o)?
            .ok_or(ArenaSupplyHostError::MissingObject { who, o })?;
        let motion = self.ents[index]
            .motion
            .as_mut()
            .ok_or(ArenaSupplyHostError::MissingMotion { who, o })?;
        if motion.unit_masks2 != before {
            return Err(ArenaSupplyHostError::StaleUnitMasks2 {
                expected: before,
                found: motion.unit_masks2,
            });
        }
        motion.unit_masks2 = after;
        Ok(())
    }

    fn take_attrition_damage(
        &mut self,
        who: i32,
        o: i32,
        damage: don_sim::systems::borders_fog::AttritionDamage,
    ) -> Result<DamageOutcome, Self::Error> {
        let index = self
            .object_index(who, o)?
            .ok_or(ArenaSupplyHostError::MissingObject { who, o })?;
        let type_id = self.ents[index].type_id;
        let uber_size = self
            .types
            .get(type_id)
            .ok_or(ArenaSupplyHostError::MissingType(type_id))?
            .uber_size;
        if uber_size != 1 {
            return Err(ArenaSupplyHostError::UnsupportedUberDamage { type_id, uber_size });
        }
        let ent = &mut self.ents[index];
        ent.hp.accumulate(damage.flat, damage.fractional);
        ent.last_damaged = self.frame;
        Ok(don_sim::systems::combat::resolve_damage(
            &ent.hp,
            ent.hp.myhits,
        ))
    }

    fn suffer_graphic_attrition(&mut self, who: i32, o: i32) -> Result<(), Self::Error> {
        // The callback is output-only in retail. Validate that the surviving live object
        // still exists; headless Arena intentionally owns no graphic-event queue.
        let index = self
            .object_index(who, o)?
            .ok_or(ArenaSupplyHostError::MissingObject { who, o })?;
        if !self.ents[index].alive {
            return Err(ArenaSupplyHostError::MissingObject { who, o });
        }
        Ok(())
    }
}

impl ArenaReloadSupplyHost for ArenaSupplyHost<'_> {
    fn territory_owner_at(&self, x: i32, y: i32) -> Result<i32, Self::Error> {
        let tx = x.div_euclid(RANGE_UNITS_PER_TILE);
        let ty = y.div_euclid(RANGE_UNITS_PER_TILE);
        let wx = tx.div_euclid(4);
        let wy = ty.div_euclid(4);
        if wx < 0 || wy < 0 || wx >= self.territory.xs || wy >= self.territory.ys {
            return Err(ArenaSupplyHostError::PositionOutsideWorld { x, y });
        }
        Ok(self.territory.get_who(wx, wy))
    }
}

impl ArenaSupplyHealingHost for ArenaSupplyHost<'_> {
    fn french_supply_bonus(&self, who: i32) -> Result<bool, Self::Error> {
        let owner = self.owner(who)?;
        Ok(self.players[owner].tribe == 0x0A)
    }

    fn completed_versailles(&self, who: i32) -> Result<bool, Self::Error> {
        self.owner(who)?;
        Ok(self.completed_owned_type(who, 0x218))
    }

    fn unsupported_prior_healing_source(&self, who: i32, o: i32) -> Result<bool, Self::Error> {
        let owner = self.owner(who)?;
        let index = self
            .object_index(who, o)?
            .ok_or(ArenaSupplyHostError::MissingObject { who, o })?;
        let ent = &self.ents[index];
        let ty = self
            .types
            .get(ent.type_id)
            .ok_or(ArenaSupplyHostError::MissingType(ent.type_id))?;
        let has_live_hero = self.hero_records[owner]
            .iter()
            .any(|record| record.hero_flags & SUPPORT_REGISTRY_ACTIVE != 0);
        // The rest of Unit::process_healing can pre-empt or add to this supply arm for
        // heroes, Iroquois, civilians/caravans/merchants, and multi-slot captain objects.
        // Arena does not silently compose the isolated supply transaction with them.
        Ok(has_live_hero
            || self.players[owner].tribe == 0x12
            || ty.cat == 5
            || ent.type_id == 0x13D
            || ty.uber_size != 1)
    }

    fn repair_supply_damage(
        &mut self,
        who: i32,
        o: i32,
        before: i32,
        amount: i32,
    ) -> Result<i32, Self::Error> {
        let index = self
            .object_index(who, o)?
            .ok_or(ArenaSupplyHostError::MissingObject { who, o })?;
        let type_id = self.ents[index].type_id;
        let uber_size = self
            .types
            .get(type_id)
            .ok_or(ArenaSupplyHostError::MissingType(type_id))?
            .uber_size;
        if uber_size != 1 {
            return Err(ArenaSupplyHostError::UnsupportedUberHealing { type_id, uber_size });
        }
        let ent = &mut self.ents[index];
        if ent.hp.damage != before {
            return Err(ArenaSupplyHostError::StaleDamage {
                expected: before,
                found: ent.hp.damage,
            });
        }
        let (damage, damage_frac) = production::repair_damage(ent.hp.damage, amount, ent.hp.myhits);
        ent.hp.damage = damage;
        ent.hp.damage_frac = damage_frac;
        Ok(damage)
    }
}

impl ArenaAttritionRecomputeHost for ArenaSupplyHost<'_> {
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
    ) -> Result<(), Self::Error> {
        let index = self
            .object_index(who, o)?
            .ok_or(ArenaSupplyHostError::MissingObject { who, o })?;
        let ent = &mut self.ents[index];
        let motion = ent
            .motion
            .as_mut()
            .ok_or(ArenaSupplyHostError::MissingMotion { who, o })?;
        if motion.unit_masks != unit_masks_before {
            return Err(ArenaSupplyHostError::StaleUnitMasks {
                expected: unit_masks_before,
                found: motion.unit_masks,
            });
        }
        if motion.unit_masks2 != unit_masks2_before {
            return Err(ArenaSupplyHostError::StaleUnitMasks2 {
                expected: unit_masks2_before,
                found: motion.unit_masks2,
            });
        }
        if ent.attrition_period != period_before {
            return Err(ArenaSupplyHostError::StaleAttritionPeriod {
                expected: period_before,
                found: ent.attrition_period,
            });
        }
        motion.unit_masks = unit_masks_after;
        motion.unit_masks2 = unit_masks2_after;
        ent.attrition_period = period_after;
        Ok(())
    }
}

struct ArenaTargetAdapter<'a> {
    ents: &'a [Ent],
    types: &'a Types,
    players: &'a [PlayerState],
    map: &'a Map,
    territory: &'a don_sim::systems::map_terrain::World,
    balance: &'a BalanceTable,
    combat: &'a CombatConstants,
    frame: i32,
}

impl ArenaTargetAdapter<'_> {
    fn ent(&self, r: ObjRef) -> Option<&Ent> {
        object_ref_index(self.ents, r).map(|i| &self.ents[i])
    }

    fn region(&self, e: &Ent) -> i32 {
        let (tx, ty) = e.tile();
        // `Regions::find_all` 0x0067EFF0 flood-fills WData land/sea classes, not the
        // passability of individual map tiles. The arena is explicitly no-water and
        // `World::init_default_rules` materialises every WData with the same region value
        // (the wipe sentinel 64), including across mountain tile blockers. Region number
        // bands distinguish land/sea elsewhere, but check_target consumes equality only;
        // on Arena's no-water maps this single equivalence class is exact. A passable-tile
        // connected component would incorrectly split retail regions at ridges.
        self.territory.get_tregion(tx, ty)
    }

    fn currently_visible(&self, observer: usize, e: &Ent) -> bool {
        let (tx, ty) = e.tile();
        if tx < 0 || ty < 0 || tx >= self.map.w || ty >= self.map.h {
            return false;
        }
        self.players[observer].visible[(ty * self.map.w + tx) as usize]
    }
}

impl AutoTargetAdapter for ArenaTargetAdapter<'_> {
    fn is_enemy(&self, observer_who: i16, candidate_who: i16) -> bool {
        // Arena match construction has no team/shared-control field: every non-negative
        // distinct participant slot is an opposing FFA leader. Diplomacy expansion remains
        // a literal MODEL 6 blocker rather than being guessed from object ownership here.
        observer_who >= 0
            && candidate_who >= 0
            && (observer_who as usize) < self.players.len()
            && (candidate_who as usize) < self.players.len()
            && observer_who != candidate_who
    }

    fn is_seen(&self, observer_who: i16, candidate: ObjRef) -> bool {
        let Ok(observer) = usize::try_from(observer_who) else {
            return false;
        };
        let Some(e) = self.ent(candidate) else {
            return false;
        };
        // UnitData::is_seen 0x00607A60 and BuildData::is_seen 0x0062E1A0 admit the
        // current fog plane or the object's remembered-visible bit. Arena memory is that
        // per-object bit plus the last sighting payload. After that admission retail's
        // find_nearby_target walks the live WData object and check/compare_target read its
        // current coordinates, damage and action. Deliberately do not substitute the
        // Sighting payload here: the simulation-internal acquisition would then diverge
        // from retail. Fog-safe bot snapshots remain restricted to the payload.
        self.currently_visible(observer, e) || self.players[observer].memory.contains_key(&e.id)
    }

    fn candidate(
        &self,
        searcher: ObjRef,
        candidate: ObjRef,
        searcher_row: TargetRow,
        candidate_row: TargetRow,
    ) -> Option<AutoTargetCandidate> {
        let attacker = self.ent(searcher)?;
        let target_ent = self.ent(candidate)?;
        let at = self.types.get(attacker.type_id)?;
        let dt = self.types.get(target_ent.type_id)?;

        // This arena's supported combat roster is direct land. The full retail predicate
        // has distinct naval and air arms; allowing those through the land reduction would
        // be a permissive default, so they remain behind MODEL 6.
        if attacker.building || at.domain != 0 || dt.domain != 0 {
            return None;
        }

        let (target_masks, target_masks2) = target_ent
            .motion
            .as_ref()
            .map_or((0, 0), |u| (u.unit_masks, u.unit_masks2));
        // UnitData::is_seen 0x00607A60 runs cloak/is_detected before its current-or-memory
        // fog test. Arena does not yet materialise the detector seen3 plane or the exact
        // action-pointer arm for CLOAK_WHILE_IDLE, so admitting any cloak-capable/dynamic
        // cloak object would reveal it permissively. Fail closed until that literal target
        // visibility host exists. CompareTarget's separate unit_masks bit 0 detection read
        // is gated for the same reason.
        if !target_ent.building
            && (dt.unit_flags & (CLOAK_TYPE_FLAG | CLOAK_WHILE_IDLE_TYPE_FLAG) != 0
                || target_masks & (CLOAK_OBJECT_FLAG | 1) != 0
                || target_masks2 & CLOAK_SECONDARY_OBJECT_FLAG != 0)
        {
            return None;
        }

        let target_terrain = self.map.at(target_ent.tile().0, target_ent.tile().1);
        let valid_target_const =
            // UnitData::is_on_map: every live arena Ent is on-map.
            !(at.unit_flags & 0x10_2000 != 0)
                // An anti-air-only land unit does not admit land targets.
                && at.obj_masks & 0x8000_0000 == 0
                // valid_target_const rejects melee acquisition into a tree surface.
                && (at.max_range != 0 || target_terrain != Terrain::Forest);

        let attacker_footprint = ObjectFootprint::Unit {
            block_radius: at.block_radius,
        };
        let target_footprint = if target_ent.building {
            ObjectFootprint::Building {
                x_size: dt.x_size,
                y_size: dt.y_size,
            }
        } else {
            ObjectFootprint::Unit {
                block_radius: dt.block_radius,
            }
        };
        // Unit vtable +0xC0 is PDB `UnitData::is_plane` 0x0046CE40. Its exact type test is
        // domain==2 && !(unit_flags&0x20), so the direct-land gate above proves footprint
        // mode for every candidate admitted here; the helper also preserves the complete
        // plane/objmask branch for future domain expansion.
        let distance_mode =
            AttackDistanceMode::for_unit_type(at.domain, at.unit_flags, at.obj_masks);
        let distance_facts = AutoTargetDistanceFacts {
            attacker: attacker_footprint,
            target: target_footprint,
            mode: distance_mode,
        };
        let dist = attack_distance(AttackDistanceInput {
            attacker_x: searcher_row.x,
            attacker_y: searcher_row.y,
            target_x: candidate_row.x,
            target_y: candidate_row.y,
            attacker: distance_facts.attacker,
            target: distance_facts.target,
            mode: distance_facts.mode,
        });
        let in_range = in_attack_range(dist, at.max_range);
        let same_region = self.region(attacker) == self.region(target_ent);

        // check_target's building tail admits ordinary buildings only when their WData
        // cell is claimed. Types carrying build flag 0x10 are the measured exception.
        let (ttx, tty) = target_ent.tile();
        let territory_claimed = self.territory.get_who(ttx >> 2, tty >> 2) != -1;
        let building_admitted =
            !target_ent.building || dt.build_flags & 0x10 != 0 || territory_claimed;

        let target_guy_flag_0x40 = target_ent
            .guys
            .guys
            .first()
            .and_then(Option::as_ref)
            .is_some_and(|g| g.guy_flags & 0x40 != 0);
        let bearing = don_sim::trig::find_angle(
            target_ent.x.wrapping_sub(attacker.x),
            target_ent.y.wrapping_sub(attacker.y),
        );
        let rear_delta = target_ent
            .facing
            .wrapping_sub(bearing)
            .wrapping_add(i32::MIN) as u32;
        let poor = poor_target(
            &PoorTargetInput {
                both_are_units: !target_ent.building,
                target_guy_flag_0x40,
                attacker_has_objmask_high: at.obj_masks & 0x8000_0000 != 0,
                target_speed_here: dt.moves,
                attacker_speed_here: at.moves,
                flank_level_from_behind: rear_delta,
                attack_dist: dist,
                attacker_role_0x400: at.role & 0x400 != 0,
                max_range_tiles: at.max_range,
            },
            don_sim::mechanics::flank_level,
        );
        let check_target = (same_region || in_range) && building_admitted && !poor;

        let (unit_masks, unit_masks2) = attacker
            .motion
            .as_ref()
            .map_or((0, 0), |u| (u.unit_masks, u.unit_masks2));
        let check_path = attacker.stance == 2
            || (unit_masks & 0x0200_0000 != 0 && unit_masks2 & 0x2_0000 == 0)
            || (at.unit_flags2 & 4 != 0 && unit_masks & 0x8_0000 == 0);

        // Missile Silo's active-trainer arm reads two BuildData captain fields the arena
        // does not materialise. Stop at that object instead of mapping queue state to them.
        if type_is(self.types, target_ent.type_id, 0x208) {
            return None;
        }

        let t_root = root_type(self.types, target_ent.type_id)?;
        let t_is_spellcaster = if target_ent.building {
            type_chain_has_build_flag(self.types, target_ent.type_id, 0x2000_0000)
        } else {
            dt.unit_flags2 & 2 != 0
        };
        let t_is_moving = !target_ent.building && matches!(target_ent.job, Job::MoveTo { .. });
        let estimated_damage = estimated_target_damage(
            self.balance,
            self.combat,
            self.frame,
            attacker,
            target_ent,
            at,
            dt,
        )?;
        let compare = CompareTargetInput {
            check_path,
            // `find_auto_target` overwrites mode from AutoTargetQuery; this initializer
            // is not observed by compare_target.
            mode: false,
            a_is_unit: true,
            a_ref: searcher,
            // The host rejects building attackers and non-land domains above. These are
            // exact impossible states for the admitted ordinary direct-land Unit path.
            a_is_building: false,
            a_stance_is_3: attacker.stance == 3,
            a_unit_mask_0x40000: unit_masks & 0x4_0000 != 0,
            a_domain_is_sea: false,
            a_domain_is_air: false,
            a_has_objmask_0x40000: at.obj_masks & 0x4_0000 != 0,
            a_has_objmask_high: at.obj_masks & 0x8000_0000 != 0,
            a_type_mask_0x40000: at.obj_masks & 0x4_0000 != 0,
            // Only the retail sea-special arm calls Type+0x108; that arm is excluded by
            // the direct-land domain gate above.
            a_type_vf_0x108: false,
            a_leader_flag_4: self.players[attacker.who as usize].leader_ai,
            // Unit::find_new_target is entered from the ordinary idle think arm: it has
            // no current action target and is not firing garrison arrows (a Build path).
            a_current_target: None,
            a_garrison_arrows: 0,
            t_ref: candidate,
            t_alive: target_ent.alive,
            t_is_building: target_ent.building,
            t_is_spellcaster,
            // Arena has no CastSpell activity; absence is an exact state, not a false
            // default for an activity it stores elsewhere.
            t_action_is_cast_spell: false,
            t_action_target: None,
            t_is_tech_0x3a: type_is(self.types, target_ent.type_id, 0x3A),
            t_is_wonder: target_ent.building && (0x20E..0x21F).contains(&dt.id),
            t_has_objmask_high: target_ent.building && dt.obj_masks & 0x8000_0000 != 0,
            t_type_value: dt.cost.iter().fold(0i32, |sum, &v| sum.wrapping_add(v)),
            t_is_city_centre: candidate_row.is_city_centre(),
            // Arena Ents materialise only the Unit and Build classes; the separate Wall
            // class cannot reach this adapter.
            t_is_defensive_wall: false,
            t_is_military_trainer: target_ent.building && t_root.build_flags & 0x4000_0000 != 0,
            t_is_training_building: target_ent.building && t_root.build_flags & 0x8000_0000 != 0,
            // This field is consulted only for Missile Silo (`is 0x208`), which is
            // hard-gated above because its two captain fields are not materialised.
            t_trainer_is_active: false,
            t_type_has_hits: dt.hits != 0,
            t_hits_left: target_ent.hits_left(),
            t_attack: dt.attack,
            t_is_damaged: target_ent.hp.damage != 0,
            t_is_moving,
            t_is_supply: !target_ent.building && dt.unit_flags2 & 0x40 != 0,
            // Non-land candidates are hard-gated above, so air is impossible here.
            t_domain_is_air: false,
            // unit_masks bit 0 is hard-gated with the missing detector plane above.
            t_stealth_undetected: false,
            t_type_mask_0x10000: dt.role & 0x1_0000 != 0,
            t_type_mask_0x200000: dt.unit_flags & 0x20_0000 != 0,
            t_type_mask_0x10: dt.unit_flags & 0x10 != 0,
            t_is_tech_0x150: type_is(self.types, target_ent.type_id, 0x150),
            t_is_tech_0x13d: type_is(self.types, target_ent.type_id, 0x13D),
            t_empty_shell: target_ent.alive
                && candidate_row.is_city_centre()
                && target_ent.hits_left() == 0,
            // Arena's admitted direct-land Unit/Build objects have no containment or
            // garrison state, so TargetRow::full is exactly zero, not a default for a
            // hidden store.
            t_full: candidate_row.full as i32,
            estimated_damage,
            in_range,
        };

        Some(AutoTargetCandidate {
            valid_target_const,
            check_target,
            check_path,
            distance_facts,
            compare,
        })
    }
}

fn estimated_target_damage(
    balance: &BalanceTable,
    combat: &CombatConstants,
    frame: i32,
    a: &Ent,
    b: &Ent,
    at: &TypeRow,
    dt: &TypeRow,
) -> Option<i32> {
    let input = DamageInput {
        // Every admitted live type pair must have a real balance-table entry. A fabricated
        // neutral 100% fallback would silently change compare_target ordering.
        balance_pct: balance.get(at.id, dt.id)?,
        attack: get_attack(at.attack, false, 0, 0),
        armor: get_armor(dt.armor, false, 0, 0),
        attacker_masks: at.obj_masks,
        defender_masks: dt.obj_masks,
        // compare_target calls get_damage with find_angle(0, 0) and zero mode flags.
        attack_dir: 0,
        splash_flag: 0,
        overkill_gate: 0,
        attacker_player: a.who as u32,
        attacker_type_id: at.id,
        attacker_domain: at.domain,
        attacker_splash_percent: at.splash_percent,
        attacker_type_0x40: 0,
        attacker_z: 0,
        attacker_flag8_bit5: false,
        defender_type_id: dt.id,
        defender_domain: dt.domain,
        defender_type_0x2b8_bit2: false,
        defender_splash_divisor: 1,
        defender_flags_0x68: 0,
        defender_flags_0x6c_bit12: false,
        defender_z: 0,
        defender_facing: b.facing,
        defender_facing_entrench: b.facing,
        defender_overkill_stamp: 0,
        defender_word_0xa4: 0,
        attacker_vf_0xe4: 0,
        current_frame: frame,
        game_flag_0x821_bit1: false,
        tile_rocky: false,
        tile_owner: -1,
    };
    let predicates = DamagePredicates {
        attacker_vf_0x18: !a.building,
        defender_vf_0x18: !b.building,
        attacker_vf_0x20: true,
        ..DamagePredicates::default()
    };
    let rules = target::combat_rules(combat);
    let terms = target::unreached_terms(combat, 0);
    Some(damage_traced(&input, &predicates, &rules, &terms).0)
}

impl World {
    /// Build an arena. `tribes` names one nation index per player.
    pub fn new(
        types: Types,
        map: Map,
        balance: BalanceTable,
        tribes: &[u8],
        params: ArenaParams,
    ) -> Result<World, String> {
        let ids = Ids::resolve(&types)?;
        if tribes.len() > map.starts.len() {
            return Err(format!(
                "{} players but the map has {} starts",
                tribes.len(),
                map.starts.len()
            ));
        }
        if tribes.len() > BANDED_SLOTS {
            return Err(format!(
                "{} players but retail has only {BANDED_SLOTS} leader building bands",
                tribes.len()
            ));
        }
        let n = (map.w * map.h) as usize;
        let sim_seed = map.seed as i32;
        let econ = EconomyRules {
            commerce_cap: types.constants.commerce_cap,
            ..EconomyRules::shipped()
        };
        let age_techs = types.age_techs();
        let spatial = map.spatial;
        let target_world = TargetWorld::new(((map.w + 3) / 4).max(1), ((map.h + 3) / 4).max(1));
        let collision_world = don_sim::systems::map_terrain::World::init_default_rules(
            ((map.w + 3) / 4).max(1),
            ((map.h + 3) / 4).max(1),
        );
        let mut w = World {
            roster: tribes.iter().map(|&t| types.for_tribe(t)).collect(),
            players: tribes
                .iter()
                .enumerate()
                .map(|(i, &t)| PlayerState {
                    who: i as u8,
                    tribe: t,
                    name: format!("P{}", i + 1),
                    stock: types.constants.starting_goods,
                    acc: [0; NRES],
                    gathered: [0; NRES],
                    techs: BTreeSet::new(),
                    alive: true,
                    defeat_frame: None,
                    orders_ok: 0,
                    orders_refused: 0,
                    orders_invalid: 0,
                    explored: vec![false; n],
                    visible: vec![false; n],
                    memory: BTreeMap::new(),
                    damage_dealt: 0,
                    damage_taken: 0,
                    kills: 0,
                    losses: 0,
                    leader_ai: true,
                    // `PlayerOptions::init` 0x006F1CD0 clears +4/+0x0C and leaves option
                    // bits 1 and 3 set. These are the bytes Unit::init 0x00612100 reads.
                    stance_type_1: 0,
                    stance_type_0: 0,
                    leader_option_flags: 0x0A,
                })
                .collect(),
            types,
            ids,
            map,
            spatial,
            ents: Vec::new(),
            frame: 0,
            balance,
            combat: CombatConstants::shipped(),
            econ,
            prod_rules: ProdRules::shipped(),
            params,
            log: Vec::new(),
            logging: true,
            flank_hist: [0; 3],
            shots: 0,
            last_object_process_order: Vec::new(),
            rejects: BTreeMap::new(),
            pathfinder: PathFinder::new(),
            game_random: Random::new(sim_seed),
            movement_coverage: DispatchCoverage::default(),
            collision_world,
            collision_check: CollCheck::new(),
            collision_units: CollisionUnits::default(),
            gather_runtime: ArenaGatherRuntime::default(),
            target_world,
            target_circle: circle_table(),
            supply_records: vec![Vec::new(); tribes.len()],
            hero_records: vec![Vec::new(); tribes.len()],
            age_techs,
            next_object_uid: vec![0; tribes.len()],
        };
        for i in 0..w.players.len() {
            w.seed_start(i);
        }
        w.update_fog();
        Ok(w)
    }

    // -----------------------------------------------------------------------
    // Setup
    // -----------------------------------------------------------------------

    /// The shipped Small Town start: a capital, the template's three Farms and a Library,
    /// and three Citizens. `citytemplates.xml` has all 17 templates at three Farms plus a
    /// Library, which `docs/tracks/ron-ai-impl.md` measured; the citizen count is not in
    /// the shipped data and is a model number, here matching the three Farms.
    fn seed_start(&mut self, pi: usize) {
        let (sx, sy) = self.map.starts[pi];
        let city = self.spawn(pi as u8, self.ids.small_city, sx, sy, true);
        // Farms and a Library on the nearest legal tiles, spiralling out from the centre.
        let mut want = vec![
            self.ids.farm,
            self.ids.farm,
            self.ids.farm,
            self.ids.library,
        ];
        let mut r = 3;
        while !want.is_empty() && r < 12 {
            for (dx, dy) in ring(r) {
                if want.is_empty() {
                    break;
                }
                let ty = *want.last().unwrap();
                if self.placement_ok(pi, ty, sx + dx, sy + dy).is_ok() {
                    self.spawn(pi as u8, ty, sx + dx, sy + dy, true);
                    want.pop();
                }
            }
            r += 1;
        }
        for k in 0..3 {
            let (dx, dy) = ring(2)[k * 3 % 16];
            self.spawn(pi as u8, self.ids.citizen, sx + dx, sy + dy, true);
        }
        let _ = city;
    }

    fn spawn(&mut self, who: u8, type_id: i32, tx: i32, ty: i32, complete: bool) -> EntId {
        let t = match self.types.get(type_id) {
            Some(t) => t.clone(),
            None => return EntId::NONE,
        };
        assert!(
            t.kind_unit ^ t.kind_building,
            "Arena can only install retail Unit or Build object bands"
        );
        let owner = usize::from(who);
        assert!(
            owner < self.players.len(),
            "object owner has no Arena leader"
        );
        let band_len = self
            .ents
            .iter()
            .filter(|e| e.who == who && e.building == t.kind_building)
            .count();
        let (band_base, band_limit) = if t.kind_building {
            (BUILD_BAND_BASE as usize, WALL_BAND_BASE as usize)
        } else {
            (0, BUILD_BAND_BASE as usize)
        };
        let object_index = band_base
            .checked_add(band_len)
            .expect("Arena owner-local object index overflow");
        assert!(
            object_index < band_limit,
            "Arena's non-recycling owner-local object band is full"
        );
        let object_o =
            i16::try_from(object_index).expect("retail unit/build object bands fit ObjectData::o");
        let id = EntId::from_index(self.ents.len());
        let object_uid = self.next_object_uid[owner];
        self.next_object_uid[owner] = object_uid.wrapping_add(1);
        let (worker_cap, gather_res) = self.gather_capacity(&t, tx, ty);
        let city = if t.kind_building {
            self.nearest_own_city(who, tx, ty).unwrap_or(EntId::NONE)
        } else {
            EntId::NONE
        };
        let x = tx * RANGE_UNITS_PER_TILE + HALF;
        let y = ty * RANGE_UNITS_PER_TILE + HALF;
        let stance = if t.kind_unit {
            initial_unit_stance(&self.players[who as usize], &t)
        } else {
            0
        };
        let build = if t.kind_building {
            let constr_time = production::update_construct_time(
                t.job_time.max(0) as u32,
                &ConstructTimeGates::default(),
                &self.prod_rules,
            );
            let is_wonder = (0x20E..0x21F).contains(&type_id);
            let city_o = self.ent(city).map_or(-1, |city| city.object_o);
            let mut b = BuildData {
                flags: production::flag::VALID
                    | if complete {
                        production::flag::STARTED | production::flag::ACTIVE
                    } else {
                        0
                    },
                who,
                myhits: t.hits,
                uid: object_uid,
                constr_time,
                construct_hits: production::construct_hits(
                    t.hits,
                    complete,
                    is_wonder,
                    0,
                    production::construct_time(
                        constr_time,
                        false,
                        &Default::default(),
                        &self.prod_rules,
                    ),
                ),
                orig_type: type_id,
                city: city_o,
                ..BuildData::default()
            };
            if complete {
                // `Build::activate` 0x00623E20 sets this measured active-building mask.
                b.build_masks |= 0x1000;
            }
            Some(b)
        } else {
            None
        };
        // `Unit::init` writes this binary angle before its first `set_new_location` call.
        const INITIAL_UNIT_ANGLE: i32 = 0x5555_5555;
        let type_stats = unit_type_stats(&t);
        let mut motion = if t.kind_unit {
            let mut u = UnitWork::at(who, object_o, x, y);
            u.uid = object_uid;
            u.body.angle = INITIAL_UNIT_ANGLE;
            u.ptype = type_id;
            u.myspeed = t.moves.clamp(0, i16::MAX as i32) as i16;
            u.path_unit = PathUnit {
                type_size: t.new_block_radius.max(1),
                can_board_transport: false,
                small_footprint: t.domain < 2,
                can_transport: false,
            };
            u.guy_env = GuyEnv {
                ut: type_stats,
                unit_speed: t.moves,
                order_speed_bonus: false,
                unit_mask_turn_scale2: false,
                turn_scale: 256,
                turn_scale2: 2,
                ai_speed: 1,
            };
            u.type_moves_while_turning = t.unit_flags & 0x20 != 0;
            u.type_snap_arm = t.unit_flags2 & 4 != 0;
            u.type_ignores_recharge = t.unit_flags & 0x400 != 0;
            u.type_wants_work = t.unit_flags & 0x4_0000 != 0;
            u.type_blocks_work = t.unit_flags & 0x4000 != 0;
            u.lead_guy.ty = type_id;
            u.lead_guy.x = x;
            u.lead_guy.y = y;
            u.lead_guy.des_x = x;
            u.lead_guy.des_y = y;
            Some(u)
        } else {
            None
        };
        let mut guys = if t.kind_unit {
            UnitGuys::spawn_full(type_id, who as i8, object_o, &type_stats)
        } else {
            UnitGuys::default()
        };
        if t.kind_unit {
            guys.set_initial_locations(
                x,
                y,
                INITIAL_UNIT_ANGLE,
                0,
                0,
                self.map.w * RANGE_UNITS_PER_TILE,
                self.map.h * RANGE_UNITS_PER_TILE,
                &type_stats,
            );
            if let (Some(u), Some(g)) =
                (motion.as_mut(), guys.guys.first().and_then(Option::as_ref))
            {
                u.lead_guy = *g;
            }
        }
        self.ents.push(Ent {
            id,
            who,
            object_o,
            type_id,
            x,
            y,
            hp: HitPoints {
                myhits: t.hits,
                damage: 0,
                damage_frac: 0,
            },
            alive: true,
            building: t.kind_building,
            complete,
            build_left: if complete { 0 } else { t.job_time },
            object_uid,
            build,
            build_order: None,
            construction_refusal: None,
            job: Job::Idle,
            cycle: AttackCycle::default(),
            facing: if t.kind_unit { INITIAL_UNIT_ANGLE } else { 0 },
            stance,
            city,
            workers: 0,
            worker_cap,
            gather_res,
            queue: Vec::new(),
            assigned_to: EntId::NONE,
            last_damaged: -1,
            spawn_frame: self.frame,
            attrition_period: 0,
            motion: motion.take(),
            guys,
        });
        if t.kind_unit && t.unit_flags2 & 0x40 != 0 {
            self.init_supply_record(who, object_o);
        }
        if t.kind_unit && t.unit_flags2 & 0x20 != 0 {
            self.init_hero_record(who, object_o);
        }
        if t.kind_unit {
            let ent = self.ents.last().expect("unit was just pushed");
            let row = collision_row(ent, &t);
            let live_guys: Vec<CollGuy> = ent
                .guys
                .guys
                .iter()
                .take(t.squad_size.max(0) as usize)
                .flatten()
                .map(|g| CollGuy {
                    x: g.x,
                    y: g.y,
                    block_radius: t.new_block_radius,
                })
                .collect();
            collision::place(
                &mut self.collision_world,
                &mut self.collision_units,
                row,
                live_guys,
            );
        }
        let ent = self.ents.last().expect("object was just pushed");
        assert!(
            self.target_world
                .place_at(target_ref(ent), target_row(ent, &t, &self.ids)),
            "every arena object must occupy its stable retail target slot"
        );
        let key = gather_key(self.ents.last().expect("object was just pushed"));
        if type_id == self.ids.citizen {
            self.gather_runtime
                .register_worker(key, type_id)
                .expect("Arena entity slots never recycle within a World");
        }
        if let Some(kind) = ordinary_gather_kind(&self.ids, type_id) {
            let capacity = if kind == OrdinaryGatherKind::Farm {
                GatherCapacityAuthority::FlatFarmOne
            } else {
                GatherCapacityAuthority::MissingRetailTerrainSource
            };
            self.gather_runtime
                .register_site(key, object_uid, kind, capacity)
                .expect("Arena entity slots never recycle within a World");
        }
        id
    }

    /// `Supplies::init_supply` (`0x0073AD40`): reuse the first inactive six-byte
    /// record, otherwise append one, and retain the registry slot as `supply`.
    fn init_supply_record(&mut self, who: u8, o: i16) {
        let records = &mut self.supply_records[usize::from(who)];
        let slot = records
            .iter()
            .position(|record| record.supply_flags & SUPPORT_REGISTRY_ACTIVE == 0)
            .unwrap_or(records.len());
        let record = SupplyRegistryRecord {
            supply: i16::try_from(slot).expect("retail supply registry index fits i16"),
            o,
            supply_flags: SUPPORT_REGISTRY_ACTIVE,
            who: who as i8,
        };
        if slot == records.len() {
            records.push(record);
        } else {
            records[slot] = record;
        }
    }

    /// `Heroes::init_hero` (`0x0073A330`), with the same inactive-hole reuse rule as
    /// the retail walked array.
    fn init_hero_record(&mut self, who: u8, o: i16) {
        let records = &mut self.hero_records[usize::from(who)];
        let slot = records
            .iter()
            .position(|record| record.hero_flags & SUPPORT_REGISTRY_ACTIVE == 0)
            .unwrap_or(records.len());
        let record = HeroRegistryRecord {
            hero: i16::try_from(slot).expect("retail hero registry index fits i16"),
            o,
            hero_flags: SUPPORT_REGISTRY_ACTIVE,
            who: who as i8,
        };
        if slot == records.len() {
            records.push(record);
        } else {
            records[slot] = record;
        }
    }

    /// `Supplies::close_supply` (`0x0073ACC0`) and `Heroes::close_hero`
    /// (`0x0073A480`): clear the active identity and trim only inactive tail slots.
    fn close_support_records(&mut self, who: u8, o: i16) {
        let supplies = &mut self.supply_records[usize::from(who)];
        if let Some(record) = supplies
            .iter_mut()
            .find(|record| record.supply_flags & SUPPORT_REGISTRY_ACTIVE != 0 && record.o == o)
        {
            record.supply_flags &= !SUPPORT_REGISTRY_ACTIVE;
            record.who = -1;
            record.o = -1;
        }
        while supplies
            .last()
            .is_some_and(|record| record.supply_flags & SUPPORT_REGISTRY_ACTIVE == 0)
        {
            supplies.pop();
        }

        let heroes = &mut self.hero_records[usize::from(who)];
        if let Some(record) = heroes
            .iter_mut()
            .find(|record| record.hero_flags & SUPPORT_REGISTRY_ACTIVE != 0 && record.o == o)
        {
            record.hero_flags &= !SUPPORT_REGISTRY_ACTIVE;
            record.who = -1;
            record.o = -1;
        }
        while heroes
            .last()
            .is_some_and(|record| record.hero_flags & SUPPORT_REGISTRY_ACTIVE == 0)
        {
            heroes.pop();
        }
    }

    /// MODEL 3 — worker slots and yield come from the terrain the building stands on.
    ///
    /// Retail's `BuildData::gather_max` (`+0x80`) is terrain-derived and its
    /// `gather_from` is a `MiningList`; neither is derived here. What *is* shipped is the
    /// build flag `g` ("resource gatherer", bit `0x40`) and the two caps the shipped
    /// script's own arithmetic implies: a Farm holds exactly one worker
    /// (`train_unit_with_need` treats a Farm as needing one only at zero) and a
    /// Woodcutter's Camp targets five (`place_woodcutter`'s `min_size`). The Mine's four
    /// is ours.
    fn gather_capacity(&self, t: &TypeRow, tx: i32, ty: i32) -> (i32, usize) {
        if !t.is_gatherer() {
            return (0, 0);
        }
        if t.id == self.ids.farm {
            (1, 0)
        } else if t.id == self.ids.camp {
            (self.map.count_within(tx, ty, 2, Terrain::Forest).min(5), 1)
        } else if t.id == self.ids.mine {
            (
                self.map.count_within(tx, ty, 2, Terrain::Mountain).min(4),
                4,
            )
        } else {
            (0, 0)
        }
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    pub fn ent(&self, id: EntId) -> Option<&Ent> {
        id.index()
            .and_then(|i| self.ents.get(i))
            .filter(|e| e.alive)
    }
    fn ent_mut(&mut self, id: EntId) -> Option<&mut Ent> {
        id.index()
            .and_then(|i| self.ents.get_mut(i))
            .filter(|e| e.alive)
    }
    pub fn ty(&self, id: EntId) -> Option<&TypeRow> {
        self.ent(id).and_then(|e| self.types.get(e.type_id))
    }

    pub fn age_of(&self, pi: usize) -> usize {
        self.age_techs
            .iter()
            .filter(|t| self.players[pi].techs.contains(t))
            .count()
            .min(7)
    }

    pub fn pop(&self, pi: usize) -> i32 {
        let mut n = 0;
        for e in self.ents.iter().filter(|e| e.alive && e.who as usize == pi) {
            if !e.building {
                n += self.types.get(e.type_id).map(|t| t.pop).unwrap_or(1);
            }
            for q in &e.queue {
                n += self.types.get(q.type_id).map(|t| t.pop).unwrap_or(0);
            }
        }
        n
    }

    pub fn pop_cap(&self, pi: usize) -> i32 {
        self.types.constants.pop_cap[self.age_of(pi).min(7)]
    }

    pub fn own_ents(&self, pi: usize) -> impl Iterator<Item = &Ent> {
        self.ents
            .iter()
            .filter(move |e| e.alive && e.who as usize == pi)
    }

    pub fn count_type(&self, pi: usize, type_id: i32, include_incomplete: bool) -> i32 {
        let built = self
            .own_ents(pi)
            .filter(|e| e.type_id == type_id && (include_incomplete || e.complete))
            .count() as i32;
        let queued = if include_incomplete {
            self.own_ents(pi)
                .flat_map(|e| e.queue.iter())
                .filter(|q| q.type_id == type_id)
                .count() as i32
        } else {
            0
        };
        built + queued
    }

    fn nearest_own_city(&self, who: u8, tx: i32, ty: i32) -> Option<EntId> {
        self.ents
            .iter()
            .filter(|e| e.alive && e.who == who && e.type_id == self.ids.small_city)
            .min_by_key(|e| Map::tile_dist(e.tile(), (tx, ty)))
            .map(|e| e.id)
    }

    pub fn is_hostile(&self, a: u8, b: u8) -> bool {
        a != b
    }

    fn tile_index(&self, tx: i32, ty: i32) -> Option<usize> {
        if tx < 0 || ty < 0 || tx >= self.map.w || ty >= self.map.h {
            None
        } else {
            Some((ty * self.map.w + tx) as usize)
        }
    }

    pub fn sees(&self, pi: usize, tx: i32, ty: i32) -> bool {
        self.tile_index(tx, ty)
            .map(|i| self.players[pi].visible[i])
            .unwrap_or(false)
    }

    pub fn explored(&self, pi: usize, tx: i32, ty: i32) -> bool {
        self.tile_index(tx, ty)
            .map(|i| self.players[pi].explored[i])
            .unwrap_or(false)
    }

    // -----------------------------------------------------------------------
    // Placement
    // -----------------------------------------------------------------------

    /// The spatial half of legality. Cost and prerequisites are checked by `submit`.
    pub fn placement_ok(&self, pi: usize, type_id: i32, tx: i32, ty: i32) -> Result<(), PlaceErr> {
        let Some(t) = self.types.get(type_id) else {
            return Err(PlaceErr::Terrain);
        };
        let (hx, hy) = ((t.x_size / 2).max(0), (t.y_size / 2).max(0));
        for dy in -hy..=hy {
            for dx in -hx..=hx {
                let (x, y) = (tx + dx, ty + dy);
                if x < 0 || y < 0 || x >= self.map.w || y >= self.map.h {
                    return Err(PlaceErr::OffMap);
                }
                if !self.map.at(x, y).buildable() {
                    return Err(PlaceErr::Terrain);
                }
            }
        }
        // No overlap with an existing building's footprint.
        for e in self.ents.iter().filter(|e| e.alive && e.building) {
            let Some(et) = self.types.get(e.type_id) else {
                continue;
            };
            let (ex, ey) = e.tile();
            let sep = ((t.x_size + et.x_size) / 2).max((t.y_size + et.y_size) / 2);
            if Map::tile_dist((ex, ey), (tx, ty)) < sep {
                return Err(PlaceErr::Occupied);
            }
        }
        if type_id == self.ids.small_city {
            // A new city must clear every existing city, including the enemy's.
            for e in self.ents.iter().filter(|e| e.alive) {
                if e.type_id == self.ids.small_city
                    && Map::tile_dist(e.tile(), (tx, ty)) < self.params.min_city_sep
                {
                    return Err(PlaceErr::TooCloseToCity);
                }
            }
            return Ok(());
        }
        // Everything else belongs to a city, within CITY_CENTER_RADIUS.
        let near_city = self.own_ents(pi).any(|e| {
            e.type_id == self.ids.small_city
                && e.complete
                && Map::tile_dist(e.tile(), (tx, ty)) <= self.spatial.city_center_radius
        });
        if !near_city {
            return Err(PlaceErr::NoCityInRange);
        }
        // MODEL: `WOODCUTTER_RADIUS` / `MINE_RADIUS` read as "how far the building may be
        // from the resource it works". Both are shipped values; the *reading* is ours.
        if type_id == self.ids.camp
            && self
                .map
                .count_within(tx, ty, self.spatial.woodcutter_radius, Terrain::Forest)
                == 0
        {
            return Err(PlaceErr::NoResource);
        }
        if type_id == self.ids.mine
            && self
                .map
                .count_within(tx, ty, self.spatial.mine_radius, Terrain::Mountain)
                == 0
        {
            return Err(PlaceErr::NoResource);
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Orders
    // -----------------------------------------------------------------------

    /// The one mutation seam. Nothing else changes the world.
    pub fn submit(&mut self, who: u8, c: Cmd) -> OrderResult {
        let verb = c.verb_name();
        let r = self.submit_inner(who, c);
        let p = &mut self.players[who as usize];
        let tag = match r {
            OrderResult::Ok(_) => {
                p.orders_ok += 1;
                return r;
            }
            OrderResult::Refused => "refused",
            OrderResult::Invalid => "invalid",
        };
        match r {
            OrderResult::Refused => p.orders_refused += 1,
            _ => p.orders_invalid += 1,
        }
        *self.rejects.entry((who, verb, tag)).or_insert(0) += 1;
        r
    }

    fn owns(&self, who: u8, id: EntId) -> bool {
        self.ent(id).map(|e| e.who == who).unwrap_or(false)
    }

    fn can_pay(&self, pi: usize, cost: &[i32; NRES]) -> bool {
        (0..NRES).all(|r| self.players[pi].stock[r] >= cost[r])
    }
    fn pay(&mut self, pi: usize, cost: &[i32; NRES]) {
        for r in 0..NRES {
            self.players[pi].stock[r] -= cost[r];
        }
    }
    fn preqs_met(&self, pi: usize, t: &TypeRow) -> bool {
        t.preq.iter().all(|p| {
            // A prerequisite is a tech the player must hold. (Unit `FROM` chains are not
            // prerequisites; they are upgrade lineage.)
            self.players[pi].techs.contains(p)
        })
    }

    fn submit_inner(&mut self, who: u8, c: Cmd) -> OrderResult {
        let pi = who as usize;
        if pi >= self.players.len() || !self.players[pi].alive {
            return OrderResult::Invalid;
        }
        if !self.owns(who, c.actor()) {
            return OrderResult::Invalid;
        }
        match c {
            Cmd::Queue {
                producer,
                type_id,
                count,
            } => self.queue_up(pi, producer, type_id, count.max(1)),
            Cmd::Build {
                worker,
                type_id,
                tx,
                ty,
            } => self.build(pi, worker, type_id, tx, ty),
            Cmd::Move { unit, tx, ty } => {
                let Some(t) = self.ty(unit).cloned() else {
                    return OrderResult::Invalid;
                };
                if t.moves <= 0 || t.kind_building {
                    return OrderResult::Invalid;
                }
                self.detach(unit);
                let e = self.ent_mut(unit).unwrap();
                e.job = Job::MoveTo {
                    x: tx * RANGE_UNITS_PER_TILE + HALF,
                    y: ty * RANGE_UNITS_PER_TILE + HALF,
                };
                OrderResult::Ok(1)
            }
            Cmd::Attack { unit, target } => {
                let Some(a) = self.ty(unit).cloned() else {
                    return OrderResult::Invalid;
                };
                let Some(t) = self.ent(target) else {
                    return OrderResult::Invalid;
                };
                if a.attack <= 0 || !self.is_hostile(who, t.who) {
                    return OrderResult::Invalid;
                }
                self.detach(unit);
                self.ent_mut(unit).unwrap().job = Job::Attack { target };
                OrderResult::Ok(1)
            }
            Cmd::Gather { unit, target } => {
                let Some(b) = self.ent(target) else {
                    return OrderResult::Invalid;
                };
                let site_key = gather_key(b);
                let (target_owner, target_type, building, complete, workers, worker_cap) = (
                    b.who,
                    b.type_id,
                    b.building,
                    b.complete,
                    b.workers,
                    b.worker_cap,
                );
                if target_owner != who || !building || !complete || worker_cap <= 0 {
                    return OrderResult::Invalid;
                }
                if workers >= worker_cap {
                    return OrderResult::Refused;
                }
                if self.ty(unit).map(|t| t.id) != Some(self.ids.citizen) {
                    return OrderResult::Invalid;
                }
                let Some(kind) = ordinary_gather_kind(&self.ids, target_type) else {
                    return OrderResult::Invalid;
                };
                let Some(worker) = self.ent(unit) else {
                    return OrderResult::Invalid;
                };
                let worker_key = gather_key(worker);
                let Ok(refusal) = self
                    .gather_runtime
                    .generated_map_preflight(worker_key, site_key, kind)
                else {
                    return OrderResult::Invalid;
                };
                debug_assert!(matches!(
                    (kind, refusal),
                    (
                        OrdinaryGatherKind::Farm,
                        GatherPrerequisiteRefusal::FarmUpdateAndGameGlobal
                    ) | (
                        OrdinaryGatherKind::Camp,
                        GatherPrerequisiteRefusal::CampTerrainGeometryAndOrderedCollision
                    ) | (
                        OrdinaryGatherKind::Mine,
                        GatherPrerequisiteRefusal::MineObjectTerrainGeometryAndOrderedCollision
                    )
                ));
                // The generated map is explicitly not a fidelity source.  This command
                // therefore remains on the labelled MODEL 3 gameplay path; the exact
                // runtime above refused before linking or consuming RNG.
                self.detach(unit);
                self.ent_mut(unit).unwrap().job = Job::Gather { target };
                OrderResult::Ok(1)
            }
            Cmd::Work { unit, target } => {
                let Some(b) = self.ent(target) else {
                    return OrderResult::Invalid;
                };
                let incomplete = !b.complete;
                if b.who != who || !b.building {
                    return OrderResult::Invalid;
                }
                if self.ty(unit).map(|t| t.id) != Some(self.ids.citizen) {
                    return OrderResult::Invalid;
                }
                if incomplete {
                    if !self.assign_build_order(unit, target) {
                        return OrderResult::Invalid;
                    }
                } else {
                    self.detach(unit);
                    self.ent_mut(unit).unwrap().job = Job::Work { target };
                }
                OrderResult::Ok(1)
            }
            Cmd::Halt { unit } => {
                self.detach(unit);
                self.ent_mut(unit).unwrap().job = Job::Idle;
                OrderResult::Ok(1)
            }
        }
    }

    /// Release a citizen from whatever slot it holds. Called before every re-order so a
    /// worker cannot be counted twice.
    fn detach(&mut self, unit: EntId) {
        let Some(e) = self.ent(unit) else { return };
        let worker_owner = e.who;
        let seat = e.assigned_to;
        let was_gathering = matches!(e.job, Job::Gather { .. });
        self.retire_exact_gather(worker_owner, unit);
        if let Some(b) = self.ent_mut(seat) {
            if was_gathering {
                b.workers = (b.workers - 1).max(0);
            }
        }
        if let Some(e) = self.ent_mut(unit) {
            e.assigned_to = EntId::NONE;
            e.build_order = None;
            if let Some(u) = &mut e.motion {
                u.orders.clear();
                order_dispatch::clear_partial_path(u);
                u.unit_masks &= !order_dispatch::masks::PATH_EXHAUSTED;
            }
        }
    }

    /// Install the measured `Unit::add_build_order` payload and queue mutation.
    ///
    /// Arena has one land region and no transport runtime (MODEL 6), so the cross-region
    /// transport branch is unreachable rather than answered with guessed leader flags.
    fn assign_build_order(&mut self, unit: EntId, target: EntId) -> bool {
        let Some(builder) = self.ent(unit) else {
            return false;
        };
        let Some(site) = self.ent(target) else {
            return false;
        };
        if builder.building || !site.building || site.complete || builder.who != site.who {
            return false;
        }
        let target_key = ObjectKey {
            who: i32::from(site.who),
            o: i32::from(site.object_o),
            uid: site.object_uid,
        };
        let builder_region = self
            .collision_world
            .get_tregion(builder.tile().0, builder.tile().1);
        let target_region = self
            .collision_world
            .get_tregion(site.tile().0, site.tile().1);
        if builder_region != target_region {
            // The no-water Arena cannot faithfully execute the measured transport arm.
            return false;
        }

        self.detach(unit);
        let Some(e) = self.ent_mut(unit) else {
            return false;
        };
        let Some(motion) = e.motion.as_mut() else {
            return false;
        };
        let receipt = construction::install_build_order(
            motion,
            target_key,
            QueuePos::New,
            false,
            BuildOrderRegions {
                builder_region,
                target_region,
                builder_transport_type: 0,
                can_ever_transport: false,
                leader_flags: 0,
            },
        );
        debug_assert_eq!(receipt.rng_draws, 0);
        e.build_order = Some(receipt.target);
        e.job = Job::Work { target };
        true
    }

    fn retire_exact_gather(&mut self, owner: u8, unit: EntId) {
        let Some(worker) = self.ent(unit) else {
            return;
        };
        debug_assert_eq!(worker.who, owner);
        let worker_key = gather_key(worker);
        if !self.gather_runtime.has_exact_order(worker_key) {
            return;
        }
        let site_key = self
            .gather_runtime
            .exact_site_for_worker(worker_key)
            .expect("an exact Gather order retains its generational target identity");
        self.gather_runtime
            .retire_exact(worker_key, site_key)
            .expect("exact Gather retirement must preserve a valid owner-local chain");
    }

    fn queue_up(&mut self, pi: usize, producer: EntId, type_id: i32, count: u16) -> OrderResult {
        let Some(t) = self.types.get(type_id).cloned() else {
            return OrderResult::Invalid;
        };
        let Some(p) = self.ent(producer) else {
            return OrderResult::Invalid;
        };
        if !p.building || !p.complete {
            return OrderResult::Invalid;
        }
        // `WHERE` names the building type this is produced at.
        if t.where_ >= 0 && p.type_id != t.where_ {
            return OrderResult::Invalid;
        }
        if !self.preqs_met(pi, &t) {
            return OrderResult::Invalid;
        }
        let is_tech = !t.kind_unit && !t.kind_building;
        if is_tech {
            if self.players[pi].techs.contains(&type_id) {
                return OrderResult::Invalid;
            }
            // One research at a time per player, as the shipped economic game models it.
            if self.own_ents(pi).flat_map(|e| e.queue.iter()).any(|q| {
                self.types
                    .get(q.type_id)
                    .map(|x| !x.kind_unit)
                    .unwrap_or(false)
            }) {
                return OrderResult::Refused;
            }
        }
        let mut made = 0;
        for _ in 0..count {
            if !is_tech && self.pop(pi) + t.pop.max(1) > self.pop_cap(pi) {
                break;
            }
            if !self.can_pay(pi, &t.cost) {
                break;
            }
            let cost = t.cost;
            self.pay(pi, &cost);
            self.ent_mut(producer).unwrap().queue.push(QueueItem {
                type_id,
                frames_left: t.job_time.max(1),
            });
            made += 1;
            if is_tech {
                break;
            }
        }
        if made == 0 {
            OrderResult::Refused
        } else {
            OrderResult::Ok(1)
        }
    }

    fn build(&mut self, pi: usize, worker: EntId, type_id: i32, tx: i32, ty: i32) -> OrderResult {
        let Some(t) = self.types.get(type_id).cloned() else {
            return OrderResult::Invalid;
        };
        if !t.kind_building {
            return OrderResult::Invalid;
        }
        if self.ty(worker).map(|t| t.id) != Some(self.ids.citizen) {
            return OrderResult::Invalid;
        }
        if !self.preqs_met(pi, &t) {
            return OrderResult::Invalid;
        }
        // Retail also gates the second city on `City State`, and it is not a PREQ0 on
        // Small City; `crate::game` records the same rule.
        if type_id == self.ids.small_city
            && !self.players[pi].techs.contains(&self.ids.city_state)
            && self.count_type(pi, self.ids.small_city, true) >= 1
        {
            return OrderResult::Invalid;
        }
        if self.placement_ok(pi, type_id, tx, ty).is_err() {
            return OrderResult::Invalid;
        }
        if !self.can_pay(pi, &t.cost) {
            return OrderResult::Refused;
        }
        let cost = t.cost;
        self.pay(pi, &cost);
        let site = self.spawn(pi as u8, type_id, tx, ty, false);
        assert!(
            self.assign_build_order(worker, site),
            "a paid Arena site must atomically receive its BUILD_AT order"
        );
        self.note(pi as u8, format!("place {} at ({tx},{ty})", t.name));
        OrderResult::Ok(site.0 as i32)
    }

    fn note(&mut self, who: u8, text: String) {
        if self.logging {
            self.log.push(Event {
                frame: self.frame,
                who,
                text,
            });
        }
    }

    // -----------------------------------------------------------------------
    // The tick
    // -----------------------------------------------------------------------

    pub fn step(&mut self) {
        // `Objects::process_all` 0x0065DCE0 reads `GameData::frame` before
        // `Game::do_frame` increments it at 0x005924BF. Its first loop always rotates
        // across ten owner slots, even though Arena materialises only player slots 0..7.
        let object_frame = self.frame;
        let n = self.players.len();
        let unit_owners: Vec<usize> = (0..OWNER_SLOTS)
            .map(|k| (object_frame.rem_euclid(OWNER_SLOTS as i64) as usize + k) % OWNER_SLOTS)
            .filter(|&pi| pi < n && self.players[pi].alive)
            .collect();
        for &pi in &unit_owners {
            self.tick_economy(pi);
        }
        self.last_object_process_order.clear();
        // The decompiled loops are unequivocal: all rotating unit bands run first, then
        // building bands in fixed leader order 0..7. A site's frame transaction therefore
        // consumes and clears helper contributions made earlier in this same object pass.
        for &pi in &unit_owners {
            self.tick_band(pi, false, object_frame);
        }
        for pi in 0..n.min(BANDED_SLOTS) {
            if self.players[pi].alive {
                self.tick_band(pi, true, object_frame);
            }
        }
        self.frame += 1;
        self.reap();
        if self.frame % self.params.fog_period == 0 {
            self.update_fog();
        }
        self.check_defeat();
    }

    fn tick_economy(&mut self, pi: usize) {
        let age = self.age_of(pi);
        let mut gross = [0i32; NRES];
        let mut upkeep = [0i32; NRES];
        let cities = self
            .own_ents(pi)
            .filter(|e| e.type_id == self.ids.small_city && e.complete)
            .count() as i32;
        for (r, slot) in gross.iter_mut().enumerate() {
            *slot = self.types.constants.city_gather[r] * cities;
        }
        for e in self.own_ents(pi) {
            if e.building {
                let active_workers = if ordinary_gather_kind(&self.ids, e.type_id).is_some() {
                    self.gather_runtime
                        .exact_active_workers(gather_key(e))
                        .expect("registered gather site keeps a valid exact chain")
                        .unwrap_or(e.workers)
                } else {
                    e.workers
                };
                if e.complete && active_workers > 0 {
                    gross[e.gather_res] += self.types.constants.peasant_rate * active_workers;
                }
            } else if let Some(u) = self.types.upkeep.get(&e.type_id) {
                for r in 0..NRES {
                    upkeep[r] += u[r];
                }
            }
        }
        let period = if self.params.gather_period_shift == 4 {
            resource_period(self.econ.gather_rate)
        } else {
            self.econ
                .gather_rate
                .wrapping_shl(self.params.gather_period_shift)
        };
        for res in 0..NRES {
            let cap = commerce_cap(age, res, &self.econ, &CommerceCapGates::default(), 0);
            let input = ResourceTickInput {
                res,
                gross: gross[res],
                expense: upkeep[res],
                bonus: 0,
                commerce_cap: cap,
                stockpile: self.players[pi].stock[res],
                interest_threshold: 0,
                interest_applies: false,
                gather_bonus_pct: self.params.difficulty_income_bonus,
                difficulty: 3,
                game_flag_0x20_bit1: false,
                game_flag_0x2a_is_9: false,
                game_speed: 1,
            };
            let out = resource_tick(&input, &self.econ);
            if let Some(income) = out.accumulated {
                let acc = &mut self.players[pi].acc[res];
                let whole = credit_resource(income, period, acc);
                if whole != 0 {
                    self.players[pi].stock[res] = self.players[pi].stock[res].saturating_add(whole);
                    if whole > 0 {
                        self.players[pi].gathered[res] += whole as i64;
                    }
                }
            }
        }
    }

    fn tick_band(&mut self, pi: usize, buildings: bool, object_frame: i64) {
        let mut idxs: Vec<usize> = (0..self.ents.len())
            .filter(|&i| {
                self.ents[i].alive
                    && self.ents[i].who as usize == pi
                    && self.ents[i].building == buildings
            })
            .collect();
        idxs.sort_unstable_by_key(|&i| self.ents[i].object_o);
        for i in idxs {
            if !self.ents[i].alive {
                continue;
            }
            self.last_object_process_order.push(self.ents[i].id);
            if buildings {
                self.begin_construction_site_frame(i);
            }
            target::decay_targeted(
                &mut self.target_world,
                object_frame as i32,
                target_ref(&self.ents[i]),
            );
            self.tick_queue(i);
            self.tick_cycle(i);
            self.tick_job(i);
            if !buildings && self.ents[i].alive {
                self.tick_supply_healing(i);
                self.tick_attrition_recompute(i);
                self.tick_supply_attrition(i);
            }
        }
    }

    /// The supply-specific land arm of `Unit::process_healing`, immediately before the
    /// recovered attrition tail. Other healing families retain their explicit blocker.
    fn tick_supply_healing(&mut self, i: usize) -> SupplyHealingTransaction {
        // This is Unit::process_healing's first supply-arm prerequisite and keeps the
        // ordinary undamaged unit band from constructing an O(n) object-table host.
        if self.ents[i].hp.damage <= 0 {
            return SupplyHealingTransaction::NoDamage;
        }
        let who = i32::from(self.ents[i].who);
        let o = i32::from(self.ents[i].object_o);
        let id = self.ents[i].id;
        let transaction = {
            let mut host = ArenaSupplyHost {
                ents: &mut self.ents,
                types: &self.types,
                players: &self.players,
                supply_records: &self.supply_records,
                hero_records: &self.hero_records,
                territory: &self.collision_world,
                frame: self.frame,
            };
            retail_systems::execute_supply_healing(self.frame as i32, who, o, &mut host)
        }
        .unwrap_or_else(|error| {
            panic!("Arena supply-healing transaction failed for ({who},{o}): {error:?}")
        });
        if matches!(transaction, SupplyHealingTransaction::Healed { .. }) {
            self.sync_target_damage(id);
        }
        transaction
    }

    /// The exact 32-frame call site before the due attrition tail. Arena closes the
    /// universal reset and friendly-territory return; the transaction receipt retains the
    /// explicit non-friendly authority boundary instead of synthesising a period.
    fn tick_attrition_recompute(&mut self, i: usize) -> AttritionRecomputeTransaction {
        if (self.frame as i32).wrapping_add(i32::from(self.ents[i].object_o)) % 32 != 0 {
            return AttritionRecomputeTransaction::NotDue;
        }
        let who = i32::from(self.ents[i].who);
        let o = i32::from(self.ents[i].object_o);
        let mut host = ArenaSupplyHost {
            ents: &mut self.ents,
            types: &self.types,
            players: &self.players,
            supply_records: &self.supply_records,
            hero_records: &self.hero_records,
            territory: &self.collision_world,
            frame: self.frame,
        };
        retail_systems::execute_attrition_recompute(self.frame as i32, who, o, &mut host)
            .unwrap_or_else(|error| {
                panic!("Arena attrition-recompute transaction failed for ({who},{o}): {error:?}")
            })
    }

    /// The due-frame block at the tail of `Unit::process` (`0x006117F6`). The adapter
    /// borrows the actual Arena tables for the whole transaction, then synchronises the
    /// existing target/death lifecycle exactly when damage was applied.
    fn tick_supply_attrition(&mut self, i: usize) -> SupplyAttritionTransaction {
        if !don_sim::systems::borders_fog::attrition_due(
            self.frame as i32,
            self.ents[i].object_o,
            self.ents[i].attrition_period,
        ) {
            return SupplyAttritionTransaction::NotDue;
        }
        let who = i32::from(self.ents[i].who);
        let o = i32::from(self.ents[i].object_o);
        let id = self.ents[i].id;
        let transaction = {
            let mut host = ArenaSupplyHost {
                ents: &mut self.ents,
                types: &self.types,
                players: &self.players,
                supply_records: &self.supply_records,
                hero_records: &self.hero_records,
                territory: &self.collision_world,
                frame: self.frame,
            };
            retail_systems::execute_supply_attrition(self.frame as i32, who, o, &mut host)
        }
        .unwrap_or_else(|error| {
            panic!("Arena supply/attrition transaction failed for ({who},{o}): {error:?}")
        });

        if let SupplyAttritionTransaction::Damaged { outcome, .. } = transaction {
            self.sync_target_damage(id);
            if outcome != DamageOutcome::Survived {
                // Retail returns from Unit::process immediately after lethal attrition;
                // make the ordinary Arena close path visible before the next object slot.
                self.reap();
            }
        }
        transaction
    }

    fn begin_construction_site_frame(&mut self, i: usize) {
        let type_id = self.ents[i].type_id;
        let is_wonder = (0x20E..0x21F).contains(&type_id);
        let Some(site) = self.ents[i].build.as_mut() else {
            panic!(
                "Arena building {} has no persistent BuildData",
                self.ents[i].id.0
            );
        };
        let receipt = construction::begin_site_frame(
            site,
            ConstructionFrame {
                construct_query: Default::default(),
                is_wonder,
            },
            &self.prod_rules,
        );
        debug_assert_eq!(receipt.rng_draws, 0);
        self.ents[i].build_left = if site.is_active() {
            0
        } else {
            site.constr_time
                .saturating_sub(site.job_counter)
                .min(i32::MAX as u32) as i32
        };
    }

    fn tick_cycle(&mut self, i: usize) {
        self.ents[i].cycle.tick();
    }

    fn tick_queue(&mut self, i: usize) {
        if !self.ents[i].building || !self.ents[i].complete || self.ents[i].queue.is_empty() {
            return;
        }
        let done = {
            let q = &mut self.ents[i].queue[0];
            q.frames_left -= 1;
            if q.frames_left <= 0 {
                Some(q.type_id)
            } else {
                None
            }
        };
        let Some(type_id) = done else { return };
        self.ents[i].queue.remove(0);
        let who = self.ents[i].who;
        let (tx, ty) = self.ents[i].tile();
        let Some(t) = self.types.get(type_id).cloned() else {
            return;
        };
        if t.kind_unit {
            // Spawn on the first free tile around the producer.
            let mut placed = false;
            'outer: for r in 1..6 {
                for (dx, dy) in ring(r) {
                    if self.map.at(tx + dx, ty + dy).passable() {
                        self.spawn(who, type_id, tx + dx, ty + dy, true);
                        placed = true;
                        break 'outer;
                    }
                }
            }
            if placed {
                self.note(who, format!("trained {}", t.name));
            }
        } else {
            self.players[who as usize].techs.insert(type_id);
            self.note(who, format!("researched {}", t.name));
        }
    }

    fn tick_job(&mut self, i: usize) {
        let job = self.ents[i].job;
        match job {
            Job::Idle => {
                self.service_idle_unit(i);
                self.auto_acquire(i);
            }
            Job::MoveTo { x, y } => match self.step_toward(i, x, y, 0) {
                MoveProgress::Arrived | MoveProgress::Failed => {
                    self.ents[i].job = Job::Idle;
                }
                MoveProgress::Working => {}
            },
            Job::Attack { target } => self.do_attack(i, target),
            Job::Gather { target } => {
                let Some(b) = self.ent(target).cloned() else {
                    self.retire_motion_order(i, KillReason::Failed);
                    self.ents[i].job = Job::Idle;
                    return;
                };
                if !b.complete {
                    // Seat it once the site finishes; meanwhile help build.
                    self.ents[i].job = Job::Work { target };
                    return;
                }
                if self.ents[i].assigned_to == target {
                    return; // seated, nothing to do
                }
                match self.step_toward(i, b.x, b.y, RANGE_UNITS_PER_TILE) {
                    MoveProgress::Arrived => {
                        let cap_free = {
                            let bb = self.ent(target).unwrap();
                            bb.workers < bb.worker_cap
                        };
                        if cap_free {
                            self.ent_mut(target).unwrap().workers += 1;
                            self.ents[i].assigned_to = target;
                        } else {
                            self.ents[i].job = Job::Idle;
                        }
                    }
                    MoveProgress::Failed => {
                        self.ents[i].job = Job::Idle;
                    }
                    MoveProgress::Working => {}
                }
            }
            Job::Work { target } => {
                let Some(b) = self.ent(target).cloned() else {
                    self.detach(self.ents[i].id);
                    self.ents[i].job = Job::Idle;
                    return;
                };
                if b.complete && b.hits_left() >= b.hp.myhits {
                    self.detach(self.ents[i].id);
                    // Finished and undamaged: a gatherer keeps its builder, everything
                    // else releases them.
                    self.ents[i].job = if b.worker_cap > 0 {
                        Job::Gather { target }
                    } else {
                        Job::Idle
                    };
                    return;
                }
                match self.step_toward(i, b.x, b.y, RANGE_UNITS_PER_TILE) {
                    MoveProgress::Arrived => {
                        if !b.complete {
                            self.preflight_construction_builder(i, target);
                        } else {
                            // Repair: 1/16 of a hit point per frame, the same granularity the
                            // engine damages in (`HitPoints::accumulate`).
                            let e = self.ent_mut(target).unwrap();
                            e.hp.damage = (e.hp.damage - 1).max(0);
                            self.sync_target_damage(target);
                        }
                    }
                    MoveProgress::Failed => {
                        self.detach(self.ents[i].id);
                        self.ents[i].job = Job::Idle;
                    }
                    MoveProgress::Working => {}
                }
            }
        }
    }

    /// Run the exact recovered `Unit::do_build` policy through the first world effect
    /// Arena cannot execute. No construction counter changes on refusal.
    fn preflight_construction_builder(&mut self, i: usize, target: EntId) {
        let Some(order) = self.ents[i].build_order else {
            panic!(
                "Arena builder {} reached an incomplete site without (who,o,uid)",
                self.ents[i].id.0
            );
        };
        let Some(site_index) = target.index() else {
            return;
        };
        let Some(site) = self.ents.get(site_index) else {
            return;
        };
        let Some(site_build) = site.build.as_ref() else {
            panic!("construction target {} has no BuildData", target.0);
        };
        let target_type = self
            .types
            .get(site.type_id)
            .expect("live construction target keeps its type row");
        let builder = &self.ents[i];
        let builder_key = ObjectKey {
            who: i32::from(builder.who),
            o: i32::from(builder.object_o),
            uid: builder.object_uid,
        };
        let (btx, bty) = builder.tile();
        let (ttx, tty) = site.tile();
        let half_x = (target_type.x_size / 2).max(0);
        let half_y = (target_type.y_size / 2).max(0);
        let builder_tile_is_covered = (btx - ttx).abs() <= half_x && (bty - tty).abs() <= half_y;
        let order_flags = builder
            .motion
            .as_ref()
            .and_then(|u| {
                u.orders
                    .iter()
                    .find(|o| o.kind == don_sim::order::OrderIndex::BuildAt)
            })
            .map_or(0, |o| o.flags);
        let unit_decoy = builder
            .motion
            .as_ref()
            .is_some_and(|u| u.unit_masks & 1 != 0);
        let plan = construction_builder::preflight(PreflightInput {
            builder: builder_key,
            target_order: order.target,
            // Direct `do_build` deliberately ignores the captured UID here.
            target_is_valid_wall: site.alive && site_build.is_valid(),
            target_is_active: site_build.is_active(),
            has_next_action_after_retire: false,
            adjacent: Map::tile_dist((btx, bty), (ttx, tty)) <= 1,
            builder_tile_is_covered,
            target_is_farm: site.type_id == self.ids.farm,
            order_flags,
            builder_x: builder.x,
            builder_y: builder.y,
            target_x: site.x,
            target_y: site.y,
            builder_angle: builder.facing,
            unit_decoy,
        });

        let refusal = match plan {
            PreflightPlan::Reswarm { .. } => {
                if self.params.construction_mode == ConstructionMode::ResearchModel {
                    // MODEL: the playable arena has no temporary Group/reswarm host. Its
                    // old one-tile approach is retained only on this explicitly labelled
                    // path, while progress itself uses the recovered harmonic kernel.
                    self.apply_research_construction(
                        i,
                        site_index,
                        construction_builder::CHAR_BUILD,
                        None,
                        true,
                    );
                    return;
                }
                ConstructionRefusal::MissingReswarmTransaction
            }
            PreflightPlan::AnimateFace {
                animation,
                set_angle,
                contribute,
            } => {
                if self.params.construction_mode == ConstructionMode::ResearchModel {
                    self.apply_research_construction(
                        i, site_index, animation, set_angle, contribute,
                    );
                    return;
                }
                // `Unit::set_anim` 0x00616F40 calls the 4,723-byte Guy::set_anim body for
                // every squad/crew member before `Unit::set_angle`. The current don-sim
                // primitive returns this ordered plan but does not own that transaction;
                // setting only `GuyData::cur_anim` would be an invented shortcut.
                ConstructionRefusal::MissingBuilderAnimationTransaction
            }
            PreflightPlan::RetireInvalid { .. } => {
                self.detach(self.ents[i].id);
                self.ents[i].job = Job::Idle;
                return;
            }
            PreflightPlan::RetireActive => {
                self.detach(self.ents[i].id);
                self.ents[i].job = Job::Idle;
                return;
            }
        };
        self.ents[site_index].construction_refusal = Some(refusal);
    }

    /// Playable-only adapter around the exact local construction arithmetic.
    ///
    /// The `BuildData` writes and harmonic helper sequence are the recovered primitives.
    /// Animation, start and activation below remain an explicit research model because
    /// Arena cannot return the mandatory full-world receipts. Claim-bearing mode never
    /// enters this function.
    fn apply_research_construction(
        &mut self,
        builder_index: usize,
        site_index: usize,
        animation: i32,
        set_angle: Option<i32>,
        contribute: bool,
    ) {
        for guy in self.ents[builder_index].guys.guys.iter_mut().flatten() {
            // MODEL: `Unit::set_anim`/`Guy::set_anim` have a larger transaction. This
            // carries only the visible current-animation byte for playable research runs.
            guy.cur_anim = animation as i8;
            if let Some(angle) = set_angle {
                guy.des_angle = angle;
            }
        }
        if let Some(angle) = set_angle {
            self.ents[builder_index].facing = angle;
            if let Some(motion) = self.ents[builder_index].motion.as_mut() {
                motion.body.angle = angle;
            }
        }
        if !contribute {
            return;
        }

        let mut completed = false;
        {
            let site = self.ents[site_index]
                .build
                .as_mut()
                .expect("construction site retains BuildData");
            if !site.is_started() {
                // MODEL: flags/frame only; the terrain/visibility/competition transaction
                // required by Build::start is deliberately not claimed here.
                site.flags |= production::flag::STARTED;
                site.frame_started = self.frame as i32;
            }
            if site.build_masks & production::mask::HELPER_COUNTED == 0 {
                site.recharging = site.recharging.wrapping_add(1);
                site.build_masks |= production::mask::HELPER_COUNTED;
            }
            let total = production::construct_time(
                site.constr_time,
                false,
                &Default::default(),
                &self.prod_rules,
            );
            let rate = production::construct_rate(site.is_under_attack(), false, &self.prod_rules);
            let step = production::do_construct(
                rate,
                1,
                site.is_active(),
                site.job_counter,
                site.job_counter_2,
                site.helpers,
                total,
            );
            site.job_counter = step.job_counter;
            site.job_counter_2 = step.job_counter_2;
            site.helpers = step.helpers;
            if step.completed {
                // MODEL: only the recovered local activation core. Leader/city/terrain,
                // event, Farm RNG and registry transactions remain absent blockers.
                site.flags |= production::flag::ACTIVE;
                site.build_masks |= 0x1000;
                site.job_counter = 0;
                site.job_counter_2 = 0;
                site.recharging = 0;
                site.construct_hits = site.myhits;
                completed = true;
            }
        }

        let site = self.ents[site_index]
            .build
            .as_ref()
            .expect("construction site retains BuildData");
        self.ents[site_index].build_left = if completed {
            0
        } else {
            site.constr_time
                .saturating_sub(site.job_counter)
                .min(i32::MAX as u32) as i32
        };
        if completed {
            self.ents[site_index].complete = true;
            let who = self.ents[site_index].who;
            let type_id = self.ents[site_index].type_id;
            let name = self
                .types
                .get(type_id)
                .map(|t| t.name.clone())
                .unwrap_or_default();
            self.note(who, format!("built {name}"));
        }
    }

    fn retire_motion_order(&mut self, i: usize, reason: KillReason) {
        let Some(motion) = self.ents[i].motion.as_mut() else {
            return;
        };
        if !motion.orders.is_empty() {
            order_dispatch::kill_current_order(motion, reason);
        }
    }

    /// Retail calls `Unit::work` for an idle unit too.  Besides the target-think phase,
    /// that services checksum-visible periodic fields and decrements `UnitData::safe`;
    /// freezing the motion record merely because the arena job is idle can carry a
    /// collision retry delay into a much later player order.
    fn service_idle_unit(&mut self, i: usize) {
        if self.ents[i].building || self.ents[i].motion.is_none() {
            return;
        }
        let mut u = self.ents[i].motion.take().expect("checked unit motion");
        assert!(
            u.orders.is_empty(),
            "idle arena object {} at frame {} retained retail order {:?}",
            self.ents[i].id.0,
            self.frame,
            u.orders.front(),
        );
        let coll_index = self
            .collision_units
            .find(u.who as i32, u.o as i32)
            .expect("every arena unit is registered in collision");
        {
            let row = &mut self.collision_units.rows[coll_index];
            row.safe = u.safe;
            row.unit_masks = u.unit_masks;
            row.moving = false;
            row.order = 0;
            row.action = 0;
            row.has_orders = false;
            row.searching = u.parked_search;
            row.path_top_flags = u.path.peek().map_or(0, |p| p.flags as u8);
        }
        let occupied_tiles = self
            .ents
            .iter()
            .filter(|e| e.alive && e.building)
            .map(Ent::tile)
            .collect();
        let collision_me = self.collision_units.rows[coll_index];
        let mut host = ArenaMoveWorld {
            map: &self.map,
            occupied_tiles,
            collision: RefCell::new(ArenaCollisionProbe {
                world: &mut self.collision_world,
                check: &mut self.collision_check,
                units: &mut self.collision_units,
                me: collision_me,
                step_dest: None,
            }),
            frame: self.frame as i32,
            rng: &mut self.game_random,
        };
        let before = (u.body.x, u.body.y, u.body.angle);
        let report = order_dispatch::work(
            &mut u,
            &mut host,
            &mut self.pathfinder,
            &mut self.movement_coverage,
        );
        drop(host);
        assert!(matches!(
            report.result,
            ArmResult::Empty | ArmResult::NoOrder
        ));
        assert_eq!(
            (u.body.x, u.body.y, u.body.angle),
            before,
            "idle Unit::work must not translate or turn its body"
        );
        let row = &mut self.collision_units.rows[coll_index];
        row.safe = u.safe;
        row.unit_masks = u.unit_masks;
        row.searching = u.parked_search;
        row.path_top_flags = u.path.peek().map_or(0, |p| p.flags as u8);
        self.ents[i].motion = Some(u);
    }

    /// Execute one frame of retail `Unit::work` against the arena map. The persistent
    /// [`UnitWork`] therefore services periodic masks, safe countdown, order dispatch,
    /// `do_move`, path/collision response and parked searches in their measured order; the
    /// world's singleton [`PathFinder`] and main [`Random`] pass through unchanged.
    fn step_toward(&mut self, i: usize, gx: i32, gy: i32, tolerance: i32) -> MoveProgress {
        let Some(t) = self.types.get(self.ents[i].type_id).cloned() else {
            return MoveProgress::Failed;
        };
        if t.moves <= 0 || self.ents[i].building {
            return MoveProgress::Failed;
        }

        let tol = tolerance.max(UCELL);
        let mut u = match self.ents[i].motion.take() {
            Some(u) => u,
            None => return MoveProgress::Failed,
        };
        let replace = u.orders.front().is_none_or(|o| {
            o.kind != don_sim::order::OrderIndex::MoveTo
                || o.x != gx
                || o.y != gy
                || u.tolerance != tol
        });
        if replace {
            if let Some(build_order) = self.ents[i].build_order {
                assert!(
                    u.orders.iter().any(|o| {
                        o.kind == don_sim::order::OrderIndex::BuildAt
                            && o.target_who == build_order.target.who
                            && o.target_o == build_order.target.o
                            && o.target_uid == build_order.target.uid
                    }),
                    "construction movement must retain the generational BUILD_AT tail"
                );
                // `check_build_order` inserts a MOVE_TO leg ahead of BUILD_AT. Arena's
                // movement executor owns only one such leg, so retire an obsolete front
                // leg before inserting the current destination without touching the tail.
                if u.orders
                    .front()
                    .is_some_and(|o| o.kind == don_sim::order::OrderIndex::MoveTo)
                {
                    u.orders.reset();
                    let _ = u.orders.remove_current();
                }
                u.orders.push_front(OrderRec::move_to(gx, gy, tol));
            } else {
                u.orders.replace(OrderRec::move_to(gx, gy, tol));
            }
            order_dispatch::clear_partial_path(&mut u);
            u.unit_masks &= !order_dispatch::masks::PATH_EXHAUSTED;
        }
        u.tolerance = tol;

        // `do_move` calls `GuyData::turn_speed(0)` using these exact live type fields.
        if let Some(g) = self.ents[i].guys.guys.first().and_then(Option::as_ref) {
            u.lead_guy = *g;
        }

        let collision_frame = self.frame as i32;
        if self.collision_units.frame != collision_frame {
            self.collision_units.frame = collision_frame;
            self.collision_units.budget = [0; 10];
        }
        let coll_index = self
            .collision_units
            .find(u.who as i32, u.o as i32)
            .expect("every moving arena unit is registered in collision");
        {
            let row = &mut self.collision_units.rows[coll_index];
            row.x = u.body.x;
            row.y = u.body.y;
            row.safe = u.safe;
            row.unit_masks = u.unit_masks;
            row.moving = !u.orders.is_empty();
            row.order = u.orders.front().map_or(0, |o| o.kind as i32);
            row.action = row.order;
            row.has_orders = !u.orders.is_empty();
            row.searching = u.parked_search;
            row.path_top_flags = u.path.peek().map_or(0, |p| p.flags as u8);
        }
        let collision_me = self.collision_units.rows[coll_index];
        let occupied_tiles = self
            .ents
            .iter()
            .filter(|e| e.alive && e.building)
            .map(Ent::tile)
            .collect();
        let before = (u.body.x, u.body.y);
        let mut host = ArenaMoveWorld {
            map: &self.map,
            occupied_tiles,
            collision: RefCell::new(ArenaCollisionProbe {
                world: &mut self.collision_world,
                check: &mut self.collision_check,
                units: &mut self.collision_units,
                me: collision_me,
                step_dest: None,
            }),
            frame: self.frame as i32,
            rng: &mut self.game_random,
        };
        let result = order_dispatch::work(
            &mut u,
            &mut host,
            &mut self.pathfinder,
            &mut self.movement_coverage,
        )
        .result;
        let after = (u.body.x, u.body.y);
        drop(host);
        assert!(
            collision::relocate_unit_anchor(
                &mut self.collision_world,
                &mut self.collision_units,
                u.who as i32,
                u.o as i32,
                after.0,
                after.1,
            ),
            "moving arena unit must remain linked in its retail WData chain"
        );
        assert!(
            self.target_world
                .relocate(target_ref(&self.ents[i]), after.0, after.1),
            "moving arena unit must remain linked in its target-acquisition WData chain"
        );
        self.ents[i].x = after.0;
        self.ents[i].y = after.1;
        self.ents[i].facing = u.body.angle;
        let moved = don_sim::systems::movement::vector_dist(after.0 - before.0, after.1 - before.1);
        for g in self.ents[i]
            .guys
            .guys
            .iter_mut()
            .take(t.squad_size.max(0) as usize)
            .flatten()
        {
            let old = (g.x, g.y);
            let new = (
                g.x.wrapping_add(after.0 - before.0),
                g.y.wrapping_add(after.1 - before.1),
            );
            collision::guy_set_new_location(
                &mut self.collision_world,
                old,
                new,
                t.domain,
                g.guy_num as i32,
                t.squad_size,
                t.new_block_radius,
            );
            g.x = new.0;
            g.y = new.1;
            g.des_x = g.x;
            g.des_y = g.y;
            g.angle = u.body.angle;
            g.last_speed = moved;
            g.update_avg_speed();
        }
        if let Some(g) = self.ents[i].guys.guys.first().and_then(Option::as_ref) {
            u.lead_guy = *g;
        }
        if let Some(ci) = self.collision_units.find(u.who as i32, u.o as i32) {
            let row = &mut self.collision_units.rows[ci];
            row.x = after.0;
            row.y = after.1;
            row.safe = u.safe;
            row.unit_masks = u.unit_masks;
            row.moving = !u.orders.is_empty();
            row.order = u.orders.front().map_or(0, |o| o.kind as i32);
            row.action = row.order;
            row.has_orders = !u.orders.is_empty();
            row.searching = u.parked_search;
            row.path_top_flags = u.path.peek().map_or(0, |p| p.flags as u8);
        }
        let live_positions: Vec<(i32, i32)> = self.ents[i]
            .guys
            .guys
            .iter()
            .take(t.squad_size.max(0) as usize)
            .flatten()
            .map(|g| (g.x, g.y))
            .collect();
        for (tg, (x, y)) in self
            .collision_units
            .guys
            .iter_mut()
            .filter(|tg| tg.who == u.who as i32 && tg.o == u.o as i32)
            .zip(live_positions)
        {
            tg.body.x = x;
            tg.body.y = y;
        }
        self.ents[i].motion = Some(u);

        match result {
            ArmResult::Retired(KillReason::Completed) => MoveProgress::Arrived,
            ArmResult::Retired(KillReason::Failed) | ArmResult::NoOrder => MoveProgress::Failed,
            ArmResult::Working | ArmResult::Moved | ArmResult::Turned | ArmResult::Blocked => {
                MoveProgress::Working
            }
            ArmResult::NotPorted
            | ArmResult::Empty
            | ArmResult::Fired(_)
            | ArmResult::Gathered(_)
            | ArmResult::MalformedOrder => MoveProgress::Failed,
        }
    }

    /// `Unit::think_attack` -> `find_new_target` for an ordinary idle land unit.
    ///
    /// Cadence, response distance, spiral/cell-chain order, fog, terrain region,
    /// `poor_target`, priority, target crowding and deterministic tie-breaking all belong
    /// to the shared retail implementation. This host supplies the live object/type state;
    /// it performs no fallback scan.
    fn auto_acquire(&mut self, i: usize) {
        let Some(e) = self.ents.get(i) else {
            return;
        };
        let Some(t) = self.types.get(e.type_id) else {
            return;
        };
        if e.building || t.attack <= 0 || t.domain != 0 {
            return;
        }
        let searcher = target_ref(e);
        let unit_masks = e.motion.as_ref().map_or(0, |u| u.unit_masks);
        let query = AutoTargetQuery {
            searcher,
            frame: self.frame as i32,
            max_range_tiles: t.max_range,
            min_range_tiles: t.min_range,
            stance: e.stance as i32,
            unit_masks,
            has_objmask_high: t.obj_masks & 0x8000_0000 != 0,
            unit_respond_range: self.combat.unit_respond_range,
            unit_defensive_respond_range: self.combat.unit_defensive_respond_range,
            compare_mode: !self.players[e.who as usize].leader_ai,
            last_order_target: None,
        };
        let adapter = ArenaTargetAdapter {
            ents: &self.ents,
            types: &self.types,
            players: &self.players,
            map: &self.map,
            territory: &self.collision_world,
            balance: &self.balance,
            combat: &self.combat,
            frame: self.frame as i32,
        };
        let step =
            target::find_auto_target(&mut self.target_world, &self.target_circle, query, &adapter);
        let AutoTargetStep::Searched { result, .. } = step else {
            return;
        };
        let Some(best) = result.best else { return };
        let Some(target_index) = object_ref_index(&self.ents, best) else {
            return;
        };
        let target_ent = &self.ents[target_index];
        assert_eq!(target_ent.who as i16, best.who, "target slot owner drift");
        self.ents[i].job = Job::Attack {
            target: target_ent.id,
        };
    }

    fn sync_target_damage(&mut self, id: EntId) {
        let Some(e) = id.index().and_then(|i| self.ents.get(i)) else {
            return;
        };
        if let Some(row) = self.target_world.row_mut(target_ref(e)) {
            row.damage = e.hp.damage;
        }
    }

    fn do_attack(&mut self, i: usize, target: EntId) {
        let Some(tgt) = self.ent(target).cloned() else {
            self.retire_motion_order(i, KillReason::Failed);
            self.ents[i].job = Job::Idle;
            return;
        };
        let attacker = self.ents[i].clone();
        let Some(at) = self.types.get(attacker.type_id).cloned() else {
            return;
        };
        let Some(dt) = self.types.get(tgt.type_id).cloned() else {
            return;
        };
        let dist = self.attack_dist(&attacker, &tgt, &dt);
        if !in_attack_range(dist, at.max_range) {
            if at.moves > 0 {
                self.step_toward(i, tgt.x, tgt.y, at.max_range * RANGE_UNITS_PER_TILE);
            }
            return;
        }
        if below_min_range(dist, at.min_range) {
            if at.moves > 0 {
                // Back off to at least min range.
                let away_x = attacker.x * 2 - tgt.x;
                let away_y = attacker.y * 2 - tgt.y;
                self.step_toward(i, away_x, away_y, 0);
            }
            return;
        }
        if self.ents[i].cycle.recharging != 0 {
            return;
        }

        let (damage_attack_dir, flank, shot_count) = if !attacker.building {
            // MODEL 6 keeps non-land domains outside this arena. Do not run their broadside
            // or aircraft state through the direct-land firing arm.
            if at.domain != 0 {
                return;
            }
            let aim_mode = AimMode::from_guys(&self.ents[i].guys);
            let input = UnitVolleyInput {
                attacker_x: attacker.x,
                attacker_y: attacker.y,
                attacker_facing: attacker.facing,
                target_x: tgt.x,
                target_y: tgt.y,
                defender_facing: tgt.facing,
                squad_size: at.squad_size,
                guys: &self.ents[i].guys,
                aim_mode,
            };
            let Ok(plan) = plan_direct_land_volley(at.domain, &input) else {
                // In particular, graphics-turret Guys stop here until the graphics graph
                // supplies `AimMode::GraphicsTurret { aligned }`; target-bearing is not a
                // safe substitute for the retained-body retail arm.
                return;
            };
            self.apply_unit_volley_aim(i, &plan);
            (
                plan.damage_attack_dir,
                (!tgt.building).then_some(plan.flank_tier),
                plan.shot_count,
            )
        } else {
            // Buildings do not enter the Unit/Unit flank predicate. Their own projectile
            // body is a single Object::do_damage producer.
            let dir = don_sim::systems::target::attack_dir(attacker.x, attacker.y, tgt.x, tgt.y);
            self.ents[i].facing = dir;
            (dir, None, 1)
        };

        let mut dealt = 0i32;
        for _ in 0..shot_count {
            dealt = dealt.wrapping_add(self.fire(i, target, &at, &dt, damage_attack_dir, flank));
        }
        let rech = if attacker.building {
            at.recharge
        } else {
            // UnitTypeData::is_siege 0x00470460 is unit_flags bit 0x20000.
            let is_siege = at.unit_flags & 0x2_0000 != 0;
            let unit_masks2_bit0 = self.ents[i]
                .motion
                .as_ref()
                .expect("live Arena unit owns UnitWork")
                .unit_masks2
                & 1
                != 0;
            let in_supply = if !is_siege {
                true
            } else if self.combat.artillery_under_attack_fires_slowly != 0 && unit_masks2_bit0 {
                // The measured under-attack gate skips UnitData::in_supply entirely.
                false
            } else {
                self.reload_supply_state(i) != ReloadSupplyState::OutOfSupply
            };
            don_sim::systems::combat::recharge_frames(
                &don_sim::systems::combat::RechargeInput {
                    base_recharge: at.recharge,
                    is_siege,
                    unit_masks2_bit0,
                    in_supply,
                    is_bombard: type_is(&self.types, at.id, 0x10B),
                },
                &self.combat,
            )
        };
        self.ents[i].cycle.fire(rech);
        let who = self.ents[i].who;
        self.players[who as usize].damage_dealt += dealt as i64;
        let tw = tgt.who as usize;
        self.players[tw].damage_taken += dealt as i64;
    }

    /// `UnitData::in_supply` (`0x00609EF0`), called only from the siege override of
    /// `UnitData::recharge` after a completed volley.
    fn reload_supply_state(&mut self, i: usize) -> ReloadSupplyState {
        let who = i32::from(self.ents[i].who);
        let o = i32::from(self.ents[i].object_o);
        let host = ArenaSupplyHost {
            ents: &mut self.ents,
            types: &self.types,
            players: &self.players,
            supply_records: &self.supply_records,
            hero_records: &self.hero_records,
            territory: &self.collision_world,
            frame: self.frame,
        };
        retail_systems::resolve_reload_supply(who, o, &host).unwrap_or_else(|error| {
            panic!("Arena reload-supply query failed for ({who},{o}): {error:?}")
        })
    }

    /// Apply the exact checksum-visible aim writes prepared by `Unit::fight`'s direct-land
    /// volley arm. Current Guy angles remain animation state; retail writes `des_angle`.
    fn apply_unit_volley_aim(&mut self, i: usize, plan: &UnitVolleyPlan) {
        for aim in &plan.guy_aims {
            self.ents[i].guys.guys[aim.slot]
                .as_mut()
                .expect("volley planner validated the live Guy pointer")
                .des_angle = aim.des_angle;
        }
        let lead = self.ents[i]
            .guys
            .guys
            .first()
            .and_then(Option::as_ref)
            .copied();
        self.ents[i].facing = plan.unit_facing;
        if let Some(u) = &mut self.ents[i].motion {
            u.body.angle = plan.unit_facing;
            if plan.toggle_unit_mask_2 {
                u.unit_masks ^= 2;
            }
            if let Some(g) = lead {
                u.lead_guy = g;
            }
        }
        let (who, o, masks) = self.ents[i].motion.as_ref().map_or(
            (self.ents[i].who as i32, i32::from(self.ents[i].object_o), 0),
            |u| (u.who as i32, u.o as i32, u.unit_masks),
        );
        if let Some(ci) = self.collision_units.find(who, o) {
            let row = &mut self.collision_units.rows[ci];
            row.angle = plan.unit_facing;
            row.unit_masks = masks;
        }
    }

    /// `ObjectData::attack_dist` is edge-to-edge: `vector_dist` with the target's
    /// footprint subtracted (`block_radius + 0x18`, or `x_size`/`y_size` x `0x60` for a
    /// building). The footprint term is a **reading** of that comment, not a measurement.
    fn attack_dist(&self, a: &Ent, b: &Ent, bt: &TypeRow) -> i32 {
        let d = vector_dist_between(a.x, a.y, b.x, b.y);
        let foot = if bt.kind_building {
            bt.x_size.max(bt.y_size) * 0x60
        } else {
            bt.block_radius.wrapping_add(0x18)
        };
        (d - foot).max(0)
    }

    /// One shot. Everything numeric here comes out of `don_sim::mechanics::damage`.
    fn fire(
        &mut self,
        i: usize,
        target: EntId,
        at: &TypeRow,
        dt: &TypeRow,
        attack_dir: i32,
        flank: Option<u32>,
    ) -> i32 {
        let a = self.ents[i].clone();
        let b = self.ent(target).cloned().unwrap();
        let balance_pct = self.balance.get(at.id, dt.id).unwrap_or(100);
        let input = DamageInput {
            balance_pct,
            // No armory/military upgrades are modelled, so both getters take their base
            // path -- the Tier-B path of `get_attack` / `get_armor`.
            attack: get_attack(at.attack, false, 0, 0),
            armor: get_armor(dt.armor, false, 0, 0),
            attacker_masks: at.obj_masks,
            defender_masks: dt.obj_masks,
            attack_dir,
            splash_flag: 0,
            overkill_gate: 0,
            attacker_player: a.who as u32,
            attacker_type_id: at.id,
            attacker_domain: at.domain,
            attacker_splash_percent: at.splash_percent,
            attacker_type_0x40: 0,
            attacker_z: 0,
            attacker_flag8_bit5: false,
            defender_type_id: dt.id,
            defender_domain: dt.domain,
            defender_type_0x2b8_bit2: false,
            defender_splash_divisor: 1,
            defender_flags_0x68: 0,
            defender_flags_0x6c_bit12: false,
            defender_z: 0,
            defender_facing: b.facing,
            defender_facing_entrench: b.facing,
            defender_overkill_stamp: 0,
            defender_word_0xa4: 0,
            attacker_vf_0xe4: 0,
            current_frame: self.frame as i32,
            game_flag_0x821_bit1: false,
            tile_rocky: false,
            tile_owner: -1,
        };
        // The predicates are inputs, not derivations (`mechanics::DamagePredicates`
        // says so). The arena sets exactly the two "is a live unit" ones and leaves
        // every optional multiplier off, which keeps the chain on its spine.
        let preds = DamagePredicates {
            attacker_vf_0x18: !a.building,
            defender_vf_0x18: !b.building,
            attacker_vf_0x20: true,
            ..DamagePredicates::default()
        };
        let rules = don_sim::systems::target::combat_rules(&self.combat);
        let terms = don_sim::systems::target::unreached_terms(&self.combat, 0);
        let (raw, trace) = damage_traced(&input, &preds, &rules, &terms);
        if let Some(lvl) = flank {
            let lvl = lvl as usize;
            self.flank_hist[lvl.min(2)] += 1;
        }
        let _ = trace;
        let scaled = if a.building {
            scale_damage_build(raw, 0x100, at.ammo_per_att)
        } else {
            scale_damage_unit(raw, 0x100, -1, at.ammo_per_att, dt.uber_size)
        };
        self.shots += 1;
        let Ok(sd) = scaled else { return 0 };
        let now = self.frame;
        let tb = self.ent_mut(target).unwrap();
        let applied = tb.hp.accumulate(sd.whole, sd.sixteenths);
        tb.last_damaged = now;
        self.sync_target_damage(target);
        applied.max(0)
    }

    fn reap(&mut self) {
        for i in 0..self.ents.len() {
            if !self.ents[i].alive {
                continue;
            }
            if self.ents[i].hp.hits_left() > 0 {
                continue;
            }
            let e = self.ents[i].clone();
            assert!(
                self.target_world.remove(target_ref(&e)),
                "dead arena object must unlink from target acquisition"
            );
            if let Some(build) = self.ents[i].build.as_mut() {
                // Target destruction invalidates the addressed object. Builders are not
                // proactively visited; their next do_build observes the invalid slot.
                // The remaining close/disband world transaction is still a declared
                // construction-interruption blocker, so no refund/terrain mutation is
                // invented here and credited counters stay in the record.
                build.flags &= !production::flag::VALID;
            }
            self.close_support_records(e.who, e.object_o);
            self.ents[i].alive = false;
            self.players[e.who as usize].losses += 1;
            for p in 0..self.players.len() {
                if p != e.who as usize {
                    self.players[p].kills += 1;
                }
            }
            // Free any seat the dead entity held or held for others.
            if e.building {
                let released: Vec<usize> = self
                    .ents
                    .iter()
                    .enumerate()
                    .filter_map(|(i, o)| (o.alive && o.assigned_to == e.id).then_some(i))
                    .collect();
                for i in released {
                    let worker_id = self.ents[i].id;
                    let worker_owner = self.ents[i].who;
                    self.retire_exact_gather(worker_owner, worker_id);
                    self.retire_motion_order(i, KillReason::Failed);
                    self.ents[i].assigned_to = EntId::NONE;
                    self.ents[i].job = Job::Idle;
                }
            } else {
                self.retire_exact_gather(e.who, e.id);
                if let Some(b) = self.ent_mut(e.assigned_to) {
                    if matches!(e.job, Job::Gather { .. }) {
                        b.workers = (b.workers - 1).max(0);
                    }
                }
            }
            let n = self
                .types
                .get(e.type_id)
                .map(|t| t.name.clone())
                .unwrap_or_default();
            self.note(e.who, format!("lost {n}"));
        }
    }

    fn check_defeat(&mut self) {
        for pi in 0..self.players.len() {
            if !self.players[pi].alive {
                continue;
            }
            let has_city = self.own_ents(pi).any(|e| e.type_id == self.ids.small_city);
            let has_citizen = self.own_ents(pi).any(|e| e.type_id == self.ids.citizen);
            if !has_city && !has_citizen {
                self.players[pi].alive = false;
                self.players[pi].defeat_frame = Some(self.frame);
                self.note(pi as u8, "defeated".to_string());
            }
        }
    }

    // -----------------------------------------------------------------------
    // Fog
    // -----------------------------------------------------------------------

    fn update_fog(&mut self) {
        let (w, h) = (self.map.w, self.map.h);
        for pi in 0..self.players.len() {
            for v in self.players[pi].visible.iter_mut() {
                *v = false;
            }
        }
        for e in self.ents.iter().filter(|e| e.alive) {
            let pi = e.who as usize;
            if pi >= self.players.len() {
                continue;
            }
            let los = self.types.get(e.type_id).map(|t| t.los).unwrap_or(0);
            let (tx, ty) = e.tile();
            for dy in -los..=los {
                for dx in -los..=los {
                    let (x, y) = (tx + dx, ty + dy);
                    if x < 0 || y < 0 || x >= w || y >= h {
                        continue;
                    }
                    let i = (y * w + x) as usize;
                    self.players[pi].visible[i] = true;
                    self.players[pi].explored[i] = true;
                }
            }
        }
        // Sightings: record what is visible, forget what provably is not there.
        for pi in 0..self.players.len() {
            let mut seen: Vec<Sighting> = Vec::new();
            for e in self.ents.iter().filter(|e| e.alive) {
                if e.who as usize == pi {
                    continue;
                }
                let (tx, ty) = e.tile();
                let i = (ty * w + tx) as usize;
                if self.players[pi].visible.get(i).copied().unwrap_or(false) {
                    seen.push(Sighting {
                        id: e.id,
                        who: e.who,
                        type_id: e.type_id,
                        tx,
                        ty,
                        frame: self.frame,
                        building: e.building,
                    });
                }
            }
            let frame = self.frame;
            let vis = std::mem::take(&mut self.players[pi].visible);
            self.players[pi].memory.retain(|_, s| {
                let i = (s.ty * w + s.tx) as usize;
                // Looking at where it was and not seeing it means it is not there.
                !(vis.get(i).copied().unwrap_or(false) && s.frame < frame)
            });
            self.players[pi].visible = vis;
            for s in seen {
                self.players[pi].memory.insert(s.id, s);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Scoring
    // -----------------------------------------------------------------------

    /// The timeout scoreboard. Stated rather than clever: an elimination is the only
    /// decisive result; everything else is reported as components so a reader can see
    /// *why* one side is ahead instead of trusting one number.
    pub fn score(&self, pi: usize) -> Score {
        let mut s = Score::default();
        s.resources = self.players[pi].gathered.iter().sum();
        for e in self.own_ents(pi) {
            let Some(t) = self.types.get(e.type_id) else {
                continue;
            };
            let value: i64 = t.cost.iter().map(|&c| c as i64).sum();
            if e.building {
                s.buildings += 1;
                s.building_value += value;
                if e.type_id == self.ids.small_city {
                    s.cities += 1;
                }
            } else if t.is_military() {
                s.army += 1;
                s.army_value += value;
            } else {
                s.civilians += 1;
            }
        }
        s.damage_dealt = self.players[pi].damage_dealt;
        s.age = self.age_of(pi) as i64;
        s
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Score {
    pub cities: i64,
    pub buildings: i64,
    pub building_value: i64,
    pub army: i64,
    pub army_value: i64,
    pub civilians: i64,
    pub resources: i64,
    pub damage_dealt: i64,
    pub age: i64,
}

/// The Chebyshev ring of radius `r` around the origin, in a fixed order.
pub fn ring(r: i32) -> Vec<(i32, i32)> {
    let mut v = Vec::new();
    if r == 0 {
        return vec![(0, 0)];
    }
    for x in -r..=r {
        v.push((x, -r));
    }
    for y in -r + 1..=r {
        v.push((r, y));
    }
    for x in (-r..r).rev() {
        v.push((x, r));
    }
    for y in (-r + 1..r).rev() {
        v.push((-r, y));
    }
    v
}

fn unit_type_stats(t: &TypeRow) -> UnitTypeStats {
    UnitTypeStats {
        domain: t.domain,
        guy_spacing: t.guy_spacing,
        x_spacing: t.x_spacing,
        y_spacing: t.y_spacing,
        guy_radius: t.guy_radius,
        new_block_radius: t.new_block_radius,
        turn_speed: t.turn_speed,
        role: t.role,
        squad_size: t.squad_size,
        uber_size: t.uber_size,
        crew_size: t.crew_size,
        base_form: t.base_form,
    }
}

fn collision_row(e: &Ent, t: &TypeRow) -> CollisionUnit {
    let (o, safe, unit_masks, moving, order, searching, path_top_flags) = e
        .motion
        .as_ref()
        .map(|u| {
            (
                u.o as i32,
                u.safe,
                u.unit_masks,
                !u.orders.is_empty(),
                u.orders.front().map_or(0, |o| o.kind as i32),
                u.parked_search,
                u.path.peek().map_or(0, |p| p.flags as u8),
            )
        })
        .unwrap_or((i32::from(e.object_o), 0, 0, false, 0, false, 0));
    CollisionUnit {
        who: e.who as i32,
        o,
        x: e.x,
        y: e.y,
        down: -1,
        down_who: -1,
        domain: t.domain,
        block_radius: t.new_block_radius,
        big_radius: t.big_radius,
        push_size: t.push_size,
        push_circles: t.push_circles,
        angle: e.facing,
        first_guy_angle: e
            .guys
            .guys
            .first()
            .and_then(Option::as_ref)
            .map_or(e.facing, |g| g.angle),
        group: e.motion.as_ref().map_or(-1, |u| u.group),
        collide_o: -1,
        collide_who: -1,
        safe,
        unit_masks,
        on_map: e.alive,
        active: e.alive,
        moving,
        action: order,
        order,
        has_orders: moving,
        searching,
        path_top_flags,
        unit_flags: t.unit_flags,
        unit_flags2: t.unit_flags2,
        attack_value: t.attack,
        spell_id: -1,
        ..CollisionUnit::default()
    }
}

/// The arena host presented to the retail order executor. The actor is omitted from
/// `obstacles`, so the A* and integrator cannot collide it with its own footprint while
/// its mutable `UnitWork` is held outside the entity table.
struct ArenaMoveWorld<'a> {
    map: &'a Map,
    occupied_tiles: Vec<(i32, i32)>,
    collision: RefCell<ArenaCollisionProbe<'a>>,
    frame: i32,
    rng: &'a mut Random,
}

struct ArenaCollisionProbe<'a> {
    world: &'a mut don_sim::systems::map_terrain::World,
    check: &'a mut CollCheck,
    units: &'a mut CollisionUnits,
    me: CollisionUnit,
    /// `MoveOrder::coll_x/coll_y`: written by the side-effecting step detector and
    /// consumed by `resolve_unit_collision`. `OrderRec` does not otherwise carry these
    /// two retail fields, so the host persists them alongside the collision row.
    step_dest: Option<(i32, i32)>,
}

/// Mutable order fields collision reads and writes while `do_move` owns the actor. Keeping
/// this bridge separate from [`ArenaCollisionAdapter`] lets the resolver's path callback
/// borrow the actor and park a real suspended search without aliasing the object table.
#[derive(Default)]
struct ArenaCollisionOrder {
    actor: (i32, i32),
    step_dest: Option<(i32, i32)>,
    detour: Option<(i32, i32)>,
    wait: Option<i32>,
    clear_dest: bool,
    target: Option<(i32, i32)>,
    actor_location: Option<(i32, i32)>,
    actor_angle: Option<i32>,
}

struct ArenaCollisionAdapter<'a> {
    table: &'a mut CollisionUnits,
    order: &'a mut ArenaCollisionOrder,
    map: &'a Map,
    occupied_tiles: &'a [(i32, i32)],
}

impl collision::CollUnits for ArenaCollisionAdapter<'_> {
    fn row(&self, who: i32, o: i32) -> Option<CollisionUnit> {
        collision::CollUnits::row(&*self.table, who, o)
    }

    fn unit_corner(&self, who: i32, o: i32, cx: i32, cy: i32) -> i32 {
        collision::CollUnits::unit_corner(&*self.table, who, o, cx, cy)
    }

    fn find_boat_units(&mut self, query: collision::BoatQuery) -> Vec<(i32, i32)> {
        collision::CollUnits::find_boat_units(&mut *self.table, query)
    }

    fn effective_owner(&self, who: i32) -> i32 {
        collision::CollUnits::effective_owner(&*self.table, who)
    }

    fn diplomacy(&self, who: i32, other: i32) -> i32 {
        collision::CollUnits::diplomacy(&*self.table, who, other)
    }

    fn boat_invalid_loc(&self, _: i32, _: i32, tx: i32, ty: i32) -> bool {
        !self.map.at(tx, ty).passable() || self.occupied_tiles.contains(&(tx, ty))
    }

    fn set_boat_location(&mut self, who: i32, o: i32, x: i32, y: i32) {
        collision::CollUnits::set_boat_location(&mut *self.table, who, o, x, y);
        if (who, o) == self.order.actor {
            self.order.actor_location = Some((x, y));
        }
    }

    fn face_pushed_idle_unit(
        &mut self,
        who: i32,
        o: i32,
        angle: i32,
        pusher_who: i32,
        pusher_o: i32,
    ) {
        collision::CollUnits::face_pushed_idle_unit(
            &mut *self.table,
            who,
            o,
            angle,
            pusher_who,
            pusher_o,
        );
        if (who, o) == self.order.actor {
            self.order.actor_angle = Some(angle);
        }
    }

    fn write(&mut self, who: i32, o: i32, row: &CollisionUnit) {
        collision::CollUnits::write(&mut *self.table, who, o, row)
    }

    fn is_enemy(&self, me: i32, them: i32) -> bool {
        collision::CollUnits::is_enemy(&*self.table, me, them)
    }

    fn order_dest(&self, who: i32, o: i32) -> Option<(i32, i32)> {
        ((who, o) == self.order.actor)
            .then_some(self.order.step_dest)
            .flatten()
    }

    fn set_order_dest(&mut self, who: i32, o: i32, x: i32, y: i32) {
        if (who, o) == self.order.actor {
            self.order.step_dest = Some((x, y));
        }
    }

    fn set_order_detour(&mut self, who: i32, o: i32, x: i32, y: i32) {
        if (who, o) == self.order.actor {
            self.order.detour = Some((x, y));
        }
    }

    fn set_order_wait(&mut self, who: i32, o: i32, ticks: i32) {
        if (who, o) == self.order.actor {
            self.order.wait = Some(ticks);
        }
    }

    fn clear_order_retry(&mut self, who: i32, o: i32) {
        if (who, o) == self.order.actor {
            self.order.clear_dest = true;
        }
    }

    fn order_targets(&self, who: i32, o: i32, target_who: i32, target_o: i32) -> bool {
        (who, o) == self.order.actor && self.order.target == Some((target_who, target_o))
    }

    fn attack_slack(&self, who: i32, o: i32, nx: i32, ny: i32) -> i32 {
        collision::CollUnits::attack_slack(&*self.table, who, o, nx, ny)
    }

    fn repath_budget(&self, who: i32) -> i32 {
        collision::CollUnits::repath_budget(&*self.table, who)
    }

    fn bump_repath_budget(&mut self, who: i32) {
        collision::CollUnits::bump_repath_budget(&mut *self.table, who)
    }

    fn frame(&self) -> i32 {
        collision::CollUnits::frame(&*self.table)
    }
}

/// Read-only collision view for a resolver-triggered local repath. Retail's validity probes
/// are read-only apart from scratch caches, so cloning the object/collision image preserves
/// the queried state while the real table remains mutably borrowed by the resolver.
struct ArenaPathSnapshot<'a> {
    map: &'a Map,
    occupied_tiles: &'a [(i32, i32)],
    world: RefCell<don_sim::systems::map_terrain::World>,
    check: RefCell<CollCheck>,
    units: RefCell<CollisionUnits>,
    me: CollisionUnit,
}

impl UnitWorld for ArenaPathSnapshot<'_> {
    fn tiles_w(&self) -> i32 {
        self.map.w
    }

    fn tiles_h(&self) -> i32 {
        self.map.h
    }

    fn wcells_w(&self) -> i32 {
        ((self.map.w + 3) / 4).max(1)
    }

    fn invalid_loc(&self, tx: i32, ty: i32) -> bool {
        !self.map.at(tx, ty).passable() || self.occupied_tiles.contains(&(tx, ty))
    }

    fn unit_collides(&self, x: i32, y: i32) -> bool {
        collision::unit_collides(
            &mut self.world.borrow_mut(),
            &mut self.check.borrow_mut(),
            &mut *self.units.borrow_mut(),
            &self.me,
            x,
            y,
        )
    }

    fn needs_transport(&self, _: i32, _: i32, _: i32, _: i32) -> i32 {
        0
    }

    fn tregion(&self, tx: i32, ty: i32) -> i32 {
        if self.invalid_loc(tx, ty) {
            -1
        } else {
            0
        }
    }
}

impl UnitWorld for ArenaMoveWorld<'_> {
    fn tiles_w(&self) -> i32 {
        self.map.w
    }

    fn tiles_h(&self) -> i32 {
        self.map.h
    }

    fn wcells_w(&self) -> i32 {
        ((self.map.w + 3) / 4).max(1)
    }

    fn invalid_loc(&self, tile_x: i32, tile_y: i32) -> bool {
        !self.map.at(tile_x, tile_y).passable() || self.occupied_tiles.contains(&(tile_x, tile_y))
    }

    fn unit_collides(&self, x: i32, y: i32) -> bool {
        let mut c = self.collision.borrow_mut();
        let ArenaCollisionProbe {
            world,
            check,
            units,
            me,
            ..
        } = &mut *c;
        collision::unit_collides(*world, *check, *units, me, x, y)
    }

    fn needs_transport(&self, _: i32, _: i32, _: i32, _: i32) -> i32 {
        0
    }

    fn tregion(&self, tile_x: i32, tile_y: i32) -> i32 {
        if self.invalid_loc(tile_x, tile_y) {
            -1
        } else {
            0
        }
    }
}

impl WorkWorld for ArenaMoveWorld<'_> {
    fn frame(&self) -> i32 {
        self.frame
    }

    fn target(&self, _: i32, _: i32) -> Option<TargetState> {
        None
    }

    fn attack(&mut self, _: &UnitWork, _: &OrderRec) -> AttackOutcome {
        AttackOutcome::Impossible
    }

    fn gather(&mut self, _: &UnitWork, _: &OrderRec) -> GatherOutcome {
        GatherOutcome::Exhausted
    }

    fn draw_path_retry_delay(&mut self) -> i32 {
        self.rng.get(0, 0xFFFF) % 3 + 6
    }

    fn patrol_think_bird(
        &mut self,
        _: &mut UnitWork,
        _: &mut don_sim::systems::patrol::AirPatrolOrder,
    ) {
        panic!("arena movement host cannot dispatch air patrol orders")
    }

    fn air_patrol_physics(
        &mut self,
        _: &mut UnitWork,
        _: &mut don_sim::systems::patrol::AirPatrolOrder,
        _: i32,
        _: i32,
    ) -> bool {
        panic!("arena movement host cannot dispatch air patrol orders")
    }

    fn air_patrol_unit_target(
        &mut self,
        _: &UnitWork,
        _: &don_sim::systems::patrol::AirPatrolOrder,
        _: i32,
        _: i32,
        _: order_dispatch::AirPatrolSearch,
    ) -> Option<don_sim::systems::patrol::AirPatrolTarget> {
        // Fail closed at ArenaAirModel/MODEL 6: spawn and ordinary acquisition reject air
        // domains, and the host has no retail air-patrol object search to delegate to.
        None
    }

    fn air_patrol_building_target(
        &mut self,
        _: &UnitWork,
        _: &don_sim::systems::patrol::AirPatrolOrder,
        _: i32,
        _: i32,
    ) -> Option<don_sim::systems::patrol::AirPatrolTarget> {
        // This explicit unsupported result is not a nearest-building substitute. Any air
        // patrol reaching the arena host is already rejected by the panic gates above.
        None
    }

    fn group_patrol_move(
        &mut self,
        _: &mut UnitWork,
        _: don_sim::systems::patrol::GroupMoveRequest,
    ) {
        panic!("arena movement host cannot dispatch group patrol orders")
    }

    fn patrol_actor_is_type(&self, _: &UnitWork, _: i32, _: bool) -> bool {
        panic!("arena movement host cannot dispatch air patrol orders")
    }

    fn patrol_inside_is_scramblable(&self, _: u8, _: i16) -> bool {
        panic!("arena movement host cannot dispatch air patrol orders")
    }

    fn patrol_scramble_inside(&mut self, _: &mut UnitWork, _: i16) {
        panic!("arena movement host cannot dispatch air patrol orders")
    }

    fn move_collision(
        &mut self,
        actor: &mut UnitWork,
        pf: &mut PathFinder,
        event: don_sim::systems::movement::MoveCollisionEvent<'_>,
    ) -> don_sim::systems::movement::MoveCollisionReply {
        use don_sim::systems::movement::{
            MoveCollisionEvent, MoveCollisionProbe, MoveCollisionReply,
        };

        let map = self.map;
        let occupied_tiles = self.occupied_tiles.as_slice();
        match event {
            MoveCollisionEvent::Detect { x, y, probe } => {
                let c = self.collision.get_mut();
                let mut row = c.me;
                let target = actor.orders.front().map(|o| (o.target_who, o.target_o));
                let mut order = ArenaCollisionOrder {
                    actor: (row.who, row.o),
                    step_dest: c.step_dest,
                    target,
                    ..ArenaCollisionOrder::default()
                };
                let mut units = ArenaCollisionAdapter {
                    table: c.units,
                    order: &mut order,
                    map,
                    occupied_tiles,
                };
                let args = match probe {
                    MoveCollisionProbe::MoveStep => collision::DetectArgs::MOVE_STEP,
                    MoveCollisionProbe::Waypoint => collision::DetectArgs::DETOUR_PROBE,
                };
                let detected = collision::detect_unit_collision(
                    c.world, c.check, &mut units, &row, x, y, args,
                );
                if probe == MoveCollisionProbe::MoveStep {
                    detected.apply(&mut units, &mut row, self.frame);
                    collision::CollUnits::write(&mut units, row.who, row.o, &row);
                    actor.safe = row.safe;
                    actor.unit_masks = row.unit_masks;
                    actor.collide_frame = row.collide_frame;
                    c.me = row;
                    c.step_dest = order.step_dest;
                }
                if detected.blocked() {
                    MoveCollisionReply::Hit
                } else {
                    MoveCollisionReply::Clear
                }
            }
            MoveCollisionEvent::Resolve {
                x: _,
                y: _,
                body,
                path,
            } => {
                let rng = &mut *self.rng;
                let c = self.collision.get_mut();
                let mut row = c.me;
                let snapped = (
                    don_sim::systems::movement::ucell_centre(don_sim::systems::movement::ucell_of(
                        row.x,
                    )),
                    don_sim::systems::movement::ucell_centre(don_sim::systems::movement::ucell_of(
                        row.y,
                    )),
                );

                // `resolve_unit_collision` can call the same budgeted `find_upath` wrapper.
                // Its raw suspended return is -1 and retail treats that as success (`!= 0`).
                // A scratch clone provides the exact pre-resolve bitmap/object image while
                // the real object adapter remains exclusively borrowed by the resolver.
                let mut snapshot_units = c.units.clone();
                if let Some(i) = snapshot_units.find(row.who, row.o) {
                    snapshot_units.rows[i].x = snapped.0;
                    snapshot_units.rows[i].y = snapped.1;
                }
                let mut snapshot_me = row;
                snapshot_me.x = snapped.0;
                snapshot_me.y = snapped.1;
                let snapshot = ArenaPathSnapshot {
                    map,
                    occupied_tiles,
                    world: RefCell::new(c.world.clone()),
                    check: RefCell::new(CollCheck::new()),
                    units: RefCell::new(snapshot_units),
                    me: snapshot_me,
                };
                let repath_dest = actor
                    .orders
                    .front()
                    .map_or((body.x, body.y), |o| (o.dest_x, o.dest_y));
                let target = actor.orders.front().map(|o| (o.target_who, o.target_o));
                let mut order = ArenaCollisionOrder {
                    actor: (row.who, row.o),
                    step_dest: c.step_dest,
                    target,
                    ..ArenaCollisionOrder::default()
                };
                let mut units = ArenaCollisionAdapter {
                    table: c.units,
                    order: &mut order,
                    map,
                    occupied_tiles,
                };

                // The repath callback seeds its stack from UnitData's snapped body.
                actor.body.x = snapped.0;
                actor.body.y = snapped.1;
                let invalid_loc = |tx: i32, ty: i32| {
                    !map.at(tx, ty).passable() || occupied_tiles.contains(&(tx, ty))
                };
                let _resolved = collision::resolve_unit_collision(
                    c.world,
                    c.check,
                    &mut units,
                    &mut row,
                    path,
                    rng,
                    &invalid_loc,
                    |repath, quick| {
                        std::mem::swap(&mut actor.path, repath);
                        let outcome = order_dispatch::find_path(
                            pf,
                            &snapshot,
                            actor,
                            repath_dest.0,
                            repath_dest.1,
                            i32::from(quick),
                        );
                        std::mem::swap(&mut actor.path, repath);
                        matches!(
                            outcome,
                            order_dispatch::PathOutcome::Found
                                | order_dispatch::PathOutcome::Suspended
                        )
                    },
                );
                // `Unit::set_new_location` owns the spatial remove/write/add sequence. The
                // arena invokes it immediately after `do_move`, before moving guy stamps;
                // retain the table row's old anchor/link here while persisting resolver state.
                let linked = collision::CollUnits::row(&units, row.who, row.o)
                    .expect("resolving actor remains in collision table");
                let mut stored = row;
                stored.x = linked.x;
                stored.y = linked.y;
                stored.down = linked.down;
                stored.down_who = linked.down_who;
                collision::CollUnits::write(&mut units, row.who, row.o, &stored);
                drop(units);

                if let Some((x, y)) = order.detour {
                    if let Some(o) = actor.orders.front_mut() {
                        o.dest_x = x;
                        o.dest_y = y;
                    }
                }
                if let Some(wait) = order.wait {
                    if let Some(o) = actor.orders.front_mut() {
                        o.pause = wait;
                    }
                }
                if order.clear_dest {
                    if let Some(o) = actor.orders.front_mut() {
                        o.dest = 0;
                    }
                }
                body.x = row.x;
                body.y = row.y;
                if let Some((x, y)) = order.actor_location {
                    body.x = x;
                    body.y = y;
                }
                if let Some(angle) = order.actor_angle {
                    body.angle = angle;
                }
                actor.body = *body;
                actor.safe = row.safe;
                actor.unit_masks = row.unit_masks;
                actor.collide_frame = row.collide_frame;
                c.me = row;
                c.step_dest = order.step_dest;
                MoveCollisionReply::Handled
            }
        }
    }
}

#[cfg(test)]
mod supply_attrition_integration {
    use super::*;
    use crate::arena::match_run::{load_world, MatchConfig};

    #[test]
    fn live_unit_band_applies_reload_healing_attrition_and_source_lifetime() {
        let Ok(mut world) = load_world(&MatchConfig::default()) else {
            return;
        };
        let supply_type = world
            .types
            .rows
            .values()
            .find(|ty| ty.kind_unit && ty.unit_flags2 & 0x40 != 0)
            .expect("live tables contain a UnitData::is_supply type")
            .id;
        let ty = world.map.w / 2;
        let target = world.spawn(0, world.ids.citizen, ty - 7, ty, true);
        let siege = world.spawn(0, 0x109, ty - 7, ty, true);
        let source = world.spawn(0, supply_type, ty + 7, ty, true);
        let enemy = world.spawn(1, world.ids.citizen, ty - 3, ty, true);
        let target_index = target.index().expect("spawn returned a dense Arena handle");
        let siege_index = siege.index().expect("spawn returned a dense Arena handle");
        let source_index = source.index().expect("spawn returned a dense Arena handle");
        let enemy_index = enemy.index().expect("spawn returned a dense Arena handle");
        world.ents[enemy_index].hp.myhits = 10_000;
        let target_o = world.ents[target_index].object_o;
        let siege_o = world.ents[siege_index].object_o;
        let source_o = world.ents[source_index].object_o;

        // UnitData::recharge consults its narrower in_supply predicate after the volley:
        // neutral territory plus this registry hit keeps the live Catapult at base delay.
        let base_recharge = world.types.get(0x109).unwrap().recharge;
        world.ents[siege_index].job = Job::Attack { target: enemy };
        world.tick_job(siege_index);
        assert_eq!(
            world.ents[siege_index].cycle.recharging,
            base_recharge as u8
        );

        world.ents[source_index].x += RANGE_UNITS_PER_TILE;
        world.ents[siege_index].cycle.recharging = 0;
        world.tick_job(siege_index);
        assert_eq!(
            world.ents[siege_index].cycle.recharging,
            (base_recharge * 3 / 2) as u8,
            "the real fire call site must apply out-of-supply siege delay"
        );

        // French supply healing has a 20-frame phase. Return the source to the inclusive
        // radius and prove World::step calls repair_damage before the attrition tail.
        world.ents[source_index].x -= RANGE_UNITS_PER_TILE;
        world.ents[siege_index].job = Job::Idle;
        world.ents[siege_index].hp.damage = 2;
        world.players[0].tribe = 0x0A;
        let heal_due = (-i64::from(world.ents[siege_index].object_o)).rem_euclid(20);
        world.frame = heal_due;
        world.step();
        assert_eq!(world.ents[siege_index].hp.damage, 1);

        // Unit::process calls process_attrition on its independent 32-frame stagger. The
        // friendly return closes after the universal mask/period reset; neutral territory
        // exposes the typed non-friendly authority boundary but retains that exact prefix.
        let (siege_tx, siege_ty) = world.ents[siege_index].tile();
        let siege_wx = siege_tx.div_euclid(4);
        let siege_wy = siege_ty.div_euclid(4);
        world.collision_world.wdata_mut(siege_wx, siege_wy).who = 0;
        {
            let siege_ent = &mut world.ents[siege_index];
            siege_ent.attrition_period = 23;
            let motion = siege_ent.motion.as_mut().expect("Catapult owns UnitWork");
            motion.unit_masks |= 0x40_0080;
            motion.unit_masks2 |= RESUPPLIED_THIS_TICK;
        }
        world.frame = (-i64::from(siege_o)).rem_euclid(32);
        assert!(matches!(
            world.tick_attrition_recompute(siege_index),
            AttritionRecomputeTransaction::FriendlyTerritoryReset {
                period_before: 23,
                ..
            }
        ));
        assert_eq!(world.ents[siege_index].attrition_period, 0);
        let motion = world.ents[siege_index].motion.as_ref().unwrap();
        assert_eq!(motion.unit_masks & 0x40_0080, 0);
        assert_eq!(motion.unit_masks2 & RESUPPLIED_THIS_TICK, 0);

        world.collision_world.wdata_mut(siege_wx, siege_wy).who = -1;
        world.ents[siege_index].attrition_period = 29;
        world.ents[siege_index].motion.as_mut().unwrap().unit_masks2 |= RESUPPLIED_THIS_TICK;
        world.frame += 32;
        assert!(matches!(
            world.tick_attrition_recompute(siege_index),
            AttritionRecomputeTransaction::BlockedNonFriendlyTerritory {
                territory_owner: -1,
                period_before: 29,
                ..
            }
        ));
        assert_eq!(world.ents[siege_index].attrition_period, 0);
        assert_eq!(
            world.ents[siege_index].motion.as_ref().unwrap().unit_masks2 & RESUPPLIED_THIS_TICK,
            0
        );

        world.ents[target_index].attrition_period = 47;
        world.ents[target_index]
            .motion
            .as_mut()
            .expect("Citizen owns UnitWork")
            .unit_masks2 &= !RESUPPLIED_THIS_TICK;

        let due = (-i64::from(target_o)).rem_euclid(47);
        world.frame = due;
        world.step();

        assert!(world.last_object_process_order.contains(&target));
        assert_ne!(
            world.ents[target_index]
                .motion
                .as_ref()
                .unwrap()
                .unit_masks2
                & RESUPPLIED_THIS_TICK,
            0,
            "the live unit-band transaction must write the walked resupply mask"
        );
        assert_eq!(world.ents[target_index].hp.damage, 0);
        assert!(world.supply_records[0].iter().any(|record| {
            record.supply_flags & SUPPORT_REGISTRY_ACTIVE != 0 && record.o == source_o
        }));

        // One tile beyond the exact fourteen-tile base radius: the next due frame must
        // traverse the same live record, exhaust supply, and mutate the Citizen object.
        world.ents[source_index].x += RANGE_UNITS_PER_TILE;
        world.ents[target_index]
            .motion
            .as_mut()
            .unwrap()
            .unit_masks2 &= !RESUPPLIED_THIS_TICK;
        world.frame = due + 47;
        world.step();

        assert_eq!(world.ents[target_index].hp.damage, 1);
        assert_eq!(
            world.ents[target_index]
                .motion
                .as_ref()
                .unwrap()
                .unit_masks2
                & RESUPPLIED_THIS_TICK,
            0
        );
        assert!(world.ents[target_index].alive);

        let source_hits = world.types.get(supply_type).unwrap().hits;
        world.ents[source_index].hp.damage = source_hits;
        world.reap();
        assert!(!world.supply_records[0].iter().any(|record| {
            record.supply_flags & SUPPORT_REGISTRY_ACTIVE != 0 && record.o == source_o
        }));
    }
}

#[cfg(test)]
mod gather_integration {
    use super::*;
    use crate::arena::match_run::{load_world, MatchConfig};
    use don_sim::systems::collision::CollUnits;

    #[test]
    fn generated_gather_command_hits_typed_exact_refusal_without_partial_attachment() {
        let Ok(mut w) = load_world(&MatchConfig::default()) else {
            return;
        };
        let farm = w
            .own_ents(0)
            .find(|ent| ent.type_id == w.ids.farm)
            .expect("Small Town start has a Farm")
            .id;
        let worker = w
            .own_ents(0)
            .find(|ent| ent.type_id == w.ids.citizen)
            .expect("Small Town start has a Citizen")
            .id;
        let worker_o = i32::from(w.ent(worker).unwrap().object_o);
        let before_runtime = w.gather_runtime.clone();
        let before_collision = w
            .collision_units
            .row(0, worker_o)
            .expect("Citizen has an on-map collision row");

        assert_eq!(
            w.submit(
                0,
                Cmd::Gather {
                    unit: worker,
                    target: farm,
                },
            ),
            OrderResult::Ok(1)
        );
        assert_eq!(
            w.gather_runtime, before_runtime,
            "MODEL 3 gameplay cannot partially enter the exact gather chain"
        );
        assert!(matches!(w.ent(worker).unwrap().job, Job::Gather { target } if target == farm));
        assert_eq!(
            w.collision_units.row(0, worker_o),
            Some(before_collision),
            "ordinary Gather assignment never seats or teleports the worker"
        );

        assert_eq!(w.submit(0, Cmd::Halt { unit: worker }), OrderResult::Ok(1));
        assert_eq!(w.collision_units.row(0, worker_o), Some(before_collision));
    }
}

#[cfg(test)]
mod target_integration {
    use super::*;
    use crate::arena::match_run::{load_world, MatchConfig};

    fn world() -> Option<World> {
        load_world(&MatchConfig::default()).ok()
    }

    fn minuteman(w: &World) -> i32 {
        w.types
            .rows
            .values()
            .find(|t| t.name == "Minuteman")
            .expect("live tables contain Minuteman")
            .id
    }

    fn due_frame(object_o: i16) -> i64 {
        (32 - (i64::from(object_o) & 31)) & 31
    }

    fn remember(w: &mut World, observer: usize, id: EntId, tx: i32, ty: i32) {
        let e = w.ent(id).expect("remembered target is live");
        let sighting = Sighting {
            id,
            who: e.who,
            type_id: e.type_id,
            tx,
            ty,
            frame: w.frame,
            building: e.building,
        };
        w.players[observer].memory.insert(id, sighting);
    }

    fn relocate_target_only(w: &mut World, id: EntId, tx: i32, ty: i32) {
        let i = id.index().expect("live target id");
        let x = tx * RANGE_UNITS_PER_TILE + HALF;
        let y = ty * RANGE_UNITS_PER_TILE + HALF;
        assert!(w.target_world.relocate(target_ref(&w.ents[i]), x, y));
        w.ents[i].x = x;
        w.ents[i].y = y;
    }

    #[test]
    fn remembered_visibility_admits_the_live_retail_object_not_its_sighting_payload() {
        let Some(mut w) = world() else { return };
        let ty = minuteman(&w);
        let (cx, cy) = (w.map.w / 2, w.map.h / 2);
        for y in cy - 2..=cy + 20 {
            for x in cx - 2..=cx + 20 {
                w.map.test_set(x, y, Terrain::Grass);
            }
        }
        let attacker = w.spawn(0, ty, cx, cy, true);
        let target = w.spawn(1, w.ids.citizen, cx + 3, cy, true);
        let ai = attacker.index().expect("spawned attacker");
        w.players[0].visible.fill(false);
        w.players[0].memory.clear();
        w.frame = due_frame(w.ents[ai].object_o);

        w.auto_acquire(ai);
        assert_eq!(
            w.ents[ai].job,
            Job::Idle,
            "unseen live objects are not exposed"
        );

        // Retail's object-visible bit remembers this identity. Move the live object well
        // outside response range while retaining the old near sighting: current WData
        // coordinates must win over the payload.
        remember(&mut w, 0, target, cx + 3, cy);
        relocate_target_only(&mut w, target, cx + 18, cy);
        w.auto_acquire(ai);
        assert_eq!(w.ents[ai].job, Job::Idle);

        // Conversely a stale far payload must not hide the live object after it returns.
        w.players[0].memory.get_mut(&target).unwrap().tx = cx + 18;
        relocate_target_only(&mut w, target, cx + 3, cy);
        w.auto_acquire(ai);
        assert_eq!(w.ents[ai].job, Job::Attack { target });
    }

    #[test]
    fn mountain_ridges_do_not_become_a_passability_component_tregion_proxy() {
        let Some(mut w) = world() else { return };
        let ty = minuteman(&w);
        let (cx, cy) = (w.map.w / 2, w.map.h / 2);
        for y in cy - 8..=cy + 8 {
            w.map.test_set(cx + 2, y, Terrain::Mountain);
        }
        w.map.test_set(cx, cy, Terrain::Grass);
        w.map.test_set(cx + 4, cy, Terrain::Grass);
        let attacker = w.spawn(0, ty, cx, cy, true);
        let target = w.spawn(1, w.ids.citizen, cx + 4, cy, true);
        let ai = attacker.index().expect("spawned attacker");
        assert_eq!(
            w.collision_world.get_tregion(cx, cy),
            w.collision_world.get_tregion(cx + 4, cy),
            "retail Regions groups the no-water WData class across tile ridges"
        );
        w.players[0].visible.fill(false);
        w.players[0].memory.clear();
        remember(&mut w, 0, target, cx + 4, cy);
        w.frame = due_frame(w.ents[ai].object_o);

        w.auto_acquire(ai);

        assert_eq!(w.ents[ai].job, Job::Attack { target });
    }

    #[test]
    fn retail_priority_spreads_targets_instead_of_choosing_the_nearest_hostile() {
        let Some(mut w) = world() else { return };
        let ty = minuteman(&w);
        let (cx, cy) = (w.map.w / 2, w.map.h / 2);
        for x in cx..=cx + 5 {
            w.map.test_set(x, cy, Terrain::Grass);
        }
        let attacker = w.spawn(0, ty, cx, cy, true);
        let near = w.spawn(1, w.ids.citizen, cx + 2, cy, true);
        let far = w.spawn(1, w.ids.citizen, cx + 4, cy, true);
        let ai = attacker.index().expect("spawned attacker");
        w.players[0].visible.fill(false);
        w.players[0].memory.clear();
        remember(&mut w, 0, near, cx + 2, cy);
        remember(&mut w, 0, far, cx + 4, cy);
        w.target_world
            .row_mut(target_ref(w.ent(near).unwrap()))
            .unwrap()
            .targeted = target::TARGETED_MAX;
        w.frame = due_frame(w.ents[ai].object_o);

        w.auto_acquire(ai);

        assert_eq!(
            w.ents[ai].job,
            Job::Attack { target: far },
            "the exact target-crowding term outweighs nearest distance"
        );
    }

    #[test]
    fn actual_instance_stance_and_object_slot_phase_drive_the_response_radius() {
        let Some(mut w) = world() else { return };
        let ty = minuteman(&w);
        let (cx, cy) = (w.map.w / 2, w.map.h / 2);
        for x in cx..=cx + 12 {
            w.map.test_set(x, cy, Terrain::Grass);
        }
        let attacker = w.spawn(0, ty, cx, cy, true);
        let target = w.spawn(1, w.ids.citizen, cx + 10, cy, true);
        let ai = attacker.index().expect("spawned attacker");
        w.players[0].visible.fill(false);
        w.players[0].memory.clear();
        remember(&mut w, 0, target, cx + 10, cy);

        let due = due_frame(w.ents[ai].object_o);
        w.frame = due + 1;
        w.auto_acquire(ai);
        assert_eq!(w.ents[ai].job, Job::Idle, "off-phase think is deferred");

        w.frame = due;
        w.ents[ai].stance = 1;
        w.auto_acquire(ai);
        assert_eq!(
            w.ents[ai].job,
            Job::Idle,
            "defensive stance uses the measured shorter response radius"
        );

        w.ents[ai].stance = 0;
        w.auto_acquire(ai);
        assert_eq!(w.ents[ai].job, Job::Attack { target });
    }

    #[test]
    fn cloak_capable_targets_fail_closed_without_the_retail_detector_plane() {
        let Some(mut w) = world() else { return };
        let ty = minuteman(&w);
        let (cx, cy) = (w.map.w / 2, w.map.h / 2);
        for x in cx..=cx + 4 {
            w.map.test_set(x, cy, Terrain::Grass);
        }
        let attacker = w.spawn(0, ty, cx, cy, true);
        let target = w.spawn(1, w.ids.citizen, cx + 3, cy, true);
        let ai = attacker.index().expect("spawned attacker");
        w.players[0].visible.fill(false);
        w.players[0].memory.clear();
        remember(&mut w, 0, target, cx + 3, cy);
        w.frame = due_frame(w.ents[ai].object_o);
        w.types.rows.get_mut(&w.ids.citizen).unwrap().unit_flags |= CLOAK_WHILE_IDLE_TYPE_FLAG;

        w.auto_acquire(ai);

        assert_eq!(
            w.ents[ai].job,
            Job::Idle,
            "memory must not bypass UnitData::is_seen's detector gate"
        );
    }

    #[test]
    fn target_spatial_lifecycle_updates_damage_and_unlinks_death() {
        let Some(mut w) = world() else { return };
        let (cx, cy) = (w.map.w / 2, w.map.h / 2);
        w.map.test_set(cx, cy, Terrain::Grass);
        let target = w.spawn(1, w.ids.citizen, cx, cy, true);
        let i = target.index().expect("spawned target");
        let r = target_ref(&w.ents[i]);
        assert!(w.target_world.row(r).unwrap().is_alive());

        w.ents[i].hp.damage = 7;
        w.sync_target_damage(target);
        assert_eq!(w.target_world.row(r).unwrap().damage, 7);

        w.ents[i].hp.damage = w.ents[i].hp.myhits;
        w.sync_target_damage(target);
        w.reap();
        assert!(!w.ents[i].alive);
        assert!(!w.target_world.row(r).unwrap().is_alive());
    }

    #[test]
    fn vanished_attack_target_retires_the_chase_and_idle_work_counts_safe_to_zero() {
        let Some(mut w) = world() else { return };
        let (cx, cy) = (w.map.w / 2, w.map.h / 2);
        w.map.test_set(cx, cy, Terrain::Grass);
        w.map.test_set(cx + 1, cy, Terrain::Grass);
        let attacker = w.spawn(0, w.ids.citizen, cx, cy, true);
        let target = w.spawn(1, w.ids.citizen, cx + 1, cy, true);
        let ai = attacker.index().expect("spawned attacker");
        let ti = target.index().expect("spawned target");
        let target_pos = (w.ents[ti].x, w.ents[ti].y);
        {
            let motion = w.ents[ai].motion.as_mut().expect("citizen motion");
            motion
                .orders
                .replace(OrderRec::move_to(target_pos.0, target_pos.1, UCELL));
            motion.safe = 6;
            motion.unit_masks |= order_dispatch::masks::PATH_EXHAUSTED;
        }
        w.ents[ai].job = Job::Attack { target };

        w.ents[ti].hp.damage = w.ents[ti].hp.myhits;
        w.reap();
        w.tick_job(ai);
        assert_eq!(w.ents[ai].job, Job::Idle);
        let motion = w.ents[ai].motion.as_ref().unwrap();
        assert!(
            motion.orders.is_empty(),
            "the dead target retires its chase"
        );
        assert_eq!(motion.safe, 6, "kill_current_order does not invent a reset");

        // Keep this test on the order-lifecycle seam; there is no replacement candidate.
        w.types.rows.get_mut(&w.ids.citizen).unwrap().attack = 0;
        for _ in 0..6 {
            w.frame += 1;
            w.tick_job(ai);
        }
        let motion = w.ents[ai].motion.as_ref().unwrap();
        assert_eq!(motion.safe, 0);
        assert!(motion.orders.is_empty());
        let ci = w
            .collision_units
            .find(0, i32::from(w.ents[ai].object_o))
            .unwrap();
        assert_eq!(w.collision_units.rows[ci].safe, 0);
    }

    #[test]
    fn dead_work_site_retires_every_assigned_workers_motion_before_idle() {
        let Some(mut w) = world() else { return };
        let (cx, cy) = (w.map.w / 2, w.map.h / 2);
        w.map.test_set(cx, cy, Terrain::Grass);
        w.map.test_set(cx + 1, cy, Terrain::Grass);
        let site = w.spawn(0, w.ids.farm, cx, cy, true);
        let worker = w.spawn(0, w.ids.citizen, cx + 1, cy, true);
        let si = site.index().expect("spawned site");
        let wi = worker.index().expect("spawned worker");
        let site_pos = (w.ents[si].x, w.ents[si].y);
        w.ents[wi].assigned_to = site;
        w.ents[wi].job = Job::Work { target: site };
        {
            let motion = w.ents[wi].motion.as_mut().expect("citizen motion");
            motion
                .orders
                .replace(OrderRec::move_to(site_pos.0, site_pos.1, UCELL));
            motion.safe = 2;
        }

        w.ents[si].hp.damage = w.ents[si].hp.myhits;
        w.reap();

        assert_eq!(w.ents[wi].job, Job::Idle);
        assert!(w.ents[wi].assigned_to.is_none());
        assert!(w.ents[wi].motion.as_ref().unwrap().orders.is_empty());
        w.types.rows.get_mut(&w.ids.citizen).unwrap().attack = 0;
        for _ in 0..2 {
            w.frame += 1;
            w.tick_job(wi);
        }
        assert_eq!(w.ents[wi].motion.as_ref().unwrap().safe, 0);
    }
}

#[cfg(test)]
mod combat_integration {
    use super::*;
    use crate::arena::match_run::{load_world, MatchConfig};
    use don_sim::systems::groups_guys::GUY_FLAG_TURRETS;

    fn world() -> Option<World> {
        load_world(&MatchConfig::default()).ok()
    }

    fn minuteman(w: &World) -> i32 {
        w.types
            .rows
            .values()
            .find(|t| t.name == "Minuteman")
            .expect("live tables contain Minuteman")
            .id
    }

    fn cardinal_pair(w: &mut World, dx: i32, dy: i32) -> (usize, EntId, i32) {
        let ty = minuteman(w);
        let cx = w.map.w / 2;
        let cy = w.map.h / 2;
        w.map.test_set(cx, cy, Terrain::Grass);
        w.map.test_set(cx + dx, cy + dy, Terrain::Grass);
        let defender = w.spawn(1, ty, cx, cy, true);
        let attacker = w.spawn(0, ty, cx + dx, cy + dy, true);
        let ai = attacker.index().expect("spawned attacker");
        w.ent_mut(defender).unwrap().facing = 0;
        (ai, defender, ty)
    }

    #[test]
    fn retail_cardinal_geometry_drives_unit_guy_and_flank_state() {
        // Defender faces north. South is rear/tier 1, east is broadside/tier 2, north is
        // front/tier 0. Each case is a fresh ready volley.
        for (dx, dy, tier) in [(0, 1, 1usize), (1, 0, 2usize), (0, -1, 0usize)] {
            let Some(mut w) = world() else { return };
            let (ai, defender, ty) = cardinal_pair(&mut w, dx, dy);
            let before_hist = w.flank_hist;
            let before_shots = w.shots;
            let a0 = w.ents[ai].clone();
            let d0 = w.ent(defender).unwrap().clone();
            let expected = don_sim::systems::target::attack_dir(a0.x, a0.y, d0.x, d0.y);

            w.do_attack(ai, defender);

            assert_eq!(w.shots, before_shots + 1);
            for n in 0..3 {
                assert_eq!(
                    w.flank_hist[n],
                    before_hist[n] + u64::from(n == tier),
                    "offset ({dx},{dy}) tier {tier}"
                );
            }
            assert_eq!(w.ents[ai].facing, expected);
            assert_eq!(w.ents[ai].motion.as_ref().unwrap().body.angle, expected);
            assert_eq!(
                w.ents[ai].guys.guys[0].as_ref().unwrap().des_angle,
                expected
            );
            assert_eq!(
                w.ents[ai].cycle.recharging,
                w.types.get(ty).unwrap().recharge as u8
            );
        }
    }

    #[test]
    fn recharge_is_written_once_after_the_volley_and_ticks_as_a_byte() {
        let Some(mut w) = world() else { return };
        let (ai, defender, ty) = cardinal_pair(&mut w, 0, 1);
        let recharge = w.types.get(ty).unwrap().recharge as u8;
        assert!(recharge > 1);

        w.do_attack(ai, defender);
        assert_eq!(w.shots, 1);
        w.do_attack(ai, defender);
        assert_eq!(w.shots, 1, "no second volley before Unit::inc_time ticks");
        for _ in 0..recharge - 1 {
            w.tick_cycle(ai);
        }
        w.do_attack(ai, defender);
        assert_eq!(w.shots, 1, "one cooldown frame remains");
        w.tick_cycle(ai);
        w.do_attack(ai, defender);
        assert_eq!(w.shots, 2);
    }

    #[test]
    fn unresolved_graphics_turret_cannot_be_laundered_into_body_aim() {
        let Some(mut w) = world() else { return };
        let (ai, defender, _) = cardinal_pair(&mut w, 1, 0);
        let old_facing = w.ents[ai].facing;
        w.ents[ai].guys.guys[0].as_mut().unwrap().guy_flags |= GUY_FLAG_TURRETS;

        w.do_attack(ai, defender);

        assert_eq!(w.shots, 0);
        assert_eq!(w.ents[ai].cycle.recharging, 0);
        assert_eq!(w.ents[ai].facing, old_facing);
    }
}

#[cfg(test)]
mod movement_integration {
    use super::*;
    use crate::arena::match_run::{load_world, MatchConfig};

    struct OpenMoveWorld {
        w: i32,
        h: i32,
    }

    impl UnitWorld for OpenMoveWorld {
        fn tiles_w(&self) -> i32 {
            self.w
        }
        fn tiles_h(&self) -> i32 {
            self.h
        }
        fn wcells_w(&self) -> i32 {
            ((self.w + 3) / 4).max(1)
        }
        fn invalid_loc(&self, _: i32, _: i32) -> bool {
            false
        }
        fn unit_collides(&self, _: i32, _: i32) -> bool {
            false
        }
        fn needs_transport(&self, _: i32, _: i32, _: i32, _: i32) -> i32 {
            0
        }
        fn tregion(&self, _: i32, _: i32) -> i32 {
            0
        }
    }

    /// Run the exact integrator on copies to learn the next translated candidate without
    /// mutating the real arena. `None` means this frame only turns or stays within the
    /// actor's current collision cell.
    fn next_candidate(w: &World, i: usize) -> Option<(i32, i32)> {
        let u = w.ents[i].motion.as_ref()?;
        let target = u
            .path
            .peek()
            .map(|p| (p.to_x, p.to_y))
            .or_else(|| u.orders.front().map(|o| (o.x, o.y)))?;
        if u.path.peek().is_some_and(|p| p.flags & 8 != 0) {
            return None;
        }
        let speed = u.myspeed.max(1) as i32;
        let mut env = u.guy_env;
        env.unit_speed = speed;
        env.unit_mask_turn_scale2 =
            u.unit_masks & don_sim::systems::groups_guys::UNIT_MASK_TURN_SCALE2 != 0;
        let turn_rate = u.lead_guy.turn_speed(&env, 0) as i32;
        let mut profile = don_sim::systems::movement::MoveTurnProfile {
            unit_flags: if u.type_moves_while_turning {
                don_sim::systems::movement::UNIT_FLAG_MOVE_WHILE_TURNING
            } else {
                0
            },
            type_turn_speed: u.guy_env.ut.turn_speed as u32,
            domain: u.guy_env.ut.domain,
            special_wide_turner: u.type_special_wide_turner,
            speed_half_latch: u.unit_masks & order_dispatch::masks::HALF_SPEED_ON_TURN != 0,
        };
        let mut body = u.body;
        let mut path = u.path.clone();
        let mut open = OpenMoveWorld {
            w: w.map.w,
            h: w.map.h,
        };
        let _ = don_sim::systems::movement::move_step_profile(
            &mut open,
            &mut body,
            &mut path,
            target,
            speed,
            turn_rate,
            &mut profile,
        );
        let old_cell = (
            don_sim::systems::movement::ucell_of(u.body.x),
            don_sim::systems::movement::ucell_of(u.body.y),
        );
        let new_cell = (
            don_sim::systems::movement::ucell_of(body.x),
            don_sim::systems::movement::ucell_of(body.y),
        );
        ((body.x, body.y) != (u.body.x, u.body.y) && new_cell != old_cell)
            .then_some((body.x, body.y))
    }

    fn world(seed: u32) -> Option<World> {
        let mut cfg = MatchConfig::default();
        cfg.map.seed = seed;
        load_world(&cfg).ok()
    }

    fn scout_index(w: &World) -> usize {
        w.ents
            .iter()
            .rposition(|e| e.who == 0 && e.type_id == w.ids.citizen)
            .expect("seed start has three citizens")
    }

    #[test]
    fn retail_pathfinder_walks_around_a_ridge_across_map_seeds() {
        for k in 0..8u32 {
            let seed = 0x5EED_0001u32.wrapping_add(k.wrapping_mul(0x9E37_79B9));
            let Some(mut w) = world(seed) else { return };
            let i = scout_index(&w);
            let (sx, sy) = w.ents[i].tile();
            let ridge_x = sx + 5;
            for y in sy - 7..=sy + 7 {
                w.map.test_set(ridge_x, y, Terrain::Mountain);
            }
            // One opening beyond the north end forces a real detour; the destination is
            // directly east, so a straight/greedy mover cannot satisfy this test.
            w.map.test_set(ridge_x, sy - 8, Terrain::Grass);
            let goal = (sx + 11, sy);
            w.map.test_set(goal.0, goal.1, Terrain::Grass);
            let target = (
                goal.0 * RANGE_UNITS_PER_TILE + HALF,
                goal.1 * RANGE_UNITS_PER_TILE + HALF,
            );
            let mut outcome = MoveProgress::Working;
            for _ in 0..2_000 {
                outcome = w.step_toward(i, target.0, target.1, 0);
                w.frame += 1;
                if outcome != MoveProgress::Working {
                    break;
                }
            }
            assert_eq!(outcome, MoveProgress::Arrived, "seed {seed:#x}");
            assert!(w.ents[i].tile().0 > ridge_x, "seed {seed:#x}");
            let row = w
                .target_world
                .row(target_ref(&w.ents[i]))
                .expect("moving unit remains in target object table");
            assert_eq!((row.x, row.y), (w.ents[i].x, w.ents[i].y));
        }
    }

    #[test]
    fn unreachable_move_retires_and_consumes_exactly_one_retry_draw_across_seeds() {
        for k in 0..8u32 {
            let seed = 0x5EED_0001u32.wrapping_add(k.wrapping_mul(0x9E37_79B9));
            let Some(mut w) = world(seed) else { return };
            let i = scout_index(&w);
            let (sx, sy) = w.ents[i].tile();
            let wall_x = sx + 5;
            for y in 0..w.map.h {
                w.map.test_set(wall_x, y, Terrain::Mountain);
            }
            let rng_before = w.game_random.state();
            let draws_before = w.movement_coverage.path_retry_draws;
            let target = (
                (sx + 10) * RANGE_UNITS_PER_TILE + HALF,
                sy * RANGE_UNITS_PER_TILE + HALF,
            );
            let mut outcome = MoveProgress::Working;
            for _ in 0..128 {
                outcome = w.step_toward(i, target.0, target.1, 0);
                w.frame += 1;
                if outcome != MoveProgress::Working {
                    break;
                }
            }
            assert_eq!(outcome, MoveProgress::Failed, "seed {seed:#x}");
            assert_eq!(w.movement_coverage.path_retry_draws, draws_before + 1);
            assert_ne!(w.game_random.state(), rng_before);
            assert!(w.ents[i]
                .motion
                .as_ref()
                .expect("citizen motion")
                .orders
                .is_empty());
        }
    }

    #[test]
    fn retail_collision_resolver_finishes_the_original_order_after_a_new_blocker() {
        for k in 0..8u32 {
            let seed = 0xC011_1DE0u32.wrapping_add(k.wrapping_mul(0x9E37_79B9));
            let Some(mut w) = world(seed) else { return };
            let cy = w.map.h / 2;
            let start = (w.map.w / 2 - 10, cy);
            let goal = (w.map.w / 2 + 10, cy);
            for y in cy - 3..=cy + 3 {
                for x in start.0 - 2..=goal.0 + 2 {
                    w.map.test_set(x, y, Terrain::Grass);
                }
            }
            let actor = w.spawn(0, w.ids.citizen, start.0, start.1, true);
            let i = actor.index().expect("spawned actor");
            let target = (
                goal.0 * RANGE_UNITS_PER_TILE + HALF,
                goal.1 * RANGE_UNITS_PER_TILE + HALF,
            );

            // Build the route first, then materialise a body on its current leading
            // waypoint. This forces the move-step Detect -> Resolve arm rather than merely
            // letting the initial A* bitmap probe route around a pre-existing unit.
            let mut candidate = None;
            for _ in 0..64 {
                assert_eq!(
                    w.step_toward(i, target.0, target.1, 0),
                    MoveProgress::Working
                );
                w.frame += 1;
                if let Some(next) = next_candidate(&w, i) {
                    candidate = Some(next);
                    break;
                }
            }
            let candidate = candidate.expect("long move exposes a translated candidate");
            let blocker_tile = (
                don_sim::systems::movement::tile_of(candidate.0),
                don_sim::systems::movement::tile_of(candidate.1),
            );
            let blocker = w.spawn(0, w.ids.citizen, blocker_tile.0, blocker_tile.1, true);
            assert!(!blocker.is_none());
            let blocker_i = blocker.index().expect("spawned blocker");
            let blocker_o = w.ents[blocker_i].object_o;
            let actor_o = w.ents[i].object_o;
            let old = (w.ents[blocker_i].x, w.ents[blocker_i].y);
            let exact = candidate;
            collision::guy_set_new_location(&mut w.collision_world, old, exact, 0, 0, 1, 1);
            w.ents[blocker_i].x = exact.0;
            w.ents[blocker_i].y = exact.1;
            if let Some(g) = w.ents[blocker_i]
                .guys
                .guys
                .first_mut()
                .and_then(Option::as_mut)
            {
                g.x = exact.0;
                g.y = exact.1;
            }
            if let Some(u) = w.ents[blocker_i].motion.as_mut() {
                u.body.x = exact.0;
                u.body.y = exact.1;
                u.lead_guy.x = exact.0;
                u.lead_guy.y = exact.1;
            }
            let blocker_row_i = w
                .collision_units
                .find(0, i32::from(blocker_o))
                .expect("blocker collision row");
            w.collision_units.rows[blocker_row_i].x = exact.0;
            w.collision_units.rows[blocker_row_i].y = exact.1;
            for guy in w
                .collision_units
                .guys
                .iter_mut()
                .filter(|g| g.who == 0 && g.o == i32::from(blocker_o))
            {
                guy.body.x = exact.0;
                guy.body.y = exact.1;
            }
            let actor_row = w.collision_units.rows[w
                .collision_units
                .find(0, i32::from(actor_o))
                .expect("actor collision row")];
            assert!(collision::unit_collides(
                &mut w.collision_world,
                &mut w.collision_check,
                &mut w.collision_units,
                &actor_row,
                candidate.0,
                candidate.1,
            ));

            let mut outcome = MoveProgress::Working;
            let mut detected_blocker = false;
            for _ in 0..2_000 {
                outcome = w.step_toward(i, target.0, target.1, 0);
                w.frame += 1;
                detected_blocker |= w
                    .collision_units
                    .find(0, i32::from(actor_o))
                    .is_some_and(|ci| w.collision_units.rows[ci].collide_o == blocker_o);
                if outcome != MoveProgress::Working {
                    break;
                }
            }
            assert_eq!(outcome, MoveProgress::Arrived, "seed {seed:#x}");
            assert!(detected_blocker, "seed {seed:#x} never entered Detect::Hit");
            assert!(
                don_sim::systems::movement::vector_dist(
                    w.ents[i].x - target.0,
                    w.ents[i].y - target.1
                ) <= UCELL,
                "seed {seed:#x} retired at a collision detour instead of the order goal"
            );
        }
    }
}
