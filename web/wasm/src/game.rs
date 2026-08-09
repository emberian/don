//! Legacy browser-world prototype plus the packed [`PlayData`] decoder.
//!
//! The playable ABI no longer instantiates [`GameWorld`]: `game_abi` owns the repository's
//! authoritative `don_sim::tick::Sim` and uses this module only for immutable packed-data
//! records and compatibility constants. The prototype remains temporarily as derivation
//! history while its still-unported build/train/research hosts move into `don_sim`; it must
//! not be reintroduced as browser gameplay state or given its own save format.
//!
//! # What is derived here and what is not — read this before believing anything on screen
//!
//! `crate::real` is the spectator's battle world. This module is the client's *game* world,
//! and it is a strictly larger claim, so the split is stated first and in more detail.
//!
//! ## Derived, and used unmodified
//!
//! | thing | where it comes from |
//! |---|---|
//! | the tile world itself | `don_sim::systems::map_terrain::World` — `WorldData`'s real layout, `World::init` `0x006b76f0`, and the real `set_*_at` mutators with their `blocked`/`solid`/`bad` counter bookkeeping |
//! | **building placement** | `WorldData::space_at_corner` `0x006b27f0` and `WorldData::check_building_wcoord` `0x006b26e0`, unmodified. The green/amber/red tint on the footprint preview *is* the grade those return |
//! | **gather reachability** | `WorldData::has_gather_access` `0x006b4e50` — a tile is workable exactly when the engine says a gatherer could reach it |
//! | **the economy tick** | `don_sim::systems::economy::leader_gather` — `Leader::gather` `0x006CE280` -> `calc_gather` `0x006CEEE0` -> `calc_resource_caps` `0x006CE900` -> `do_gather` `0x006CE450`, driven once per frame per player |
//! | the per-worker gather rate | `worker_rate` — `PEASANT_RATE`/`OIL_RATE` unscaled 8.8, i.e. 10 and 35 per `GATHER_RATE` frames |
//! | the city yield | `calc_city_resources` `0x006D5530`, which is where `CITY_GATHER` (10 food + 10 timber) enters |
//! | the income ceiling | `calc_resource_caps`, per age |
//! | starting stockpiles | `STARTING_GOODS` = `[200, 200, 100, 100, 100, 100]`, rules offset 564 |
//! | the population cap | `POP_CAP[setting]` = `{25,50,75,100,125,150,175,200}`, rules offset 964 |
//! | costs, build times, footprints | `COST[6]`, `JOB_TIME`, `X_SIZE`/`Y_SIZE` off the live `UnitType`/`BuildType` tables |
//! | what a building can make | the 312 `WHERE` -> building-name edges, see [`PlayData::products_of`] |
//! | age advance costs | the seven age techs, `TypeIndex` 544..550 |
//! | damage | `don_sim::damage`, exactly as `crate::real` uses it |
//! | the frame scheduler | `Objects::process_all`'s `(frame + i) % 10` owner rotation |
//!
//! ## Not derived — this file's own glue, every one a placeholder
//!
//! * **Map generation.** The forests, mountains, lakes and start positions are integer
//!   value noise written here. The tile *semantics* are the engine's; the layout is not.
//! * **Movement.** Straight-line steps of `MOVES` subtiles, with a blocked-tile slide.
//!   `PathFinder::astar_path` exists in `don-sim` and is not called from here.
//! * **Target acquisition** and **the attack angle**, as in [`crate::real`].
//! * **Which resource a tile yields.** `has_gather_access` says *whether* a tile is
//!   workable; nothing derived says trees are timber and mountains are metal. That mapping
//!   is [inferred] and is flagged as such in the UI's coverage panel.
//! * **Construction progress.** One point per builder-frame against `JOB_TIME`. The
//!   engine's construction is `BuildData` work we have not read.
//! * **`object_income`'s composition.** `calc_gather` takes the four object-graph loops as
//!   one input, by design; the loop that fills it here (workers on nodes, cities, nothing
//!   else) is ours. Gather-enhancer buildings — Granary, Lumber Mill, Smelter, Refinery —
//!   contribute **nothing**, because `BuildData::calc_gather` `0x0062D360` is unported.
//! * **No fog of war, no line of sight, no techs other than ages, no market, no borders.**

use crate::gamedata::GameData;
use crate::real::{atan2_u32, predicates, OWNER_SLOTS, SUBTILE};
use don_sim::systems::economy as econ;
use don_sim::systems::map_terrain as terr;
use don_sim::{damage, get_armor, get_attack, DamageInput, UnreachedTerms};

/// World cells across the map. `World::init(32, 32)` gives 128x128 tiles, the same span
/// `crate::real` uses, so the two share a coordinate space.
pub const W_CELLS: i32 = 32;
/// Map extent in tiles.
pub const MAP_TILES: i32 = W_CELLS * terr::TILES_PER_WCELL;
/// Map extent in subtiles — the coordinate every command carries.
pub const MAP_SPAN: i32 = MAP_TILES * SUBTILE;
/// Players a game world provisions. The engine has ten owner slots and rotates all ten;
/// four is a lobby size, not an engine limit.
pub const PLAYERS: usize = 4;
/// Object rows a world provisions.
pub const CAPACITY: usize = 4096;
/// Deepest production queue per building.
pub const QUEUE_MAX: usize = 8;
/// `TypeIndex` of the first building.
pub const FIRST_BUILDING: i32 = 414;
/// `TypeIndex` of the first unit.
pub const FIRST_UNIT: i32 = 50;
/// The seven age techs, `TypeIndex` 544..550 — Classical through Information.
pub const AGE_TECH_BASE: i32 = 544;

/// Rules byte offset of `STARTING_GOODS[6]` (`docs/derivation/rules-constants.json`).
const OFF_STARTING_GOODS: usize = 564;
/// Rules byte offset of `POP_CAP[8]`.
const OFF_POP_CAP: usize = 964;
/// Rules byte offset of `MAX_POP_LIMIT`.
const OFF_MAX_POP_LIMIT: usize = 1008;

/// Order kinds a unit can hold. These are *this file's* order set, not the engine's
/// 28-entry `OrderIndex` jump table; each one names the retail order it stands for.
pub mod ord {
    /// No standing order.
    pub const NONE: u8 = 0;
    /// `OrderIndex::MOVE_TO` (1).
    pub const MOVE: u8 = 1;
    /// `OrderIndex::ATTACK` (10).
    pub const ATTACK: u8 = 2;
    /// `OrderIndex::GATHER` (7) — **absent from `Unit::do_job` in `don-sim`**; the
    /// behaviour here is this file's.
    pub const GATHER: u8 = 3;
    /// `OrderIndex::BUILD_AT` (6) — likewise absent; ours.
    pub const BUILD: u8 = 4;
}

/// Reasons an issued command did not execute. The client shows these live rather than
/// letting a swallowed order look like a working one.
pub mod gap {
    pub const UNKNOWN_OPCODE: usize = 0;
    pub const NO_SELECTION: usize = 1;
    pub const CANNOT_AFFORD: usize = 2;
    pub const POP_CAPPED: usize = 3;
    pub const PLACEMENT_BLOCKED: usize = 4;
    pub const NOT_A_PRODUCER: usize = 5;
    pub const QUEUE_FULL: usize = 6;
    pub const NO_WORKER: usize = 7;
    pub const NOT_GATHERABLE: usize = 8;
    pub const WRONG_AGE: usize = 9;
    pub const CAPACITY_FULL: usize = 10;
    /// A unit gave up on an order because straight-line movement could not make progress.
    /// This is the placeholder pathfinder failing, and it is counted rather than hidden.
    pub const PATH_STUCK: usize = 11;
    pub const COUNT: usize = 12;
}

// ---------------------------------------------------------------------------------------
// The extra derived tables (`web/tools/pack-playdata.mjs` -> `playdata.bin`)
// ---------------------------------------------------------------------------------------

/// i32 fields per packed unit record. Must match `UNIT_FIELDS` in the packer.
pub const PLAY_UNIT_FIELDS: usize = 13;
/// i32 fields per packed building record. Must match `BLD_FIELDS` in the packer.
pub const PLAY_BLD_FIELDS: usize = 18;
const PLAY_MAGIC: &[u8; 8] = b"DONPLAY1";

/// The costed half of a unit type: what it takes to make one, and where.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlayUnit {
    pub type_id: i32,
    /// `COST[6]`, engine slot order: food, timber, wealth, knowledge, metal, oil.
    pub cost: [i32; econ::NUM_RESOURCES],
    /// `POP` from `ron-data/unitrules.xml`. `-1` when the XML was absent; treated as 1.
    pub pop: i32,
    /// `JOB_TIME`, used here as frames.
    pub job_time: i32,
    /// `WHERE` — the building type that trains it, or `-1`.
    pub where_: i32,
    pub age: i32,
    pub obj_masks: i32,
    pub los: i32,
}

/// A building type.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlayBld {
    pub type_id: i32,
    pub cost: [i32; econ::NUM_RESOURCES],
    pub job_time: i32,
    /// `X_SIZE`/`Y_SIZE` — the footprint in tiles.
    pub x_size: i32,
    pub y_size: i32,
    pub hits: i32,
    pub armor: i32,
    pub attack: i32,
    pub max_range: i32,
    pub recharge: i32,
    pub age: i32,
    pub obj_masks: i32,
    pub los: i32,
}

/// Everything `playdata.bin` carries, plus the joins the client asks of it.
pub struct PlayData {
    pub units: Vec<PlayUnit>,
    unit_by_id: Vec<i32>,
    pub blds: Vec<PlayBld>,
    bld_by_id: Vec<i32>,
    /// `(producer building type id, product unit type id)`, sorted by producer.
    pub edges: Vec<(i32, i32)>,
    /// Age tech costs, index 0 = Classical (advancing *to* age 1).
    pub age_cost: Vec<[i32; econ::NUM_RESOURCES]>,
    pub rules: econ::EconRules,
    /// False when `playdata.bin` was absent or did not parse: the client then has no
    /// costs, no menus and no economy, and says so rather than inventing any.
    pub is_real: bool,
}

impl PlayData {
    /// Shipped-constant fallback with no tables at all. Everything it answers is empty,
    /// which is what makes "no play data" visible instead of plausible.
    pub fn empty() -> PlayData {
        PlayData {
            units: Vec::new(),
            unit_by_id: Vec::new(),
            blds: Vec::new(),
            bld_by_id: Vec::new(),
            edges: Vec::new(),
            age_cost: Vec::new(),
            rules: econ::EconRules::shipped(),
            is_real: false,
        }
    }

