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

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use don_sim::balance::BalanceTable;
use don_sim::mechanics::{
    commerce_cap, credit_resource, damage_traced, flank_level, get_armor, get_attack,
    resource_period, resource_tick, CommerceCapGates, DamageInput, DamagePredicates, EconomyRules,
    ResourceTickInput,
};
use don_sim::rng::Random;
use don_sim::systems::collision::{
    self, CollCheck, CollGuy, UnitRow as CollisionUnit, UnitTable as CollisionUnits,
};
use don_sim::systems::combat::{
    below_min_range, in_attack_range, scale_damage_build, scale_damage_unit, vector_dist_between,
    AttackCycle, CombatConstants, HitPoints, RANGE_UNITS_PER_TILE,
};
use don_sim::systems::groups_guys::{GuyEnv, UnitGuys, UnitTypeStats};
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
    collision_world: don_sim::systems::map_terrain::World,
    collision_check: CollCheck,
    collision_units: CollisionUnits,
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
            collision_world,
            collision_check: CollCheck::new(),
            collision_units: CollisionUnits::default(),
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
        // `Unit::init` writes this binary angle before its first `set_new_location` call.
        const INITIAL_UNIT_ANGLE: i32 = 0x5555_5555;
        let type_stats = unit_type_stats(&t);
        let mut motion = if t.kind_unit {
            let mut u = UnitWork::at(who, self.ents.len() as i16, x, y);
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
            UnitGuys::spawn_full(type_id, who as i8, self.ents.len() as i16, &type_stats)
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
            facing: if t.kind_unit { INITIAL_UNIT_ANGLE } else { 0 },
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
            u.orders.replace(OrderRec::move_to(gx, gy, tol));
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
        .unwrap_or((
            e.id.index().unwrap_or_default() as i32,
            0,
            0,
            false,
            0,
            false,
            0,
        ));
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
            let blocker_o = blocker_i as i16;
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
                .find(0, blocker_i as i32)
                .expect("blocker collision row");
            w.collision_units.rows[blocker_row_i].x = exact.0;
            w.collision_units.rows[blocker_row_i].y = exact.1;
            for guy in w
                .collision_units
                .guys
                .iter_mut()
                .filter(|g| g.who == 0 && g.o == blocker_i as i32)
            {
                guy.body.x = exact.0;
                guy.body.y = exact.1;
            }
            let actor_row = w.collision_units.rows[w
                .collision_units
                .find(0, i as i32)
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
                    .find(0, i as i32)
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
