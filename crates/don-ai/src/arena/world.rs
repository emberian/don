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
//! | balance percentages | `schema/live/balance-real.bin`, the real 493x493 table |
//! | unit and building stats | `schema/live/live-tables-*.tsv`, live reads |
//! | rules constants | `ron-data/rules.xml` |
//!
//! Ours, and each one a **model choice, not a fidelity claim**, marked `MODEL n` at its
//! site:
//!
//! 2. A building's construction consumes builder-frames: `JOB_TIME` is spent one frame
//!    per assigned citizen per frame, which is exactly what the shipped comment says
//!    (*"How long for one citizen to build"*) but is not how retail's build sites are
//!    known to work.
//! 3. Gather slots come from the terrain under the building, capped at the numbers the
//!    shipped script's own arithmetic implies (Farm 1, Camp 5).
//! 4. Target acquisition is "nearest hostile inside `UNIT_RESPOND_RANGE`". Retail's is
//!    `Object::poor_target` plus a scan whose scheduling is not derived.
//! 5. Flanking is fed **real geometry** and whatever `mechanics::damage` then does with
//!    it. `attack_dir`'s semantics are a known-open question in this project, so the
//!    arena measures the resulting flank-level distribution rather than assuming it.
//! 6. No water, no naval, no air, no diplomacy, no attrition, no supply.
//!
//! Fidelity: **C**. Nothing here is differentially tested against retail.

use std::collections::{BTreeMap, BTreeSet};

use don_sim::balance::BalanceTable;
use don_sim::mechanics::{
    commerce_cap, credit_resource, damage_traced, flank_level, get_armor, get_attack,
    resource_period, resource_tick, CommerceCapGates, DamageInput, DamagePredicates, EconomyRules,
    ResourceTickInput,
};
use don_sim::rng::Random;
use don_sim::systems::combat::{
    below_min_range, in_attack_range, scale_damage_build, scale_damage_unit, vector_dist_between,
    AttackCycle, CombatConstants, HitPoints, RANGE_UNITS_PER_TILE,
};
use don_sim::systems::groups_guys::{GuyData, GuyEnv, UnitGuys, UnitTypeStats};
use don_sim::systems::movement::{PathFinder, PathUnit, UnitWorld, UCELL};
use don_sim::systems::order_dispatch::{
    self, ArmResult, AttackOutcome, DispatchCoverage, GatherOutcome, KillReason, OrderRec,
    TargetState, UnitWork, WorkWorld,
};

use super::cmd::{Cmd, EntId};
use super::map::{Map, Spatial, Terrain};
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
    /// Builder-frames of `JOB_TIME` still owed.
    pub build_left: i32,
    pub job: Job,
    pub cycle: AttackCycle,
    /// 32-bit turn units. Set when the entity moves or fires.
    pub facing: i32,
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
    /// `UNIT_RESPOND_RANGE`, in tiles — from `CombatConstants::shipped`.
    pub respond_range: i32,
    /// Frames between target re-acquisitions. `docs/tracks/ron-ai.md` measured "each unit
    /// re-evaluates every 32 ticks"; that is the number.
    pub retarget_period: i64,
    /// Minimum tile separation between city centres. Ours: `CITY_CENTER_RADIUS`, the
    /// radius inside which a city owns its buildings, so two cities at that spacing have
    /// touching rather than overlapping build areas.
    pub min_city_sep: i32,
}