    pub fn parse(b: &[u8]) -> Option<PlayData> {
        if b.len() < 40 || &b[0..8] != PLAY_MAGIC {
            return None;
        }
        let rd = |o: usize| i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let u = |o: usize| rd(o) as usize;
        let (nu, fu, nb, fb) = (u(8), u(12), u(16), u(20));
        let (ne, na, nr) = (u(24), u(28), u(32));
        if fu != PLAY_UNIT_FIELDS || fb != PLAY_BLD_FIELDS {
            return None;
        }
        let head = 40;
        let need = head + (nu * fu + nb * fb + ne * 2 + na * 8 + nr) * 4;
        if b.len() < need {
            return None;
        }
        let mut o = head;
        let mut units = Vec::with_capacity(nu);
        for _ in 0..nu {
            let f = |j: usize| rd(o + j * 4);
            units.push(PlayUnit {
                type_id: f(0),
                cost: [f(1), f(2), f(3), f(4), f(5), f(6)],
                pop: f(7),
                job_time: f(8),
                where_: f(9),
                age: f(10),
                obj_masks: f(11),
                los: f(12),
            });
            o += fu * 4;
        }
        let mut blds = Vec::with_capacity(nb);
        for _ in 0..nb {
            let f = |j: usize| rd(o + j * 4);
            blds.push(PlayBld {
                type_id: f(0),
                cost: [f(1), f(2), f(3), f(4), f(5), f(6)],
                job_time: f(7),
                x_size: f(8).max(1),
                y_size: f(9).max(1),
                hits: f(10).max(1),
                armor: f(11).max(0),
                attack: f(12).max(0),
                max_range: f(13).max(0),
                recharge: f(14).max(1),
                age: f(15),
                obj_masks: f(16),
                los: f(17),
            });
            o += fb * 4;
        }
        let mut edges = Vec::with_capacity(ne);
        for _ in 0..ne {
            edges.push((rd(o), rd(o + 4)));
            o += 8;
        }
        edges.sort_unstable();
        let mut age_cost = Vec::with_capacity(na);
        for _ in 0..na {
            age_cost.push([rd(o + 4), rd(o + 8), rd(o + 12), rd(o + 16), rd(o + 20), rd(o + 24)]);
            o += 32;
        }
        let mut block = vec![0i32; nr];
        for (k, slot) in block.iter_mut().enumerate() {
            *slot = rd(o + k * 4);
        }

        let mut unit_by_id = vec![-1i32; 1024];
        for (i, u) in units.iter().enumerate() {
            if (0..1024).contains(&u.type_id) {
                unit_by_id[u.type_id as usize] = i as i32;
            }
        }
        let mut bld_by_id = vec![-1i32; 1024];
        for (i, x) in blds.iter().enumerate() {
            if (0..1024).contains(&x.type_id) {
                bld_by_id[x.type_id as usize] = i as i32;
            }
        }
        Some(PlayData {
            units,
            unit_by_id,
            blds,
            bld_by_id,
            edges,
            age_cost,
            rules: econ::EconRules::from_block(&block),
            is_real: true,
        })
    }

    #[inline]
    pub fn unit(&self, type_id: i32) -> Option<&PlayUnit> {
        let k = *self.unit_by_id.get(type_id.max(0) as usize)?;
        if k < 0 {
            None
        } else {
            self.units.get(k as usize)
        }
    }
    #[inline]
    pub fn bld(&self, type_id: i32) -> Option<&PlayBld> {
        let k = *self.bld_by_id.get(type_id.max(0) as usize)?;
        if k < 0 {
            None
        } else {
            self.blds.get(k as usize)
        }
    }
    /// Product type ids a producer can train — the `WHERE` join, as data.
    pub fn products_of(&self, producer: i32) -> impl Iterator<Item = i32> + '_ {
        self.edges
            .iter()
            .filter(move |(w, _)| *w == producer)
            .map(|(_, t)| *t)
    }
    /// Is this a producer at all?
    pub fn is_producer(&self, type_id: i32) -> bool {
        self.edges.iter().any(|(w, _)| *w == type_id)
    }
}

// ---------------------------------------------------------------------------------------
// Objects
// ---------------------------------------------------------------------------------------

/// Everything about an object that is not streamed to the GPU every frame.
///
/// The three hot columns (`pos_x`, `pos_y`, `tag`) stay structure-of-arrays because they
/// are bound directly as vertex buffers; this record is the cold half and nothing reads it
/// per pixel, so keeping it as one struct costs nothing and saves twenty parallel `Vec`s.
#[derive(Clone, Copy, Debug, Default)]
pub struct Aux {
    pub hits: i32,
    pub max_hits: i32,
    pub owner: u8,
    /// 0 = unit, 1 = building.
    pub is_building: bool,
    pub type_id: i32,
    /// Row in `GameData::units` for a unit; row in `PlayData::blds` for a building.
    pub tidx: u16,
    pub cooldown: i16,
    pub facing: i32,
    pub order: u8,
    /// Order argument: target object id (ATTACK/BUILD) or tile index (GATHER).
    pub order_a: i32,
    pub order_x: i32,
    pub order_y: i32,
    pub selected: bool,
    /// Anchor tile of a building's footprint.
    pub tx: i32,
    pub ty: i32,
    /// Construction points accrued; `job_time` completes it. `-1` = complete.
    pub build_progress: i32,
    /// Production queue (type ids) and the frames accrued on the head item.
    pub queue: [i32; QUEUE_MAX],
    pub queue_n: u8,
    pub queue_prog: i32,
    /// Rally point in subtiles.
    pub rally_x: i32,
    pub rally_y: i32,
    /// Which resource slot this unit delivered into last frame, `-1` for none. Drives the
    /// "carrying" tint and the per-resource worker counts in the HUD.
    pub gather_res: i8,
    /// Consecutive frames this unit tried to move and could not.
    pub stuck: u8,
}

/// One player's ledger. The economy fields are `don_sim`'s; the rest is bookkeeping.
#[derive(Clone, Debug)]
pub struct Player {
    pub econ: econ::LeaderEcon,
    pub last_calc: i32,
    pub dirty: bool,
    /// Payout displayed by the HUD, per resource — `do_gather`'s post-cap pre-handicap
    /// income, i.e. exactly the number retail shows.
    pub income: [i32; econ::NUM_RESOURCES],
    /// `calc_gather`'s gross, **before** the commerce-cap clamp. Shown next to `income` so
    /// the clamp is visible rather than silently eating the difference.
    pub gross: [i32; econ::NUM_RESOURCES],
    pub pop: i32,
    pub pop_cap: i32,
    /// Workers currently delivering into each slot.
    pub workers: [i32; econ::NUM_RESOURCES],
    pub buildings: i32,
    pub units: i32,
    /// Age tech being researched, or -1. Progress is in frames against `JOB_TIME` 600.
    pub researching: i32,
    pub research_prog: i32,
}

impl Default for Player {
    fn default() -> Self {
        Player {
            econ: econ::LeaderEcon::new(),
            last_calc: -1,
            dirty: true,
            income: [0; econ::NUM_RESOURCES],
            gross: [0; econ::NUM_RESOURCES],
            pop: 0,
            pop_cap: 0,
            workers: [0; econ::NUM_RESOURCES],
            buildings: 0,
            units: 0,
            researching: -1,
            research_prog: 0,
        }
    }
}

/// A playable world.
pub struct GameWorld {
    pub terrain: terr::World,
    /// Bumped whenever a tile mask changes, so the renderer re-uploads only then.
    pub terrain_version: u32,

    // hot, GPU-bound columns
    pub pos_x: Vec<i32>,
    pub pos_y: Vec<i32>,
    pub tag: Vec<u32>,
    // cold half
    pub aux: Vec<Aux>,

    handle_of_row: Vec<u32>,
    row_of_handle: Vec<u32>,
    live: u32,
    capacity: u32,

    /// tile index -> object id of the building covering it, or -1.
    tile_obj: Vec<i32>,

    pub players: Vec<Player>,
    pub frame: u64,
    rng: u32,
    pub pop_cap_setting: usize,
    /// 0 = run the retail `Leader::do_gather` commerce-cap step: income is clamped by the
    /// derived `COMMERCE_CAP[age]`. This is one recovered step, not a claim that the whole
    /// playable world is in Fidelity mode. 1 = a DoN experiment that lifts only this clamp.
    /// See the track report for why this switch exists — with the derived
    /// composition a single worker already exceeds `COMMERCE_CAP[0]` = 70, so at Ancient
    /// Age workers 2..n buy nothing, and whether that is retail's behaviour or a scale
    /// mismatch in an unported input is an open question this lane surfaces rather than
    /// silently picks a side of.
    pub income_mode: u32,

    /// Issued-but-unexecuted counters, indexed by [`gap`].
    pub gaps: [u32; gap::COUNT],
    pub commands_seen: u64,
    pub orders_applied: u64,

    /// Per-owner ordering scratch for the `(frame + i) % 10` rotation.
    order: Vec<u32>,
    owner_start: [u32; OWNER_SLOTS + 1],
    /// Uniform bucket grid over the map, rebuilt once per frame by counting sort. Target
    /// acquisition without it is a full scan per unit per frame — measured at 22 ms for
    /// 4,087 objects, which is the whole frame budget spent on distance tests.
    grid_dim: i32,
    cell_start: Vec<u32>,
    cell_rows: Vec<u32>,
    /// Start positions in subtiles, one per player.
    pub start_x: [i32; PLAYERS],
    pub start_y: [i32; PLAYERS],
}

const NO_TARGET: i32 = -1;
const NO_ROW: u32 = u32::MAX;

impl GameWorld {
    pub fn new(gd: &GameData, pd: &PlayData, seed: u64) -> GameWorld {
        let terrain = terr::World::init(W_CELLS, W_CELLS, 44, 4, 4);
        let n = CAPACITY;
        let mut w = GameWorld {
            terrain,
            terrain_version: 1,
            pos_x: vec![0; n],
            pos_y: vec![0; n],
            tag: vec![0; n],
            aux: vec![Aux::default(); n],
            handle_of_row: (0..n as u32).collect(),
            row_of_handle: vec![NO_ROW; n],
            live: 0,
            capacity: n as u32,
            tile_obj: vec![-1; (MAP_TILES * MAP_TILES) as usize],
            players: vec![Player::default(); PLAYERS],
            frame: 0,
            rng: (seed as u32) | 1,
            pop_cap_setting: 3,
            income_mode: 0,
            gaps: [0; gap::COUNT],
            commands_seen: 0,
            orders_applied: 0,
            order: vec![0; n],
            owner_start: [0; OWNER_SLOTS + 1],
            grid_dim: 32,
            cell_start: vec![0; 32 * 32 + 1],
            cell_rows: vec![0; n],
            start_x: [0; PLAYERS],
            start_y: [0; PLAYERS],
        };
        w.generate_terrain();
        w.seed_players(gd, pd);
        w.refresh_derived(gd, pd);
        w
    }