impl Default for ArenaParams {
    fn default() -> Self {
        ArenaParams {
            gather_period_shift: 0,
            difficulty_income_bonus: 0,
            fog_period: 15,
            respond_range: CombatConstants::shipped().unit_respond_range,
            retarget_period: 32,
            min_city_sep: 20,
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
    pub params: ArenaParams,
    pub log: Vec<Event>,
    pub logging: bool,
    /// Flank levels observed, indexed 0..=2. A measurement, not a control.
    pub flank_hist: [u64; 3],
    pub shots: u64,
    /// `(player, verb, result)` -> count. A bot that spends half its commands on orders
    /// the world refuses is broken in a way no score line shows, so the arena counts them
    /// by verb rather than in one bucket.
    pub rejects: BTreeMap<(u8, &'static str, &'static str), u32>,
    /// Retail has one shared pathfinder singleton and one main simulation RNG.  Both are
    /// persistent here so a failed search consumes the same stream later call sites use.
    pathfinder: PathFinder,
    game_random: Random,
    pub movement_coverage: DispatchCoverage,
    age_techs: Vec<i32>,
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
        let n = (map.w * map.h) as usize;
        let sim_seed = map.seed as i32;
        let econ = EconomyRules {
            commerce_cap: types.constants.commerce_cap,
            ..EconomyRules::shipped()
        };
        let age_techs = types.age_techs();
        let spatial = map.spatial;
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
            params,
            log: Vec::new(),
            logging: true,
            flank_hist: [0; 3],
            shots: 0,
            rejects: BTreeMap::new(),
            pathfinder: PathFinder::new(),
            game_random: Random::new(sim_seed),
            movement_coverage: DispatchCoverage::default(),
            age_techs,
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
        let id = EntId::from_index(self.ents.len());
        let (worker_cap, gather_res) = self.gather_capacity(&t, tx, ty);
        let city = if t.kind_building {
            self.nearest_own_city(who, tx, ty).unwrap_or(EntId::NONE)
        } else {
            EntId::NONE
        };
        let x = tx * RANGE_UNITS_PER_TILE + HALF;
        let y = ty * RANGE_UNITS_PER_TILE + HALF;
        let type_stats = unit_type_stats(&t);
        let mut motion = if t.kind_unit {
            let mut u = UnitWork::at(who, self.ents.len() as i16, x, y);
            u.ptype = type_id;
            u.myspeed = t.moves.clamp(0, i16::MAX as i32) as i16;
            u.path_unit = PathUnit {
                type_size: t.new_block_radius.max(1),
                can_board_transport: false,
                small_footprint: t.domain < 2,
                can_transport: false,
            };
            u.turn_rate = t.turn_speed;
            Some(u)
        } else {
            None
        };
        let mut guys = if t.kind_unit {
            UnitGuys::spawn_full(type_id, who as i8, self.ents.len() as i16, &type_stats)
        } else {
            UnitGuys::default()
        };
        for g in guys.guys.iter_mut().flatten() {
            g.x = x;
            g.y = y;
            g.des_x = x;
            g.des_y = y;
        }
        self.ents.push(Ent {
            id,
            who,
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
            job: Job::Idle,
            cycle: AttackCycle::default(),
            facing: 0,
            city,
            workers: 0,
            worker_cap,
            gather_res,
            queue: Vec::new(),
            assigned_to: EntId::NONE,
            last_damaged: -1,
            spawn_frame: self.frame,
            motion: motion.take(),
            guys,
        });
        id
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
                if b.who != who || !b.building || !b.complete || b.worker_cap <= 0 {
                    return OrderResult::Invalid;
                }
                if b.workers >= b.worker_cap {
                    return OrderResult::Refused;
                }
                if self.ty(unit).map(|t| t.id) != Some(self.ids.citizen) {
                    return OrderResult::Invalid;
                }
                self.detach(unit);
                self.ent_mut(unit).unwrap().job = Job::Gather { target };
                OrderResult::Ok(1)
            }
            Cmd::Work { unit, target } => {
                let Some(b) = self.ent(target) else {
                    return OrderResult::Invalid;
                };
                if b.who != who || !b.building {
                    return OrderResult::Invalid;
                }
                if self.ty(unit).map(|t| t.id) != Some(self.ids.citizen) {
                    return OrderResult::Invalid;
                }
                self.detach(unit);
                self.ent_mut(unit).unwrap().job = Job::Work { target };
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
        let seat = e.assigned_to;
        let was_gathering = matches!(e.job, Job::Gather { .. });
        if let Some(b) = self.ent_mut(seat) {
            if was_gathering {
                b.workers = (b.workers - 1).max(0);
            }
        }
        if let Some(e) = self.ent_mut(unit) {
            e.assigned_to = EntId::NONE;
            if let Some(u) = &mut e.motion {
                u.orders.clear();
                order_dispatch::clear_partial_path(u);
                u.unit_masks &= !order_dispatch::masks::PATH_EXHAUSTED;
            }
        }
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
        self.detach(worker);
        self.ent_mut(worker).unwrap().job = Job::Work { target: site };
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
        self.frame += 1;
        let n = self.players.len();
        // `Objects::process_all` rotates owner order every frame as `(frame + i) % 10`
        // [measured]. The arena echoes it so no player is permanently first.
        for k in 0..n {
            let pi = ((self.frame as usize) + k) % n;
            if !self.players[pi].alive {
                continue;
            }
            self.tick_economy(pi);
            self.tick_ents(pi);
        }
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
                if e.complete && e.workers > 0 {
                    gross[e.gather_res] += self.types.constants.peasant_rate * e.workers;
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

    fn tick_ents(&mut self, pi: usize) {
        let idxs: Vec<usize> = (0..self.ents.len())
            .filter(|&i| self.ents[i].alive && self.ents[i].who as usize == pi)
            .collect();
        for i in idxs {
            if !self.ents[i].alive {
                continue;
            }
            self.tick_queue(i);
            self.tick_cycle(i);
            self.tick_job(i);
        }
    }

    fn tick_cycle(&mut self, i: usize) {
        let c = self.ents[i].cycle.recharging;
        if c > 0 {
            self.ents[i].cycle.recharging = c - 1;
        }
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
                self.auto_acquire(i);
            }
            Job::MoveTo { x, y } => {
                match self.step_toward(i, x, y, 0) {
                    MoveProgress::Arrived | MoveProgress::Failed => {
                        self.ents[i].job = Job::Idle;
                    }
                    MoveProgress::Working => {}
                }
                self.auto_acquire(i);
            }
            Job::Attack { target } => self.do_attack(i, target),
            Job::Gather { target } => {
                let Some(b) = self.ent(target).cloned() else {
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
                    self.ents[i].job = Job::Idle;
                    return;
                };
                if b.complete && b.hits_left() >= b.hp.myhits {
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
                        // MODEL 2 — one builder-frame per assigned citizen per frame.
                        let e = self.ent_mut(target).unwrap();
                        if !e.complete {
                            e.build_left -= 1;
                            if e.build_left <= 0 {
                                e.complete = true;
                                let name = self
                                    .types
                                    .get(self.ents[i].who as i32)
                                    .map(|_| String::new())
                                    .unwrap_or_default();
                                let _ = name;
                                let ty = self.ent(target).unwrap().type_id;
                                let who = self.ents[i].who;
                                let n = self
                                    .types
                                    .get(ty)
                                    .map(|t| t.name.clone())
                                    .unwrap_or_default();
                                self.note(who, format!("built {n}"));
                            }
                        } else {
                            // Repair: 1/16 of a hit point per frame, the same granularity the
                            // engine damages in (`HitPoints::accumulate`).
                            let e = self.ent_mut(target).unwrap();
                            e.hp.damage = (e.hp.damage - 1).max(0);
                        }
                    }
                    MoveProgress::Failed => self.ents[i].job = Job::Idle,
                    MoveProgress::Working => {}
                }
            }
        }
    }