    /// `Random::get`'s recurrence `0x00a39cf0`. The *mapping* to a bounded value is ours.
    #[inline]
    fn rand(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        self.rng
    }
    #[inline]
    fn rand_below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            (self.rand() >> 8) % n
        }
    }

    // -- map generation (ours) ----------------------------------------------------------

    /// Deterministic integer value noise. Not the engine's `Map::make`; a stand-in that
    /// produces a map worth playing on, with the engine's own tile semantics stamped into
    /// it through the real `set_*_at` mutators.
    fn generate_terrain(&mut self) {
        let seed = self.rng;
        let hash = |x: i32, y: i32, s: u32| -> u32 {
            let mut h = (x as u32).wrapping_mul(0x9E37_79B9) ^ (y as u32).wrapping_mul(0x85EB_CA6B);
            h ^= s;
            h = h.wrapping_mul(0xC2B2_AE35);
            h ^= h >> 15;
            h
        };
        // Smooth two octaves of cell noise into a per-tile field, integers only.
        let field = |x: i32, y: i32, s: u32, scale: i32| -> i32 {
            let mut acc = 0i32;
            for (oct, weight) in [(scale, 3i32), (scale / 2, 1i32)] {
                let o = oct.max(1);
                let (cx, cy) = (x / o, y / o);
                let (fx, fy) = (x % o, y % o);
                let corner = |dx: i32, dy: i32| (hash(cx + dx, cy + dy, s) >> 20) as i32; // 0..4095
                let a = corner(0, 0);
                let b = corner(1, 0);
                let c = corner(0, 1);
                let d = corner(1, 1);
                let top = a + (b - a) * fx / o;
                let bot = c + (d - c) * fx / o;
                acc += weight * (top + (bot - top) * fy / o);
            }
            acc / 4
        };

        for ty in 0..MAP_TILES {
            for tx in 0..MAP_TILES {
                let f = field(tx, ty, seed, 16);
                let m = field(tx, ty, seed ^ 0x5151_5151, 12);
                let wv = field(tx, ty, seed ^ 0xA3A3_A3A3, 22);
                // Keep the four start quadrants clear of everything.
                let near_start = (0..PLAYERS).any(|p| {
                    let (sx, sy) = Self::start_tile(p);
                    (tx - sx).abs() < 9 && (ty - sy).abs() < 9
                });
                if near_start {
                    continue;
                }
                if wv > 3400 {
                    self.terrain.set_tocean(tx, ty);
                    self.terrain.set_blocked_at(tx, ty, true);
                } else if m > 3350 {
                    self.terrain.set_mountain_at(tx, ty, true);
                } else if f > 2900 {
                    self.terrain.set_tree_at(tx, ty, true);
                }
            }
        }
        // A small forest and a small mountain within reach of every start, so a player has
        // something to work in the first thirty seconds. Placement is ours; the stamping
        // is the engine's mutator.
        for p in 0..PLAYERS {
            let (sx, sy) = Self::start_tile(p);
            for k in 0..40 {
                let a = (k * 37) % 360;
                let r = 7 + (k % 3);
                let dx = (a as i32 - 180) / 40;
                let _ = dx;
                let (ox, oy) = ((k % 7) as i32 - 3, (k / 7) as i32 - 3);
                let (fx, fy) = (sx + r + ox, sy - r + oy);
                if fx > 0 && fy > 0 && fx < MAP_TILES && fy < MAP_TILES {
                    self.terrain.set_tree_at(fx, fy, true);
                }
                let (mx, my) = (sx - r + ox, sy + r + oy);
                if mx > 0 && my > 0 && mx < MAP_TILES && my < MAP_TILES && (ox + oy) % 2 == 0 {
                    self.terrain.set_mountain_at(mx, my, true);
                }
            }
        }
        self.terrain_version += 1;
    }

    /// Start tile of player `p` — four corners, inset.
    fn start_tile(p: usize) -> (i32, i32) {
        let inset = MAP_TILES / 5;
        match p % 4 {
            0 => (inset, inset),
            1 => (MAP_TILES - inset, MAP_TILES - inset),
            2 => (MAP_TILES - inset, inset),
            _ => (inset, MAP_TILES - inset),
        }
    }

    /// The opening position: a Small City, a Library-less handful of Citizens, and two
    /// soldiers. Composition is ours; every stat, cost and footprint is the table's.
    fn seed_players(&mut self, gd: &GameData, pd: &PlayData) {
        // Citizens are `TypeIndex` 50; the first military type in the roster stands in for
        // a starting escort so a new game is not defenceless.
        let citizen = 50;
        let escort = gd
            .roster
            .first()
            .and_then(|&r| gd.units.get(r as usize))
            .map(|u| u.type_id)
            .unwrap_or(citizen);
        let starting = (0..econ::NUM_RESOURCES)
            .map(|i| pd.rules.at(OFF_STARTING_GOODS + i * 4))
            .collect::<Vec<_>>();
        for p in 0..PLAYERS {
            let (stx, sty) = Self::start_tile(p);
            self.start_x[p] = stx * SUBTILE;
            self.start_y[p] = sty * SUBTILE;
            for (i, v) in starting.iter().enumerate() {
                self.players[p].econ.stockpile[i] = *v;
            }
            self.players[p].econ.age = 0;
            // The city goes down through the same placement gate a player's build would use.
            let placed = self.place_building(pd, p as u8, FIRST_BUILDING, stx - 2, sty - 2, true);
            if placed.is_none() {
                self.gaps[gap::PLACEMENT_BLOCKED] += 1;
            }
            for k in 0..6 {
                let a = (k as u32) << 29;
                let dx = ((a >> 29) as i32 - 3) * SUBTILE;
                let dy = SUBTILE * 3;
                self.spawn_unit(gd, p as u8, citizen, stx * SUBTILE + dx, sty * SUBTILE + dy);
            }
            for k in 0..2 {
                self.spawn_unit(
                    gd,
                    p as u8,
                    escort,
                    stx * SUBTILE + (k * 2 - 1) * SUBTILE * 2,
                    sty * SUBTILE - SUBTILE * 3,
                );
            }
        }
    }

    // -- row management ------------------------------------------------------------------

    #[inline]
    pub fn live_count(&self) -> u32 {
        self.live
    }
    #[inline]
    pub fn capacity(&self) -> u32 {
        self.capacity
    }
    #[inline]
    pub fn row_of_id(&self, id: i32) -> Option<usize> {
        if id < 0 || id as u32 >= self.capacity {
            return None;
        }
        let row = self.row_of_handle[id as usize];
        if row >= self.live || self.handle_of_row[row as usize] != id as u32 {
            return None;
        }
        Some(row as usize)
    }
    #[inline]
    pub fn id_of_row(&self, row: usize) -> i32 {
        self.handle_of_row[row] as i32
    }

    fn alloc_row(&mut self) -> Option<usize> {
        if self.live >= self.capacity {
            self.gaps[gap::CAPACITY_FULL] += 1;
            return None;
        }
        let row = self.live as usize;
        let id = self.handle_of_row[row];
        self.row_of_handle[id as usize] = row as u32;
        self.live += 1;
        self.aux[row] = Aux::default();
        Some(row)
    }

    fn despawn_row(&mut self, row: usize) {
        if self.aux[row].is_building {
            self.unstamp_building(row);
        }
        let last = self.live as usize - 1;
        let dead = self.handle_of_row[row];
        if row != last {
            self.pos_x[row] = self.pos_x[last];
            self.pos_y[row] = self.pos_y[last];
            self.tag[row] = self.tag[last];
            self.aux[row] = self.aux[last];
            let moved = self.handle_of_row[last];
            self.handle_of_row[row] = moved;
            self.row_of_handle[moved as usize] = row as u32;
            if self.aux[row].is_building {
                self.restamp_building(row);
            }
        }
        self.handle_of_row[last] = dead;
        self.live -= 1;
    }

    fn spawn_unit(&mut self, gd: &GameData, owner: u8, type_id: i32, x: i32, y: i32) -> i32 {
        let Some(tidx) = gd.index_of_type(type_id) else { return -1 };
        let Some(row) = self.alloc_row() else { return -1 };
        let ut = gd.units[tidx];
        self.pos_x[row] = x.clamp(0, MAP_SPAN - 1);
        self.pos_y[row] = y.clamp(0, MAP_SPAN - 1);
        let a = &mut self.aux[row];
        a.hits = ut.hits.max(1);
        a.max_hits = ut.hits.max(1);
        a.owner = owner;
        a.is_building = false;
        a.type_id = type_id;
        a.tidx = tidx as u16;
        a.order_a = NO_TARGET;
        a.gather_res = -1;
        a.build_progress = -1;
        self.handle_of_row[row] as i32
    }

    // -- buildings -----------------------------------------------------------------------

    #[inline]
    fn t_of(&self, tx: i32, ty: i32) -> usize {
        (ty.clamp(0, MAP_TILES - 1) * MAP_TILES + tx.clamp(0, MAP_TILES - 1)) as usize
    }

    /// Grade a footprint with the **engine's** predicate.
    ///
    /// `WorldData::space_at_corner` grades a 4x4 tile block; a footprint bigger than that
    /// is covered by tiling blocks over it and taking the worst grade, which is this file's
    /// composition of a derived primitive, not a derived rule of its own.
    pub fn grade_placement(&self, pd: &PlayData, who: i32, type_id: i32, tx: i32, ty: i32) -> i32 {
        let Some(b) = pd.bld(type_id) else { return terr::space::CORE_BLOCKED };
        let mut worst = terr::space::FULLY_CLEAR;
        let mut bx = 0;
        while bx < b.x_size {
            let mut by = 0;
            while by < b.y_size {
                let g = self.terrain.space_at_corner(tx + bx, ty + by, who, false);
                if g < worst {
                    worst = g;
                }
                if worst == terr::space::CORE_BLOCKED {
                    return worst;
                }
                by += 4;
            }
            bx += 4;
        }
        worst
    }

    /// `WorldData::check_building_wcoord` `0x006b26e0` over the W cell a point falls in —
    /// the engine's own "is there room near here" query, exposed for the client's
    /// snap-to-buildable hint.
    pub fn check_wcell(&self, who: i32, wx: i32, wy: i32, rx: i32, ry: i32, max_dist: i32) -> i32 {
        self.terrain.check_building_wcoord(wx, wy, who, rx, ry, max_dist, false)
    }

    fn stamp_building(&mut self, row: usize, started_only: bool) {
        let (tx, ty) = (self.aux[row].tx, self.aux[row].ty);
        let (sx, sy) = self.footprint(row);
        let id = self.handle_of_row[row] as i32;
        for y in ty..ty + sy {
            for x in tx..tx + sx {
                if x < 0 || y < 0 || x >= MAP_TILES || y >= MAP_TILES {
                    continue;
                }
                let t = self.t_of(x, y);
                self.tile_obj[t] = id;
                if started_only {
                    self.terrain.set_started_at(x, y, true);
                } else {
                    self.terrain.set_building_at(x, y, true);
                    self.terrain.set_blocked_at(x, y, true);
                }
            }
        }
        self.terrain_version += 1;
    }

    fn unstamp_building(&mut self, row: usize) {
        let (tx, ty) = (self.aux[row].tx, self.aux[row].ty);
        let (sx, sy) = self.footprint(row);
        for y in ty..ty + sy {
            for x in tx..tx + sx {
                if x < 0 || y < 0 || x >= MAP_TILES || y >= MAP_TILES {
                    continue;
                }
                let t = self.t_of(x, y);
                self.tile_obj[t] = -1;
                self.terrain.set_started_at(x, y, false);
                self.terrain.set_building_at(x, y, false);
                self.terrain.set_blocked_at(x, y, false);
            }
        }
        self.terrain_version += 1;
    }

    /// Re-point the tile index at a building whose row moved during compaction.
    fn restamp_building(&mut self, row: usize) {
        let (tx, ty) = (self.aux[row].tx, self.aux[row].ty);
        let (sx, sy) = self.footprint(row);
        let id = self.handle_of_row[row] as i32;
        for y in ty..ty + sy {
            for x in tx..tx + sx {
                if x >= 0 && y >= 0 && x < MAP_TILES && y < MAP_TILES {
                    let t = self.t_of(x, y);
                    self.tile_obj[t] = id;
                }
            }
        }
    }

    fn footprint(&self, row: usize) -> (i32, i32) {
        let a = &self.aux[row];
        if !a.is_building {
            return (1, 1);
        }
        (a.order_x.max(1), a.order_y.max(1))
    }

    /// Place a building. `instant` skips construction (used for the starting city).
    /// Returns the object id.
    pub fn place_building(
        &mut self,
        pd: &PlayData,
        owner: u8,
        type_id: i32,
        tx: i32,
        ty: i32,
        instant: bool,
    ) -> Option<i32> {
        let b = *pd.bld(type_id)?;
        if self.grade_placement(pd, owner as i32, type_id, tx, ty) == terr::space::CORE_BLOCKED {
            return None;
        }
        let bidx = pd.bld_by_id[type_id.max(0) as usize];
        let row = self.alloc_row()?;
        self.pos_x[row] = (tx * SUBTILE + b.x_size * SUBTILE / 2).clamp(0, MAP_SPAN - 1);
        self.pos_y[row] = (ty * SUBTILE + b.y_size * SUBTILE / 2).clamp(0, MAP_SPAN - 1);
        {
            let a = &mut self.aux[row];
            a.owner = owner;
            a.is_building = true;
            a.type_id = type_id;
            a.tidx = bidx.max(0) as u16;
            a.tx = tx;
            a.ty = ty;
            // The footprint rides in order_x/order_y; a building has no move order.
            a.order_x = b.x_size;
            a.order_y = b.y_size;
            a.max_hits = b.hits.max(1);
            a.hits = if instant { b.hits.max(1) } else { (b.hits / 10).max(1) };
            a.build_progress = if instant { -1 } else { 0 };
            a.order_a = NO_TARGET;
            a.gather_res = -1;
            a.rally_x = -1;
            a.rally_y = -1;
        }
        self.stamp_building(row, !instant);
        if instant {
            // Completing re-stamps as a real blocker.
            self.stamp_building(row, false);
        }
        Some(self.handle_of_row[row] as i32)
    }

    // -- gathering -----------------------------------------------------------------------

    /// Which resource slot a tile yields, or `None`.
    ///
    /// **The gate is derived, the mapping is not.** `has_gather_access` decides whether a
    /// gatherer could reach the tile at all — that is `WorldData::has_gather_access`
    /// `0x006b4e50`, unmodified. Turning "trees" into timber and "mountain" into metal is
    /// [inferred] from the shipped building names (Woodcutter's Camp, Mine) and is this
    /// file's, which is why the client shows it in the coverage panel.
    pub fn tile_resource(&self, tx: i32, ty: i32) -> Option<usize> {
        if !self.terrain.valid_t(tx, ty) {
            return None;
        }
        if self.terrain.has_gather_access(tx, ty) {
            let m = self.terrain.tmask(tx, ty);
            if m & terr::tflag::SURFACE_MASK == terr::tflag::SURFACE_TREES {
                return Some(econ::RES_TIMBER);
            }
            return Some(econ::RES_METAL);
        }
        // A finished Farm or Oil Well makes its own tiles workable.
        let id = self.tile_obj[self.t_of(tx, ty)];
        if id >= 0 {
            if let Some(row) = self.row_of_id(id) {
                if self.aux[row].build_progress < 0 {
                    return match self.aux[row].type_id {
                        417 => Some(econ::RES_FOOD), // Farm
                        421 | 422 => Some(econ::RES_OIL), // Oil Well / Oil Platform
                        _ => None,
                    };
                }
            }
        }
        None
    }

    // -- the frame -----------------------------------------------------------------------

    fn build_owner_order(&mut self) {
        let n = self.live as usize;
        let mut counts = [0u32; OWNER_SLOTS + 1];
        for row in 0..n {
            counts[(self.aux[row].owner as usize) % OWNER_SLOTS + 1] += 1;
        }
        for k in 1..=OWNER_SLOTS {
            counts[k] += counts[k - 1];
        }
        self.owner_start[..=OWNER_SLOTS].copy_from_slice(&counts[..=OWNER_SLOTS]);
        let mut cursor = counts;
        for row in 0..n {
            let o = (self.aux[row].owner as usize) % OWNER_SLOTS;
            self.order[cursor[o] as usize] = row as u32;
            cursor[o] += 1;
        }
    }

    #[inline]
    fn cell_of(&self, x: i32, y: i32) -> usize {
        let d = self.grid_dim;
        let span = MAP_SPAN / d;
        let cx = (x / span).clamp(0, d - 1);
        let cy = (y / span).clamp(0, d - 1);
        (cy * d + cx) as usize
    }

    /// Bucket every live object into the uniform grid, by counting sort. One pass to count,
    /// one to place; no allocation.
    fn build_grid(&mut self) {
        let n = self.live as usize;
        let cells = (self.grid_dim * self.grid_dim) as usize;
        for c in self.cell_start[..=cells].iter_mut() {
            *c = 0;
        }
        for row in 0..n {
            let c = self.cell_of(self.pos_x[row], self.pos_y[row]);
            self.cell_start[c + 1] += 1;
        }
        for c in 1..=cells {
            self.cell_start[c] += self.cell_start[c - 1];
        }
        let mut cursor: Vec<u32> = self.cell_start[..cells].to_vec();
        for row in 0..n {
            let c = self.cell_of(self.pos_x[row], self.pos_y[row]);
            self.cell_rows[cursor[c] as usize] = row as u32;
            cursor[c] += 1;
        }
    }

    /// Nearest enemy object within `reach`, or -1. Ours, like `crate::real`'s, but scanning
    /// only the grid cells the reach can touch rather than the whole world.
    fn acquire(&self, row: usize, reach: i32) -> i32 {
        let (px, py) = (self.pos_x[row], self.pos_y[row]);
        let me = self.aux[row].owner;
        let r2 = (reach as i64) * (reach as i64);
        let d = self.grid_dim;
        let span = MAP_SPAN / d;
        let rad = (reach / span + 1).min(d);
        let ccx = (px / span).clamp(0, d - 1);
        let ccy = (py / span).clamp(0, d - 1);
        let mut best = -1i32;
        let mut best_d2 = i64::MAX;
        for gy in (ccy - rad).max(0)..=(ccy + rad).min(d - 1) {
            for gx in (ccx - rad).max(0)..=(ccx + rad).min(d - 1) {
                let c = (gy * d + gx) as usize;
                let (s, e) = (self.cell_start[c] as usize, self.cell_start[c + 1] as usize);
                for k in s..e {
                    let other = self.cell_rows[k] as usize;
                    if self.aux[other].owner == me || self.aux[other].hits <= 0 {
                        continue;
                    }
                    let dx = (self.pos_x[other] - px) as i64;
                    let dy = (self.pos_y[other] - py) as i64;
                    let d2 = dx * dx + dy * dy;
                    if d2 <= r2 && d2 < best_d2 {
                        best_d2 = d2;
                        best = self.handle_of_row[other] as i32;
                    }
                }
            }
        }
        best
    }

    /// One step of `speed` subtiles toward a point, sliding along a blocked tile rather
    /// than stopping dead. **Not the engine's pathfinder** — `PathFinder::astar_path` is
    /// ported in `don_sim::systems::movement` and is not called from here.
    fn move_towards(&mut self, row: usize, tx: i32, ty: i32, speed: i32) -> bool {
        let (px, py) = (self.pos_x[row], self.pos_y[row]);
        let dx = (tx - px) as i64;
        let dy = (ty - py) as i64;
        let d2 = dx * dx + dy * dy;
        if d2 == 0 {
            return true;
        }
        let dist = isqrt_i64(d2);
        self.aux[row].facing = atan2_u32(dy as i32, dx as i32) as i32;
        let s = speed.max(1) as i64;
        let (mut nx, mut ny) = if dist <= s {
            (tx as i64, ty as i64)
        } else {
            (px as i64 + (dx * s) / dist, py as i64 + (dy * s) / dist)
        };
        if self.blocked_at_sub(nx as i32, ny as i32) {
            // Try the two axis-aligned slides before giving up. Cheap, and it stops units
            // wedging themselves permanently on a forest edge.
            let ax = px as i64 + if dx > 0 { s } else if dx < 0 { -s } else { 0 };
            let ay = py as i64 + if dy > 0 { s } else if dy < 0 { -s } else { 0 };
            if !self.blocked_at_sub(ax as i32, py) {
                nx = ax;
                ny = py as i64;
            } else if !self.blocked_at_sub(px, ay as i32) {
                nx = px as i64;
                ny = ay;
            } else {
                // Last resort: try the four cardinals in the engine's `NEIGHBOUR4` order,
                // so a unit wedged in a pocket still finds its way out deterministically.
                let mut freed = false;
                for i in 0..4 {
                    let cx = px + terr::NEIGHBOUR4_DX[i] * s as i32;
                    let cy = py + terr::NEIGHBOUR4_DY[i] * s as i32;
                    if !self.blocked_at_sub(cx, cy) {
                        nx = cx as i64;
                        ny = cy as i64;
                        freed = true;
                        break;
                    }
                }
                if !freed {
                    return false;
                }
            }
        }
        self.pos_x[row] = (nx as i32).clamp(0, MAP_SPAN - 1);
        self.pos_y[row] = (ny as i32).clamp(0, MAP_SPAN - 1);
        dist <= s
    }

    #[inline]
    fn blocked_at_sub(&self, x: i32, y: i32) -> bool {
        let (tx, ty) = (x / SUBTILE, y / SUBTILE);
        if !self.terrain.valid_t(tx, ty) {
            return true;
        }
        self.terrain.tmask(tx, ty) & terr::tflag::BLOCKED != 0
    }

    /// Assemble the damage chain's inputs and run it. Same construction as
    /// `crate::real::strike`; only the type lookup differs, because a defender here may be
    /// a building.
    fn strike(&mut self, gd: &GameData, pd: &PlayData, row: usize, trow: usize, dir: u32) -> i32 {
        let (a_attack, a_type, a_dom, a_mask, a_ml, a_splash) = self.combat_stats(gd, pd, row);
        let (_, d_type, d_dom, d_mask, d_ml, _) = self.combat_stats(gd, pd, trow);
        let d_armor = self.armor_of(gd, pd, trow);
        let i = DamageInput {
            balance_pct: gd.balance_pct(a_type, d_type),
            attack: get_attack(a_attack, false, a_ml, gd.rules_0x8b8),
            armor: get_armor(d_armor, false, d_ml, gd.rules_0x8b8),
            attacker_masks: a_mask as u32,
            defender_masks: d_mask as u32,
            attack_dir: dir as i32,
            splash_flag: 0,
            overkill_gate: 0,
            attacker_player: self.aux[row].owner as u32,
            attacker_type_id: a_type,
            attacker_domain: a_dom,
            attacker_splash_percent: a_splash,
            attacker_type_0x40: 0,
            attacker_z: 0,
            attacker_flag8_bit5: false,
            defender_type_id: d_type,
            defender_domain: d_dom,
            defender_type_0x2b8_bit2: false,
            defender_splash_divisor: 1,
            defender_flags_0x68: 0,
            defender_flags_0x6c_bit12: false,
            defender_z: 0,
            defender_facing: self.aux[trow].facing,
            defender_facing_entrench: self.aux[trow].facing,
            defender_overkill_stamp: 0,
            defender_word_0xa4: 0,
            attacker_vf_0xe4: 0,
            current_frame: self.frame as i32,
            game_flag_0x821_bit1: false,
            tile_rocky: false,
            tile_owner: self.aux[row].owner as i32,
        };
        damage(&i, &predicates(), &gd.rules, &UnreachedTerms::default())
    }

    /// `(attack_x10, type_id, domain, obj_masks, military_level, splash_percent)`.
    fn combat_stats(
        &self,
        gd: &GameData,
        pd: &PlayData,
        row: usize,
    ) -> (i32, i32, i32, i32, i32, i32) {
        let a = &self.aux[row];
        if a.is_building {
            let b = pd.blds.get(a.tidx as usize).copied().unwrap_or_default();
            (b.attack, b.type_id, 0, b.obj_masks, 0, 0)
        } else {
            let u = gd.units[a.tidx as usize];
            (u.attack, u.type_id, u.domain, u.obj_masks, u.military_level, u.splash_percent)
        }
    }
    fn armor_of(&self, gd: &GameData, pd: &PlayData, row: usize) -> i32 {
        let a = &self.aux[row];
        if a.is_building {
            pd.blds.get(a.tidx as usize).map(|b| b.armor).unwrap_or(0)
        } else {
            gd.units[a.tidx as usize].armor
        }
    }
    /// `(reach in subtiles, recharge frames, move speed in subtiles/frame)`.
    fn kinetics(&self, gd: &GameData, pd: &PlayData, row: usize) -> (i32, i32, i32) {
        let a = &self.aux[row];
        if a.is_building {
            let b = pd.blds.get(a.tidx as usize).copied().unwrap_or_default();
            (b.max_range.max(0) * SUBTILE, b.recharge.max(1), 0)
        } else {
            let u = gd.units[a.tidx as usize];
            let reach = if u.max_range > 0 { u.max_range * SUBTILE } else { SUBTILE };
            (reach, u.recharge.max(1), u.moves.max(1))
        }
    }

    fn process(&mut self, gd: &GameData, pd: &PlayData, row: usize) {
        if self.aux[row].hits <= 0 {
            return;
        }
        if self.aux[row].cooldown > 0 {
            self.aux[row].cooldown -= 1;
        }
        let (reach, recharge, speed) = self.kinetics(gd, pd, row);

        // Buildings: run the production queue, then shoot if they can.
        if self.aux[row].is_building {
            self.tick_building(gd, pd, row);
            if self.aux[row].build_progress >= 0 {
                return;
            }
            if reach > 0 {
                let t = self.acquire(row, reach);
                if let Some(trow) = self.row_of_id(t) {
                    let dx = self.pos_x[trow] - self.pos_x[row];
                    let dy = self.pos_y[trow] - self.pos_y[row];
                    let dir = atan2_u32(dy, dx);
                    if self.aux[row].cooldown == 0 {
                        let d = self.strike(gd, pd, row, trow, dir);
                        if d > 0 {
                            self.aux[trow].hits -= d;
                        }
                        self.aux[row].cooldown = recharge.clamp(1, i16::MAX as i32) as i16;
                    }
                }
            }
            return;
        }

        match self.aux[row].order {
            ord::GATHER => {
                let t = self.aux[row].order_a;
                let (tx, ty) = (t % MAP_TILES, t / MAP_TILES);
                // A mountain tile is BLOCKED, so stand on the approach tile instead. The
                // ring and its order are the engine's — `NEIGHBOUR4_DX/DY`, the same four
                // offsets `WorldData::has_gather_access` walks to decide the node is
                // reachable at all.
                let (mut sx, mut sy) = (tx, ty);
                if self.terrain.tmask(tx, ty) & terr::tflag::BLOCKED != 0 {
                    for i in 0..4 {
                        let (nx, ny) =
                            (tx + terr::NEIGHBOUR4_DX[i], ty + terr::NEIGHBOUR4_DY[i]);
                        if self.terrain.valid_t(nx, ny)
                            && self.terrain.tmask(nx, ny) & terr::tflag::BLOCKED == 0
                        {
                            sx = nx;
                            sy = ny;
                            break;
                        }
                    }
                }
                let (gx, gy) = (sx * SUBTILE + SUBTILE / 2, sy * SUBTILE + SUBTILE / 2);
                let dx = (gx - self.pos_x[row]) as i64;
                let dy = (gy - self.pos_y[row]) as i64;
                // 2.5 tiles: a mountain tile is BLOCKED, so a worker sent to one stops at
                // its edge and must still count as working it.
                let arrive = (SUBTILE as i64 * 5) / 2;
                if dx * dx + dy * dy <= arrive * arrive {
                    self.aux[row].gather_res =
                        self.tile_resource(tx, ty).map(|r| r as i8).unwrap_or(-1);
                    if self.aux[row].gather_res < 0 {
                        self.aux[row].order = ord::NONE;
                    }
                } else {
                    self.aux[row].gather_res = -1;
                    let moved = self.move_towards(row, gx, gy, speed);
                    self.note_progress(row, moved);
                }
            }
            ord::BUILD => {
                let target = self.aux[row].order_a;
                let Some(brow) = self.row_of_id(target) else {
                    self.aux[row].order = ord::NONE;
                    return;
                };
                if self.aux[brow].build_progress < 0 {
                    self.aux[row].order = ord::NONE;
                    return;
                }
                let (bx, by) = (self.pos_x[brow], self.pos_y[brow]);
                let dx = (bx - self.pos_x[row]) as i64;
                let dy = (by - self.pos_y[row]) as i64;
                let span = (self.aux[brow].order_x.max(self.aux[brow].order_y) + 2) * SUBTILE;
                if dx * dx + dy * dy <= (span as i64) * (span as i64) {
                    self.aux[brow].build_progress += 1;
                    let b = pd.blds.get(self.aux[brow].tidx as usize).copied().unwrap_or_default();
                    let jt = b.job_time.max(1);
                    let p = self.aux[brow].build_progress;
                    self.aux[brow].hits =
                        ((b.hits as i64 * p.min(jt) as i64) / jt as i64).max(1) as i32;
                    if p >= jt {
                        self.aux[brow].build_progress = -1;
                        self.aux[brow].hits = b.hits.max(1);
                        self.stamp_building(brow, false);
                        self.aux[row].order = ord::NONE;
                        self.players[self.aux[row].owner as usize % PLAYERS].dirty = true;
                    }
                } else {
                    let moved = self.move_towards(row, bx, by, speed);
                    self.note_progress(row, moved);
                }
            }
            ord::ATTACK => {
                let t = self.aux[row].order_a;
                let Some(trow) = self.row_of_id(t) else {
                    self.aux[row].order = ord::NONE;
                    return;
                };
                self.engage(gd, pd, row, trow, reach, recharge, speed);
            }
            ord::MOVE => {
                let (ox, oy) = (self.aux[row].order_x, self.aux[row].order_y);
                let (px, py) = (self.pos_x[row], self.pos_y[row]);
                if self.move_towards(row, ox, oy, speed) {
                    self.aux[row].order = ord::NONE;
                    self.aux[row].stuck = 0;
                } else {
                    let moved = self.pos_x[row] != px || self.pos_y[row] != py;
                    self.note_progress(row, moved);
                }
            }
            _ => {
                // Idle military auto-acquires within its own reach; a civilian does not.
                let (attack, ..) = self.combat_stats(gd, pd, row);
                if attack > 0 {
                    let t = self.acquire(row, reach.max(SUBTILE * 6));
                    if let Some(trow) = self.row_of_id(t) {
                        self.engage(gd, pd, row, trow, reach, recharge, speed);
                    }
                }
            }
        }
    }

    /// Give up on an order that straight-line movement cannot make progress on, and count
    /// it. The alternative — a unit shivering against a cliff forever — is the exact
    /// failure mode a client must not hide.
    #[inline]
    fn note_progress(&mut self, row: usize, moved: bool) {
        if moved {
            self.aux[row].stuck = 0;
        } else {
            self.aux[row].stuck = self.aux[row].stuck.saturating_add(1);
            if self.aux[row].stuck > 90 {
                self.aux[row].order = ord::NONE;
                self.aux[row].stuck = 0;
                self.gaps[gap::PATH_STUCK] += 1;
            }
        }
    }

    fn engage(
        &mut self,
        gd: &GameData,
        pd: &PlayData,
        row: usize,
        trow: usize,
        reach: i32,
        recharge: i32,
        speed: i32,
    ) {
        let dx = self.pos_x[trow] - self.pos_x[row];
        let dy = self.pos_y[trow] - self.pos_y[row];
        let d2 = (dx as i64) * (dx as i64) + (dy as i64) * (dy as i64);
        let span = reach + self.aux[trow].order_x.max(1) * SUBTILE / 2;
        if d2 <= (span as i64) * (span as i64) {
            let dir = atan2_u32(dy, dx);
            self.aux[row].facing = dir as i32;
            if self.aux[row].cooldown == 0 {
                let d = self.strike(gd, pd, row, trow, dir);
                if d > 0 {
                    self.aux[trow].hits -= d;
                }
                self.aux[row].cooldown = recharge.clamp(1, i16::MAX as i32) as i16;
            }
        } else {
            let (tx, ty) = (self.pos_x[trow], self.pos_y[trow]);
            self.move_towards(row, tx, ty, speed);
        }
    }

    /// Advance a building's queue and its age research.
    fn tick_building(&mut self, gd: &GameData, pd: &PlayData, row: usize) {
        if self.aux[row].build_progress >= 0 || self.aux[row].queue_n == 0 {
            return;
        }
        let head = self.aux[row].queue[0];
        let owner = self.aux[row].owner as usize % PLAYERS;
        let jt = pd.unit(head).map(|u| u.job_time.max(1)).unwrap_or(60);
        self.aux[row].queue_prog += 1;
        if self.aux[row].queue_prog < jt {
            return;
        }
        // Spawn at the rally point if there is one, else just outside the footprint.
        let (bx, by) = (self.pos_x[row], self.pos_y[row]);
        let out = (self.aux[row].order_x.max(1) * SUBTILE) / 2 + SUBTILE;
        let jitter = (self.rand_below(5) as i32 - 2) * SUBTILE / 2;
        let id = self.spawn_unit(gd, owner as u8, head, bx + jitter, by + out);
        if id < 0 {
            return; // capacity: leave it queued and try again next frame
        }
        if self.aux[row].rally_x >= 0 {
            if let Some(nrow) = self.row_of_id(id) {
                self.aux[nrow].order = ord::MOVE;
                self.aux[nrow].order_x = self.aux[row].rally_x;
                self.aux[nrow].order_y = self.aux[row].rally_y;
            }
        }
        let n = self.aux[row].queue_n as usize;
        for k in 1..n {
            self.aux[row].queue[k - 1] = self.aux[row].queue[k];
        }
        self.aux[row].queue_n -= 1;
        self.aux[row].queue_prog = 0;
        self.players[owner].dirty = true;
    }

    /// One simulation frame.
    pub fn step(&mut self, gd: &GameData, pd: &PlayData) {
        self.frame += 1;
        self.build_grid();
        self.build_owner_order();
        // `Objects::process_all` `0x0065DCE0` rotates the owner slots every frame.
        for slot in 0..OWNER_SLOTS {
            let o = (self.frame as usize + slot) % OWNER_SLOTS;
            let s = self.owner_start[o] as usize;
            let e = self.owner_start[o + 1] as usize;
            for k in s..e {
                let row = self.order[k] as usize;
                self.process(gd, pd, row);
            }
        }
        let mut row = 0usize;
        while row < self.live as usize {
            if self.aux[row].hits <= 0 {
                self.despawn_row(row);
            } else {
                row += 1;
            }
        }
        self.tick_research(pd);
        self.tick_economy(gd, pd);
        self.refresh_tags(gd, pd);
    }

    fn tick_research(&mut self, pd: &PlayData) {
        for p in 0..PLAYERS {
            if self.players[p].researching < 0 {
                continue;
            }
            self.players[p].research_prog += 1;
            // Age techs have no JOB_TIME column in the tech table we packed; 600 frames is
            // the shipped `VILLAGE` JOB_TIME and is used here as a stand-in. Ours.
            if self.players[p].research_prog >= 600 {
                self.players[p].econ.age = (self.players[p].econ.age + 1).min(7);
                self.players[p].econ.age_alt = self.players[p].econ.age;
                self.players[p].researching = -1;
                self.players[p].research_prog = 0;
                self.players[p].dirty = true;
            }
        }
        let _ = pd;
    }

    /// The economy tick: compose `object_income`, then run the derived `Leader::gather`.
    fn tick_economy(&mut self, gd: &GameData, pd: &PlayData) {
        let _ = gd;
        let rules = &pd.rules;
        let rate_land = econ::worker_rate(rules, false) * 16;
        let rate_oil = econ::worker_rate(rules, true) * 16;

        let mut income = [[0i32; econ::NUM_RESOURCES]; PLAYERS];
        let mut workers = [[0i32; econ::NUM_RESOURCES]; PLAYERS];
        let mut pop = [0i32; PLAYERS];
        let mut nb = [0i32; PLAYERS];
        let mut nu = [0i32; PLAYERS];

        for row in 0..self.live as usize {
            let a = self.aux[row];
            let p = a.owner as usize % PLAYERS;
            if a.is_building {
                nb[p] += 1;
                if a.build_progress < 0 {
                    // Cities pay `CITY_GATHER` through the engine's own city function.
                    if (414..=416).contains(&a.type_id) {
                        let c = econ::CityResourceInputs::default();
                        let y = econ::calc_city_resources(rules, Some(&c));
                        for i in 0..econ::NUM_RESOURCES {
                            income[p][i] = income[p][i].wrapping_add(y[i]);
                        }
                    }
                }
                continue;
            }
            nu[p] += 1;
            pop[p] += pd.unit(a.type_id).map(|u| u.pop.max(0)).unwrap_or(1).max(if pd.is_real {
                0
            } else {
                1
            });
            if a.gather_res >= 0 {
                let r = a.gather_res as usize;
                workers[p][r] += 1;
                income[p][r] = income[p][r]
                    .wrapping_add(if r == econ::RES_OIL { rate_oil } else { rate_land });
            }
        }

        for p in 0..PLAYERS {
            let pl = &mut self.players[p];
            pl.pop = pop[p];
            pl.workers = workers[p];
            pl.buildings = nb[p];
            pl.units = nu[p];
            pl.pop_cap = rules
                .at(OFF_POP_CAP + self.pop_cap_setting.min(7) * 4)
                .clamp(0, rules.at(OFF_MAX_POP_LIMIT).max(1));
            let gi = econ::GatherInputs {
                object_income: income[p],
                // `total_land_tiles` is a divisor: zero disables the territory-tax term,
                // which is what we want, because territory is not modelled.
                total_land_tiles: 0,
                ..Default::default()
            };
            // `Leader::gather` `0x006CE280`, inlined rather than called through
            // `leader_gather`, for one reason: the commerce-cap clamp lives *inside*
            // `do_gather`, and the improved mode has to be able to lift it between
            // `calc_resource_caps` and `do_gather`. Every step below is the derived
            // function, in retail's order; the only deviation is the one `income_mode`
            // names, and it is a single assignment.
            let g = econ::calc_gather(rules, &gi);
            pl.econ.gross = g.gross;
            pl.gross = g.gross;
            pl.econ.breakdown = [0; econ::NUM_RESOURCES];
            pl.econ.expense = [0; econ::NUM_RESOURCES];
            pl.econ.commerce_cap =
                econ::calc_resource_caps(rules, pl.econ.age, &econ::CapGates::default());
            if self.income_mode == 1 {
                pl.econ.commerce_cap = [i32::MAX / 4; econ::NUM_RESOURCES];
            }
            let payouts = econ::do_gather(rules, &mut pl.econ, &econ::DoGatherContext::default());
            pl.last_calc = self.frame as i32;
            pl.dirty = false;
            for i in 0..econ::NUM_RESOURCES {
                pl.income[i] = payouts[i].displayed;
            }
        }
    }

    /// Recompute the whole derived block after a structural change (used at construction).
    fn refresh_derived(&mut self, gd: &GameData, pd: &PlayData) {
        self.tick_economy(gd, pd);
        self.refresh_tags(gd, pd);
    }

    /// Per-instance render tag. One `u32` carries everything the vertex shader needs.
    ///
    /// | bits | meaning |
    /// |---|---|
    /// | 31 | occupied |
    /// | 30 | selected |
    /// | 29 | building |
    /// | 28 | under construction |
    /// | 24..27 | size class (footprint tiles for a building, bulk class for a unit) |
    /// | 16..23 | hit points, 0..255 of maximum |
    /// | 8..15 | type hue, the low byte of the real `type_id` |
    /// | 4..7 | carrying: gathered resource slot + 1, or 0 |
    /// | 0..3 | owner |
    pub fn refresh_tags(&mut self, gd: &GameData, pd: &PlayData) {
        for row in 0..self.live as usize {
            let a = self.aux[row];
            let hp = if a.max_hits > 0 {
                ((a.hits.max(0) as i64 * 255) / a.max_hits as i64) as u32
            } else {
                0
            };
            let size = if a.is_building {
                (a.order_x.max(a.order_y).clamp(1, 15)) as u32
            } else {
                let u = gd.units[a.tidx as usize];
                (2 + (u.hits / 90).clamp(0, 5)) as u32
            };
            let carry = if a.gather_res >= 0 { (a.gather_res as u32 + 1) & 0xf } else { 0 };
            self.tag[row] = 0x8000_0000
                | ((a.selected as u32) << 30)
                | ((a.is_building as u32) << 29)
                | (((a.build_progress >= 0) as u32) << 28)
                | ((size & 0xf) << 24)
                | ((hp & 0xff) << 16)
                | (((a.type_id as u32) & 0xff) << 8)
                | (carry << 4)
                | (a.owner as u32 & 0xf);
        }
        for slot in self.live as usize..self.capacity as usize {
            self.tag[slot] = 0;
        }
        let _ = pd;
    }

    // -- commands --------------------------------------------------------------------------

    /// `GroupCommand` (0x00). Replaces `who`'s selection.
    pub fn cmd_group(&mut self, who: u8, ids: impl Iterator<Item = i16>) -> u32 {
        for r in 0..self.live as usize {
            if self.aux[r].owner == who {
                self.aux[r].selected = false;
            }
        }
        let mut n = 0;
        for id in ids {
            if let Some(row) = self.row_of_id(id as i32) {
                if self.aux[row].owner == who {
                    self.aux[row].selected = true;
                    n += 1;
                }
            }
        }
        if n == 0 {
            self.gaps[gap::NO_SELECTION] += 1;
        }
        n
    }

    /// `MoveToCommand` (0x07).
    pub fn cmd_move_to(&mut self, who: u8, tx: i32, ty: i32) -> u32 {
        let mut n = 0;
        for r in 0..self.live as usize {
            if self.aux[r].owner == who && self.aux[r].selected && !self.aux[r].is_building {
                self.aux[r].order = ord::MOVE;
                self.aux[r].order_x = tx.clamp(0, MAP_SPAN - 1);
                self.aux[r].order_y = ty.clamp(0, MAP_SPAN - 1);
                self.aux[r].order_a = NO_TARGET;
                self.aux[r].gather_res = -1;
                n += 1;
            } else if self.aux[r].owner == who && self.aux[r].selected {
                // A selected building takes the point as its rally point instead.
                self.aux[r].rally_x = tx.clamp(0, MAP_SPAN - 1);
                self.aux[r].rally_y = ty.clamp(0, MAP_SPAN - 1);
                n += 1;
            }
        }
        if n == 0 {
            self.gaps[gap::NO_SELECTION] += 1;
        }
        n
    }

    /// `AttackCommand` (0x04).
    pub fn cmd_attack(&mut self, who: u8, whom: i32) -> u32 {
        if self.row_of_id(whom).is_none() {
            return 0;
        }
        let mut n = 0;
        for r in 0..self.live as usize {
            if self.aux[r].owner == who && self.aux[r].selected && !self.aux[r].is_building {
                self.aux[r].order = ord::ATTACK;
                self.aux[r].order_a = whom;
                self.aux[r].gather_res = -1;
                n += 1;
            }
        }
        if n == 0 {
            self.gaps[gap::NO_SELECTION] += 1;
        }
        n
    }

    /// `HaltCommand` (0x0c).
    pub fn cmd_halt(&mut self, who: u8) -> u32 {
        let mut n = 0;
        for r in 0..self.live as usize {
            if self.aux[r].owner == who && self.aux[r].selected {
                self.aux[r].order = ord::NONE;
                self.aux[r].order_a = NO_TARGET;
                self.aux[r].gather_res = -1;
                n += 1;
            }
        }
        n
    }

    /// `GatherCommand` (0x13). `ox` carries the tile index of the node here — retail's
    /// `ox` is an object index into a `Good`, which this world does not model, and the
    /// difference is recorded rather than papered over.
    pub fn cmd_gather(&mut self, who: u8, tile: i32) -> u32 {
        if tile < 0 || tile >= MAP_TILES * MAP_TILES {
            self.gaps[gap::NOT_GATHERABLE] += 1;
            return 0;
        }
        let (tx, ty) = (tile % MAP_TILES, tile / MAP_TILES);
        if self.tile_resource(tx, ty).is_none() {
            self.gaps[gap::NOT_GATHERABLE] += 1;
            return 0;
        }
        let mut n = 0;
        for r in 0..self.live as usize {
            if self.aux[r].owner == who && self.aux[r].selected && !self.aux[r].is_building {
                self.aux[r].order = ord::GATHER;
                self.aux[r].order_a = tile;
                n += 1;
            }
        }
        if n == 0 {
            self.gaps[gap::NO_WORKER] += 1;
        }
        n
    }

    /// `BuildCommand` (0x19). `x`/`y` are tile coordinates of the footprint's top-left.
    pub fn cmd_build(&mut self, pd: &PlayData, who: u8, tx: i32, ty: i32, type_id: i32) -> u32 {
        let Some(b) = pd.bld(type_id).copied() else {
            self.gaps[gap::UNKNOWN_OPCODE] += 1;
            return 0;
        };
        let p = who as usize % PLAYERS;
        if !self.can_afford(p, &b.cost) {
            self.gaps[gap::CANNOT_AFFORD] += 1;
            return 0;
        }
        if self.grade_placement(pd, who as i32, type_id, tx, ty) == terr::space::CORE_BLOCKED {
            self.gaps[gap::PLACEMENT_BLOCKED] += 1;
            return 0;
        }
        // Only a builder can start one, so a build with nothing selected is a visible gap.
        let builders: Vec<usize> = (0..self.live as usize)
            .filter(|&r| {
                self.aux[r].owner == who && self.aux[r].selected && !self.aux[r].is_building
            })
            .collect();
        if builders.is_empty() {
            self.gaps[gap::NO_WORKER] += 1;
            return 0;
        }
        let Some(id) = self.place_building(pd, who, type_id, tx, ty, false) else {
            self.gaps[gap::PLACEMENT_BLOCKED] += 1;
            return 0;
        };
        self.pay(p, &b.cost);
        for r in builders {
            self.aux[r].order = ord::BUILD;
            self.aux[r].order_a = id;
            self.aux[r].gather_res = -1;
        }
        1
    }

    /// `QueueUpCommand` (0x18) — train a unit, or start an age.
    ///
    /// Retail applies this to the player's current selection; so does this. The producer
    /// legality test is the `WHERE` join: a Barracks can make exactly the 118 types whose
    /// `WHERE` names it.
    pub fn cmd_queue_up(&mut self, pd: &PlayData, who: u8, type_id: i32, num: i32) -> u32 {
        let p = who as usize % PLAYERS;
        if (AGE_TECH_BASE..AGE_TECH_BASE + 7).contains(&type_id) {
            return self.start_age(pd, p, type_id);
        }
        let Some(u) = pd.unit(type_id).copied() else {
            self.gaps[gap::UNKNOWN_OPCODE] += 1;
            return 0;
        };
        let mut queued = 0;
        for _ in 0..num.clamp(1, QUEUE_MAX as i32) {
            let mut placed = false;
            for r in 0..self.live as usize {
                if self.aux[r].owner != who
                    || !self.aux[r].selected
                    || !self.aux[r].is_building
                    || self.aux[r].build_progress >= 0
                {
                    continue;
                }
                if self.aux[r].type_id != u.where_ {
                    continue;
                }
                if self.aux[r].queue_n as usize >= QUEUE_MAX {
                    self.gaps[gap::QUEUE_FULL] += 1;
                    continue;
                }
                if !self.can_afford(p, &u.cost) {
                    self.gaps[gap::CANNOT_AFFORD] += 1;
                    return queued;
                }
                let pop_cost = u.pop.max(0);
                if self.players[p].pop + pop_cost > self.players[p].pop_cap {
                    self.gaps[gap::POP_CAPPED] += 1;
                    return queued;
                }
                self.pay(p, &u.cost);
                let n = self.aux[r].queue_n as usize;
                self.aux[r].queue[n] = type_id;
                self.aux[r].queue_n += 1;
                queued += 1;
                placed = true;
                break;
            }
            if !placed {
                self.gaps[gap::NOT_A_PRODUCER] += 1;
                break;
            }
        }
        queued
    }

    /// `UnqueueCommand` (0x30) — cancel the last queued item on a selected producer and
    /// refund it, which is the only honest thing to do when the cost was taken up front.
    pub fn cmd_unqueue(&mut self, pd: &PlayData, who: u8, type_id: i32) -> u32 {
        let p = who as usize % PLAYERS;
        for r in 0..self.live as usize {
            if self.aux[r].owner != who || !self.aux[r].selected || !self.aux[r].is_building {
                continue;
            }
            let n = self.aux[r].queue_n as usize;
            for k in (0..n).rev() {
                if type_id < 0 || self.aux[r].queue[k] == type_id {
                    let t = self.aux[r].queue[k];
                    if let Some(u) = pd.unit(t) {
                        let cost = u.cost;
                        self.refund(p, &cost);
                    }
                    for j in k + 1..n {
                        self.aux[r].queue[j - 1] = self.aux[r].queue[j];
                    }
                    self.aux[r].queue_n -= 1;
                    if k == 0 {
                        self.aux[r].queue_prog = 0;
                    }
                    return 1;
                }
            }
        }
        0
    }

    fn start_age(&mut self, pd: &PlayData, p: usize, type_id: i32) -> u32 {
        let want = type_id - AGE_TECH_BASE; // 0 = Classical, i.e. advancing to age 1
        if want != self.players[p].econ.age || self.players[p].researching >= 0 {
            self.gaps[gap::WRONG_AGE] += 1;
            return 0;
        }
        let Some(cost) = pd.age_cost.get(want as usize).copied() else {
            self.gaps[gap::WRONG_AGE] += 1;
            return 0;
        };
        if !self.can_afford(p, &cost) {
            self.gaps[gap::CANNOT_AFFORD] += 1;
            return 0;
        }
        self.pay(p, &cost);
        self.players[p].researching = type_id;
        self.players[p].research_prog = 0;
        1
    }

    #[inline]
    fn can_afford(&self, p: usize, cost: &[i32; econ::NUM_RESOURCES]) -> bool {
        (0..econ::NUM_RESOURCES).all(|i| self.players[p].econ.stockpile[i] >= cost[i])
    }
    #[inline]
    fn pay(&mut self, p: usize, cost: &[i32; econ::NUM_RESOURCES]) {
        for i in 0..econ::NUM_RESOURCES {
            self.players[p].econ.stockpile[i] -= cost[i];
        }
        self.players[p].dirty = true;
    }
    #[inline]
    fn refund(&mut self, p: usize, cost: &[i32; econ::NUM_RESOURCES]) {
        for i in 0..econ::NUM_RESOURCES {
            self.players[p].econ.stockpile[i] += cost[i];
        }
    }

    /// **Benchmark hook.** Spawn `n` units of `type_id` for `owner` in a ring around their
    /// start position, bypassing cost and population. It exists so a frame-rate figure can
    /// be quoted at a realistic army size without first playing an hour; nothing in the
    /// command path can reach it, and the client labels any run that used it.
    pub fn debug_spawn(&mut self, gd: &GameData, owner: u8, type_id: i32, n: u32) -> u32 {
        let p = (owner as usize) % PLAYERS;
        let (cx, cy) = (self.start_x[p], self.start_y[p]);
        let mut made = 0;
        for k in 0..n {
            let ang = ((k as u64 * 2654435761) % 0x1_0000_0000) as u32;
            let r = SUBTILE * (4 + (k % 40) as i32);
            let (dx, dy) = (
                ((ang >> 16) as i32 % 2001 - 1000) * r / 1000,
                ((ang & 0xffff) as i32 % 2001 - 1000) * r / 1000,
            );
            if self.spawn_unit(gd, owner, type_id, cx + dx, cy + dy) >= 0 {
                made += 1;
            } else {
                break;
            }
        }
        made
    }

    // -- queries -----------------------------------------------------------------------

    /// Object ids of `who`'s objects inside an axis-aligned subtile box.
    pub fn pick_box(&self, who: u8, x0: i32, y0: i32, x1: i32, y1: i32, out: &mut [i16]) -> usize {
        let (lx, hx) = (x0.min(x1), x0.max(x1));
        let (ly, hy) = (y0.min(y1), y0.max(y1));
        let mut n = 0;
        // Units first, so a box over a base selects the army rather than the walls.
        for pass in 0..2 {
            for row in 0..self.live as usize {
                if n >= out.len() {
                    return n;
                }
                let a = &self.aux[row];
                if a.owner != who || a.is_building != (pass == 1) {
                    continue;
                }
                if self.pos_x[row] >= lx
                    && self.pos_x[row] <= hx
                    && self.pos_y[row] >= ly
                    && self.pos_y[row] <= hy
                {
                    out[n] = self.handle_of_row[row] as i16;
                    n += 1;
                }
            }
            if pass == 0 && n > 0 {
                return n;
            }
        }
        n
    }

    /// Every object of `who` whose type id matches — the double-click "select all of type".
    pub fn pick_type(&self, who: u8, type_id: i32, out: &mut [i16]) -> usize {
        let mut n = 0;
        for row in 0..self.live as usize {
            if n >= out.len() {
                break;
            }
            if self.aux[row].owner == who && self.aux[row].type_id == type_id {
                out[n] = self.handle_of_row[row] as i16;
                n += 1;
            }
        }
        n
    }

    /// The object nearest a point, any owner, preferring units over buildings within a
    /// tile — a click on a garrison should grab the soldier standing on it.
    pub fn pick_at(&self, x: i32, y: i32) -> i32 {
        let mut best = -1i32;
        let mut best_d2 = i64::MAX;
        for row in 0..self.live as usize {
            let a = &self.aux[row];
            let half = if a.is_building {
                a.order_x.max(a.order_y).max(1) * SUBTILE / 2
            } else {
                SUBTILE / 2
            };
            let dx = (self.pos_x[row] - x) as i64;
            let dy = (self.pos_y[row] - y) as i64;
            let d2 = dx * dx + dy * dy;
            let bias = if a.is_building { (half as i64) * (half as i64) } else { 0 };
            if d2 <= (half as i64 + SUBTILE as i64) * (half as i64 + SUBTILE as i64)
                && d2 + bias < best_d2
            {
                best_d2 = d2 + bias;
                best = self.handle_of_row[row] as i32;
            }
        }
        best
    }

    /// Order-independent digest, same construction as `crate::real`'s.
    pub fn digest(&self) -> u64 {
        let mut acc: u64 = 0;
        for row in 0..self.live as usize {
            let a = &self.aux[row];
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for v in [
                self.handle_of_row[row] as u64,
                self.pos_x[row] as u32 as u64,
                self.pos_y[row] as u32 as u64,
                a.hits as u32 as u64,
                a.type_id as u32 as u64,
                a.owner as u64,
                a.order as u64,
                a.order_a as u32 as u64,
                a.build_progress as u32 as u64,
            ] {
                h ^= v;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
            acc = acc.wrapping_add(h);
        }
        for p in &self.players {
            for i in 0..econ::NUM_RESOURCES {
                acc = acc
                    .wrapping_mul(0x0000_0100_0000_01B3)
                    .wrapping_add(p.econ.stockpile[i] as u32 as u64);
            }
            acc = acc.wrapping_add(p.econ.age as u64);
        }
        acc ^ self.frame ^ ((self.live as u64) << 40)
    }
}

/// Integer square root of a non-negative `i64`. No float in the result path.
#[inline]
fn isqrt_i64(v: i64) -> i64 {
    if v <= 0 {
        return 0;
    }
    let mut x = (v as f64).sqrt() as i64;
    for _ in 0..4 {
        if x <= 0 {
            x = 1;
        }
        let nx = (x + v / x) >> 1;
        if nx == x {
            break;
        }
        x = nx;
    }
    while x * x > v {
        x -= 1;
    }
    while (x + 1) * (x + 1) <= v {
        x += 1;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> (GameData, PlayData, GameWorld) {
        let gd = GameData::synthetic();
        let pd = PlayData::empty();
        let w = GameWorld::new(&gd, &pd, 0xC0FFEE);
        (gd, pd, w)
    }

    #[test]
    fn a_new_world_has_objects_and_terrain() {
        let (_gd, _pd, w) = world();
        assert!(w.live_count() > 0, "no starting objects");
        let mut trees = 0;
        let mut blocked = 0;
        for ty in 0..MAP_TILES {
            for tx in 0..MAP_TILES {
                let m = w.terrain.tmask(tx, ty);
                if m & terr::tflag::SURFACE_MASK == terr::tflag::SURFACE_TREES {
                    trees += 1;
                }
                if m & terr::tflag::BLOCKED != 0 {
                    blocked += 1;
                }
            }
        }
        assert!(trees > 100, "generated map has {trees} tree tiles");
        assert!(blocked > 100, "generated map has {blocked} blocked tiles");
    }

    #[test]
    fn placement_uses_the_engine_grades() {
        let (_gd, _pd, mut w) = world();
        // A tile we blocked ourselves must grade CORE_BLOCKED for any footprint over it.
        w.terrain.set_blocked_at(60, 60, true);
        w.terrain.set_blocked_at(61, 61, true);
        assert_eq!(w.terrain.space_at_corner(59, 59, 0, false), terr::space::CORE_BLOCKED);
    }

    #[test]
    fn gather_access_gates_the_resource_map() {
        let (_gd, _pd, mut w) = world();
        // A lone tree with a clear neighbour is gatherable; the engine's predicate says so.
        w.terrain.set_tree_at(70, 70, true);
        assert!(w.terrain.has_gather_access(70, 70));
        assert_eq!(w.tile_resource(70, 70), Some(econ::RES_TIMBER));
        w.terrain.set_mountain_at(72, 70, true);
        assert_eq!(w.tile_resource(72, 70), Some(econ::RES_METAL));
        // Bare ground is not.
        assert_eq!(w.tile_resource(64, 64), None);
    }

    #[test]
    fn stepping_is_deterministic_and_pays_the_derived_city_income() {
        let gd = GameData::synthetic();
        let pd = PlayData::empty();
        let run = || {
            let mut w = GameWorld::new(&gd, &pd, 7);
            for _ in 0..600 {
                w.step(&gd, &pd);
            }
            (w.digest(), w.players[0].econ.stockpile, w.live_count())
        };
        let a = run();
        let b = run();
        assert_eq!(a.0, b.0, "same seed must give the same digest");
        assert_eq!(a.1, b.1);
        assert_eq!(a.2, b.2);
    }

    #[test]
    fn a_worker_ordered_onto_trees_moves_income() {
        let gd = GameData::synthetic();
        // `EconRules::shipped()` carries CITY_GATHER and PEASANT_RATE, so a gather is
        // measurable even without the packed live block.
        let pd = PlayData::empty();
        let mut w = GameWorld::new(&gd, &pd, 3);
        // Find a citizen and a tree tile, and put the citizen on it.
        let (sx, sy) = GameWorld::start_tile(0);
        let mut tile = -1;
        'find: for r in 0..24 {
            for dy in -r..=r {
                for dx in -r..=r {
                    let (tx, ty) = (sx + dx, sy + dy);
                    if w.tile_resource(tx, ty) == Some(econ::RES_TIMBER) {
                        tile = ty * MAP_TILES + tx;
                        break 'find;
                    }
                }
            }
        }
        assert!(tile >= 0, "no timber tile near the start position");
        let ids: Vec<i16> = (0..w.live_count() as usize)
            .filter(|&r| w.aux[r].owner == 0 && !w.aux[r].is_building)
            .map(|r| w.id_of_row(r) as i16)
            .collect();
        assert!(!ids.is_empty());
        w.cmd_group(0, ids.into_iter());
        assert!(w.cmd_gather(0, tile) > 0, "no worker took the gather order");
        let before = w.players[0].econ.stockpile[econ::RES_TIMBER];
        for _ in 0..1200 {
            w.step(&gd, &pd);
        }
        let after = w.players[0].econ.stockpile[econ::RES_TIMBER];
        assert!(w.players[0].workers[econ::RES_TIMBER] > 0, "nobody is on the node");
        assert!(after > before, "timber did not move: {before} -> {after}");
    }
}