    /// Execute one frame of retail `Unit::do_move` against the arena map.  The `UnitWork`
    /// stored on the entity owns the real order queue, waypoint stack, movement masks and
    /// parked-search state; the world's singleton [`PathFinder`] and main [`Random`] are
    /// passed through unchanged.
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
                || o.dest_x != gx
                || o.dest_y != gy
                || u.tolerance != tol
        });
        if replace {
            u.orders.replace(OrderRec::move_to(gx, gy, tol));
            order_dispatch::clear_partial_path(&mut u);
            u.unit_masks &= !order_dispatch::masks::PATH_EXHAUSTED;
        }
        u.tolerance = tol;

        // `GuyData::turn_speed(0)` is the exact retail damping calculation.  The live
        // `turn_speed` is already a binary angle; `(turn_speed >> 8) * 256` reconstructs
        // it through the shipped `UNIT_TURN_SPEED` master scale.
        if let Some(g) = self.ents[i].guys.guys.first().and_then(Option::as_ref) {
            let env = GuyEnv {
                ut: unit_type_stats(&t),
                unit_speed: t.moves,
                order_speed_bonus: false,
                unit_mask_turn_scale2: u.unit_masks & 0x0008_0000 != 0,
                turn_scale: 256,
                turn_scale2: 2,
                ai_speed: 1,
            };
            u.turn_rate = g.turn_speed(&env, 0) as i32;
        }

        let mover_radius = t.new_block_radius.max(1) * UCELL;
        let obstacles = self
            .ents
            .iter()
            .enumerate()
            .filter(|(j, e)| *j != i && e.alive)
            .map(|(_, e)| {
                let r = self.types.get(e.type_id).map_or(UCELL, |ot| {
                    if ot.kind_building {
                        ot.x_size.max(ot.y_size).max(1) * (RANGE_UNITS_PER_TILE / 2)
                    } else {
                        ot.new_block_radius.max(1) * UCELL
                    }
                });
                (e.x, e.y, r)
            })
            .collect();
        let before = (u.body.x, u.body.y);
        let mut host = ArenaMoveWorld {
            map: &self.map,
            obstacles,
            mover_radius,
            frame: self.frame as i32,
            rng: &mut self.game_random,
        };
        let result = order_dispatch::do_move(
            &mut u,
            &mut host,
            &mut self.pathfinder,
            &mut self.movement_coverage,
        );

        let after = (u.body.x, u.body.y);
        self.ents[i].x = after.0;
        self.ents[i].y = after.1;
        self.ents[i].facing = u.body.angle;
        let moved = don_sim::systems::movement::vector_dist(after.0 - before.0, after.1 - before.1);
        for g in self.ents[i].guys.guys.iter_mut().flatten() {
            g.x = g.x.wrapping_add(after.0 - before.0);
            g.y = g.y.wrapping_add(after.1 - before.1);
            g.des_x = g.x;
            g.des_y = g.y;
            g.angle = u.body.angle;
            g.last_speed = moved;
            g.update_avg_speed();
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
            | ArmResult::Gathered(_) => MoveProgress::Failed,
        }
    }

    /// MODEL 4 — nearest hostile inside `UNIT_RESPOND_RANGE`, re-evaluated every
    /// `retarget_period` frames.
    fn auto_acquire(&mut self, i: usize) {
        if (self.frame + i as i64) % self.params.retarget_period != 0 {
            return;
        }
        let e = &self.ents[i];
        let Some(t) = self.types.get(e.type_id) else {
            return;
        };
        // Civilians do not pick fights. A Citizen has `ATTACK 4` and would otherwise
        // charge the first soldier that walked past, which is neither retail behaviour nor
        // anything a player would want.
        if t.attack <= 0 || t.is_civilian() {
            return;
        }
        let (ex, ey, who) = (e.x, e.y, e.who);
        let reach = self.params.respond_range * RANGE_UNITS_PER_TILE;
        let mut best: Option<(i32, EntId)> = None;
        for o in self.ents.iter().filter(|o| o.alive) {
            if !self.is_hostile(who, o.who) {
                continue;
            }
            let d = vector_dist_between(ex, ey, o.x, o.y);
            if d <= reach && best.map_or(true, |(bd, _)| d < bd) {
                best = Some((d, o.id));
            }
        }
        if let Some((_, id)) = best {
            self.ents[i].job = Job::Attack { target: id };
        }
    }

    fn do_attack(&mut self, i: usize, target: EntId) {
        let Some(tgt) = self.ent(target).cloned() else {
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
        self.ents[i].facing = dir_between(attacker.x, attacker.y, tgt.x, tgt.y);
        if self.ents[i].cycle.recharging != 0 {
            return;
        }
        let dealt = self.fire(i, target, &at, &dt);
        let rech = don_sim::systems::combat::recharge_frames(
            &don_sim::systems::combat::RechargeInput {
                base_recharge: at.recharge,
                is_siege: at.cat == 3,
                unit_masks2_bit0: false,
                in_supply: true,
                is_bombard: false,
            },
            &self.combat,
        );
        self.ents[i].cycle.fire(rech);
        let who = self.ents[i].who;
        self.players[who as usize].damage_dealt += dealt as i64;
        let tw = tgt.who as usize;
        self.players[tw].damage_taken += dealt as i64;
    }

    /// `ObjectData::attack_dist` is edge-to-edge: `vector_dist` with the target's
    /// footprint subtracted (`block_radius + 0x18`, or `x_size`/`y_size` x `0x60` for a
    /// building). The footprint term is a **reading** of that comment, not a measurement.
    fn attack_dist(&self, a: &Ent, b: &Ent, bt: &TypeRow) -> i32 {
        let d = vector_dist_between(a.x, a.y, b.x, b.y);
        let foot = if bt.kind_building {
            bt.x_size.max(bt.y_size) * 0x60
        } else {
            0x18
        };
        (d - foot).max(0)
    }

    /// One shot. Everything numeric here comes out of `don_sim::mechanics::damage`.
    fn fire(&mut self, i: usize, target: EntId, at: &TypeRow, dt: &TypeRow) -> i32 {
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
            attack_dir: dir_between(a.x, a.y, b.x, b.y),
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
        // MODEL 5 — record what the flank classifier actually did with real geometry.
        if !a.building && !b.building {
            let delta = (b.facing as u32)
                .wrapping_sub(input.attack_dir as u32)
                .wrapping_sub(0x8000_0000);
            let lvl = if delta >= 0x2AAA_AAAA {
                flank_level(delta) as usize
            } else {
                0
            };
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
            self.ents[i].alive = false;
            self.players[e.who as usize].losses += 1;
            for p in 0..self.players.len() {
                if p != e.who as usize {
                    self.players[p].kills += 1;
                }
            }
            // Free any seat the dead entity held or held for others.
            if e.building {
                for o in self.ents.iter_mut() {
                    if o.alive && o.assigned_to == e.id {
                        o.assigned_to = EntId::NONE;
                        o.job = Job::Idle;
                    }
                }
            } else if let Some(b) = self.ent_mut(e.assigned_to) {
                if matches!(e.job, Job::Gather { .. }) {
                    b.workers = (b.workers - 1).max(0);
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

/// The 8-way movement lattice, in the order `dir_index` numbers it.
const DIR8: [(i32, i32); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

/// The lattice index nearest the direction `(dx, dy)`.
fn dir_index(dx: i32, dy: i32) -> i32 {
    // Octant by comparing |dx| and |dy| against each other -- no trigonometry, no floats.
    let (ax, ay) = (dx.abs(), dy.abs());
    let diag = ax * 2 > ay && ay * 2 > ax;
    match (dx.signum(), dy.signum()) {
        (1, 0) => 0,
        (1, 1) => {
            if diag {
                1
            } else if ax > ay {
                0
            } else {
                2
            }
        }
        (0, 1) => 2,
        (-1, 1) => {
            if diag {
                3
            } else if ax > ay {
                4
            } else {
                2
            }
        }
        (-1, 0) => 4,
        (-1, -1) => {
            if diag {
                5
            } else if ax > ay {
                4
            } else {
                6
            }
        }
        (0, -1) => 6,
        (1, -1) => {
            if diag {
                7
            } else if ax > ay {
                0
            } else {
                6
            }
        }
        _ => 0,
    }
}

/// The 8-way lattice direction as a 32-bit turn angle.
fn dir8(dx: i32, dy: i32) -> i32 {
    let k = match (dx.signum(), dy.signum()) {
        (1, 0) => 0,
        (1, 1) => 1,
        (0, 1) => 2,
        (-1, 1) => 3,
        (-1, 0) => 4,
        (-1, -1) => 5,
        (0, -1) => 6,
        _ => 7,
    };
    (k as i32).wrapping_mul(0x2000_0000)
}

fn dir_between(ax: i32, ay: i32, bx: i32, by: i32) -> i32 {
    dir8(bx - ax, by - ay)
}
