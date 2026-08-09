//! `Game::do_frame` as an **executable** schedule: the 29 ordered subsystem calls of
//! `0x00591EF0`, driving the ported `systems/*` modules.
//!
//! # What this file is for
//!
//! [`crate::schedule::DO_FRAME`] has always held the tick as *data* — 29 entries with a VA
//! and a status. Nothing executed it. [`crate::world::World::step`] visits the 29 counters
//! and runs one of them ([`crate::objects`]'s traversal), which is the *ordering* of the
//! tick without its *content*; `docs/mechanics/COVERAGE.md` §5 names that as the finding
//! that dominates all others.
//!
//! [`Sim`] is the missing driver. It owns the state the `systems/*` modules need and calls
//! them from the retail step they belong to, in retail's order. Every step reports one of
//! four outcomes and the outcomes are counted, so what ran is **measured, not asserted**:
//!
//! | outcome | meaning |
//! |---|---|
//! | [`StepRun::Executed`] | ported, derived code ran **and had inputs** |
//! | [`StepRun::Vacuous`] | ported code ran over an empty population — counted apart so an empty world cannot inflate the number |
//! | [`StepRun::Unimplemented`] | no port; the retail function that belongs here is named in [`Gap`] |
//! | [`StepRun::OutOfScope`] | presentation, telemetry or session management: correctly absent from a headless core |
//!
//! Sub-calls inside a step are counted the same way (`Leaders::process_all` is five retail
//! calls, of which two are ported), so "step 8 executed" never hides "three of its five
//! children do not exist". [`Coverage::gaps`] is that ledger.
//!
//! # Ordering facts this driver is obliged to honour
//!
//! * **`Objects::process_all` rotates owner order every frame** — `(frame + i) % 10` for the
//!   unit band, fixed 0..8 for the building and wall bands ([`crate::objects`], `0x0065DCE0`).
//! * **`Game::frame++` is step 20, *after* the object pass**, so the rotation uses the
//!   pre-increment frame (`0x005924BF`).
//! * **`Objects::inc_time` is step 15** — after every unit, building and wall has processed.
//!   Projectile flight, impact and death therefore land at the *end* of the tick, in
//!   ammo-pool slot order, never interleaved with the object pass.
//! * `Groups::process` runs at the **tail** of `GameDaemon::process_all` (step 12), before
//!   any unit moves, not after.
//! * A construction site's `helpers` counter is a **single-frame accumulator**: it is
//!   consumed and cleared by `Wall::process`, and each builder in the same frame divides
//!   the rate by `helpers + 1`. The order builders arrive in is `Objects::process_all`'s
//!   rotated order, which is why `production::construct_frame`'s own doc comment says the
//!   worker order "is not owned by this module". It is owned here.
//!
//! # Fidelity, stated plainly
//!
//! This is **assembly, not derivation**. Every system called here carries whatever tier its
//! own module claims; wiring them together adds no fidelity and this file derives nothing
//! new from the binary. What it adds is *execution*: code that was compiled and tested in
//! isolation now runs in retail's order against shared state. Nothing here is comparable to
//! a retail checksum yet — several inputs (terrain height, unit collision, worker→site
//! assignment) are stand-ins, and each one is a named [`Gap`] rather than a silent guess.

use crate::checksum::adler32;
use crate::objects::{Band, BANDED_SLOTS, HERD_PERIOD, WILDLIFE_PERIOD};
use crate::order::{Order, OrderIndex};
use crate::schedule::{StepStatus, DO_FRAME, FRAMES_PER_SECOND};
use crate::systems::{
    ammo, borders_fog, casters_animals, combat, economy, groups_guys, movement, production,
    victory_score, walls,
};
use crate::world::{Handle, World, MAP_SPAN, OBJ_FLAG_ACTIVE};

/// The `world` channel's own store, consolidated into `map_terrain` by the sibling lane.
/// Aliased because this file also names [`crate::world::World`], which is the unit SoA.
pub use crate::systems::map_terrain::World as TerrainWorld;

/// The 29 entries of `Game::do_frame`.
pub const NUM_STEPS: usize = DO_FRAME.len();

/// Leader slots the tick drives. `Leaders` is ten `LeaderData` but only 0..8 are players
/// (`objects.rs`: the building/wall bands are walked for eight).
pub const NUM_LEADERS: usize = BANDED_SLOTS;

// =======================================================================================
// The gap ledger — every sub-call we do not have, named by its retail function
// =======================================================================================

/// A retail call that belongs inside a step and has no port.
///
/// Naming them individually is the point: "step 12 ran" is only honest next to "and these
/// two of its seven children did not".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(usize)]
pub enum Gap {
    RunScript = 0,
    LeaderCalcWallStats,
    LeaderCalcUnitStats,
    LeaderProcessTaunt,
    LeaderCheckExplore,
    LeaderPlanStrategy,
    LeaderDiplomacy,
    GameDaemonCalcDanger,
    GameDaemonProcessCollBlocks,
    ArmiesProcessAll,
    UnitSufferAttrition,
    UnitProcessSupply,
    GuyProcess,
    UnitDetectCollision,
    AmmoAntiAirDud,
    UnitNeedsTransport,
    ObjectsWildlifeSpawn,
    UnitIncTime,
    LeadersEndProcessAll,
    LeaderProcessEventFrame,
    RoadsScanStray,
    TurnControlCheckCannonTime,
    GameProcessEndGame,
}

impl Gap {
    pub const COUNT: usize = Gap::GameProcessEndGame as usize + 1;
    #[inline]
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn note(self) -> &'static str {
        GAP_NOTES[self as usize]
    }
}

/// One line per [`Gap`], in enum order: the retail function and why it is absent.
pub const GAP_NOTES: [&str; Gap::COUNT] = [
    "step 4  RunTimeEnv::run_script 0x0043D0E0 - no BHS interpreter; the script channel has no runtime producer",
    "step 8  Leader::calc_wall_stats leaders.cpp:13858 - uncited",
    "step 8  Leader::calc_unit_stats leaders.cpp:13831 - uncited",
    "step 8  Leader::process_taunt leaders.cpp:28050 - AI chat",
    "step 11 Leader::check_explore leaders.cpp:26413 - uncited",
    "step 11 Leader::plan_strategy leaders.cpp:26880 (11 KB) - uncited",
    "step 11 Leader::diplomacy 0x006BC950 (20,348 B) - deliberately not ported; a self-play agent replaces it",
    "step 12 GameDaemon::calc_danger gamedaemon.cpp:102 - uncited",
    "step 12 GameDaemon::process_coll_blocks gamedaemon.cpp:556 - uncited",
    "step 13 Armies::process_all 0x006F3B00 / Army::find_target 0x006F69B0 - uncited",
    "step 14 Unit::suffer_attrition - borders_fog::step_attrition exists but needs supply/territory state this driver does not build",
    "step 14 Unit::process_supply unit.cpp:29845 - uncited",
    "step 14 Guy::process 0x005E0230 / Guy::move 0x005D9240 - groups_guys::GuyData exists; per-guy bodies are not populated",
    "step 14 Unit::detect_unit_collision 0x00617060 - unported; the UnitWorld view answers 'never collides'",
    "step 14 Ammo::init anti-air dud roll - unported; it draws game_random 1-2 times per launch, so every launch shifts the stream",
    "step 14 UnitData::needs_transport 0x00609920 - unported; the UnitWorld view answers 0",
    "step 14 Objects::process_all wildlife spawn (frame%32) - draws game_random an unknown number of times; drawing wrongly is worse than not drawing",
    "step 15 Unit::inc_time 0x00610B40 (vtable +0xA0) - uncited; only the Ammo half of Objects::inc_time runs",
    "step 17 Leaders::end_process_all 0x006ED070 - uncited",
    "step 19 Leader::process_event_frame 0x006EC180 - uncited",
    "step 22 Roads::scan_and_kill_stray_roads 0x008956A0 - uncited",
    "step 24 TurnControl::check_cannon_time 0x009579E0 - uncited",
    "step 27 Game::process_end_game 0x00591CE0 - uncited; victory_score::check_victory runs at step 11 instead",
];

// =======================================================================================
// The trace
// =======================================================================================

/// What one step did on one tick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepRun {
    /// Ported code ran and had something to work on.
    Executed,
    /// Ported code ran with nothing in its population. Never counted as executed.
    Vacuous,
    /// No port. The named retail function is what belongs here.
    Unimplemented(Gap),
    /// Presentation, telemetry or session management.
    OutOfScope,
}

impl StepRun {
    #[inline]
    pub fn ran(self) -> bool {
        matches!(self, StepRun::Executed)
    }
    pub fn glyph(self) -> char {
        match self {
            StepRun::Executed => 'X',
            StepRun::Vacuous => 'o',
            StepRun::Unimplemented(_) => '.',
            StepRun::OutOfScope => '-',
        }
    }
}

/// One tick's worth of trace: the outcome of each of the 29 steps and how much work it did.
#[derive(Clone, Debug)]
pub struct TickTrace {
    /// `Game::frame` **before** step 20 incremented it — the value the owner rotation used.
    pub frame: i32,
    pub steps: [StepRun; NUM_STEPS],
    /// A step-defined unit of work: leaders gathered, objects visited, tiles claimed, …
    pub work: [u32; NUM_STEPS],
}

impl Default for TickTrace {
    fn default() -> Self {
        TickTrace {
            frame: 0,
            steps: [StepRun::OutOfScope; NUM_STEPS],
            work: [0; NUM_STEPS],
        }
    }
}

impl TickTrace {
    /// The number this wave is judged by: steps that dispatched into derived code with
    /// real inputs.
    pub fn executed(&self) -> usize {
        self.steps.iter().filter(|s| s.ran()).count()
    }
    pub fn vacuous(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| matches!(s, StepRun::Vacuous))
            .count()
    }
    pub fn unimplemented(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| matches!(s, StepRun::Unimplemented(_)))
            .count()
    }
    pub fn out_of_scope(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| matches!(s, StepRun::OutOfScope))
            .count()
    }

    /// One line of 29 glyphs: `X` executed, `o` vacuous, `.` unimplemented, `-` out of scope.
    pub fn line(&self) -> String {
        let mut s = String::with_capacity(NUM_STEPS + 24);
        s.push_str(&format!("f{:<6} ", self.frame));
        for st in self.steps.iter() {
            s.push(st.glyph());
        }
        s.push_str(&format!("  {}/29 executed", self.executed()));
        s
    }

    /// The full per-step table, for the binary and for a report.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "Game::do_frame 0x00591EF0 - frame {} ({} executed, {} vacuous, {} unimplemented, {} out of scope)\n",
            self.frame,
            self.executed(),
            self.vacuous(),
            self.unimplemented(),
            self.out_of_scope()
        ));
        for (i, st) in self.steps.iter().enumerate() {
            let d = &DO_FRAME[i];
            let what = match st {
                StepRun::Executed => format!("EXECUTED  work={}", self.work[i]),
                StepRun::Vacuous => "vacuous   (ported, empty population)".to_string(),
                StepRun::Unimplemented(g) => format!("skipped   {}", g.note()),
                StepRun::OutOfScope => "out of scope".to_string(),
            };
            out.push_str(&format!(
                "  {:>2} {:<44} {:<12} {}\n",
                i,
                d.name,
                d.va.unwrap_or("-"),
                what
            ));
        }
        out
    }
}

/// Cumulative outcomes over a run.
#[derive(Clone, Debug)]
pub struct Coverage {
    pub ticks: u64,
    pub executed: [u64; NUM_STEPS],
    pub vacuous: [u64; NUM_STEPS],
    pub unimplemented: [u64; NUM_STEPS],
    pub out_of_scope: [u64; NUM_STEPS],
    pub work: [u64; NUM_STEPS],
    /// Times each named sub-call was skipped.
    pub gaps: [u64; Gap::COUNT],
    // ---- what the driven systems actually did, counted at the call site ----
    pub unit_process: u64,
    pub unit_move_step: u64,
    pub unit_attack: u64,
    pub damage_applied: u64,
    pub damage_total: i64,
    pub deaths: u64,
    pub hold_decrements: u64,
    pub build_process: u64,
    pub build_construct_steps: u64,
    pub builds_completed: u64,
    pub wall_process: u64,
    pub ammo_steps: u64,
    pub ammo_impacts: u64,
    pub ammo_closed: u64,
    pub fog_cells_revealed: u64,
    pub border_tiles: u64,
    pub group_normalises: u64,
    pub leader_gathers: u64,
    pub market_cycles: u64,
    pub paths_searched: u64,
    pub path_search_failures: u64,
    pub herd_steps: u64,
    pub rng_draws_missing: u64,
}

impl Default for Coverage {
    fn default() -> Self {
        Coverage {
            ticks: 0,
            executed: [0; NUM_STEPS],
            vacuous: [0; NUM_STEPS],
            unimplemented: [0; NUM_STEPS],
            out_of_scope: [0; NUM_STEPS],
            work: [0; NUM_STEPS],
            gaps: [0; Gap::COUNT],
            unit_process: 0,
            unit_move_step: 0,
            unit_attack: 0,
            damage_applied: 0,
            damage_total: 0,
            deaths: 0,
            hold_decrements: 0,
            build_process: 0,
            build_construct_steps: 0,
            builds_completed: 0,
            wall_process: 0,
            ammo_steps: 0,
            ammo_impacts: 0,
            ammo_closed: 0,
            fog_cells_revealed: 0,
            border_tiles: 0,
            group_normalises: 0,
            leader_gathers: 0,
            market_cycles: 0,
            paths_searched: 0,
            path_search_failures: 0,
            herd_steps: 0,
            rng_draws_missing: 0,
        }
    }
}

impl Coverage {
    fn record(&mut self, t: &TickTrace) {
        self.ticks += 1;
        for i in 0..NUM_STEPS {
            self.work[i] += t.work[i] as u64;
            match t.steps[i] {
                StepRun::Executed => self.executed[i] += 1,
                StepRun::Vacuous => self.vacuous[i] += 1,
                StepRun::Unimplemented(g) => {
                    self.unimplemented[i] += 1;
                    self.gaps[g.index()] += 1;
                }
                StepRun::OutOfScope => self.out_of_scope[i] += 1,
            }
        }
    }

    /// Steps that executed on **every** tick of the run.
    pub fn always_executed(&self) -> usize {
        if self.ticks == 0 {
            return 0;
        }
        self.executed.iter().filter(|&&n| n == self.ticks).count()
    }
    /// Steps that executed on at least one tick.
    pub fn ever_executed(&self) -> usize {
        self.executed.iter().filter(|&&n| n > 0).count()
    }
    /// Steps with a runnable body that never ran because the world was empty.
    pub fn ever_vacuous(&self) -> usize {
        self.vacuous.iter().filter(|&&n| n > 0).count()
    }
}

// =======================================================================================
// Per-leader and map state the systems modules need
// =======================================================================================

/// One player slot's economy and border inputs, as the tick reads them.
#[derive(Clone, Debug, Default)]
pub struct LeaderSlot {
    /// `leaders[who].flags & 1` — the gate every per-leader loop tests.
    pub active: bool,
    /// The econ block at `LeaderData + 0x6EB8`.
    pub econ: economy::LeaderEcon,
    /// `Leader::gather`'s recompute stamp and dirty bit.
    pub last_calc_frame: i32,
    pub dirty: bool,
    pub gather_inputs: economy::GatherInputs,
    pub cap_gates: economy::CapGates,
    pub gather_ctx: economy::DoGatherContext,
    pub border: borders_fog::LeaderBorderInput,
}

/// The world planes `GameDaemon::process_all` reads and writes.
///
/// The planes themselves live on [`TerrainWorld`] — the sibling lane consolidated the two
/// claimants of checksum channel 12 onto one store while this file was being written, so
/// a fog stamp or a territory claim made here is immediately visible to
/// `map_terrain::World::checksum()`, which is the point of the consolidation.
#[derive(Clone, Debug)]
pub struct MapState {
    pub world: TerrainWorld,
    /// Fog *policy* — the per-leader facts and the game option. Not a store.
    pub fog: borders_fog::Fog,
    pub circle: borders_fog::CircleTable,
    pub territory: borders_fog::TerritoryRules,
    pub regions: Vec<borders_fog::RegionBorderState>,
    pub border_sources: [Vec<borders_fog::BorderSource>; NUM_LEADERS],
}

impl MapState {
    /// A flat, unblocked map of `wcells` x `wcells` WCoord cells (4 tiles each).
    pub fn new(wcells: u16) -> MapState {
        MapState {
            world: TerrainWorld::init_default_rules(wcells as i32, wcells as i32),
            fog: borders_fog::Fog::new(),
            circle: borders_fog::CircleTable::build(),
            territory: borders_fog::TerritoryRules::default(),
            regions: Vec::new(),
            border_sources: Default::default(),
        }
    }

    /// One region covering the whole grid, which is what `check_borders` walks.
    pub fn single_region(&mut self) {
        let mut coords = Vec::with_capacity(self.world.size as usize);
        for wy in 0..self.world.ys {
            for wx in 0..self.world.xs {
                coords.push((wx, wy));
            }
        }
        self.regions = vec![borders_fog::RegionBorderState {
            flags: 4,
            size: coords.len() as i32,
            borders: 0,
            coords,
        }];
    }

    /// Re-arm the region cursors so `check_borders` has work again — retail dirties a
    /// region when a border source changes; this driver has no such event yet.
    pub fn dirty_regions(&mut self) {
        for r in self.regions.iter_mut() {
            r.borders = 0;
        }
    }
}

/// The `UnitWorld` view `movement.rs` searches and steps over.
///
/// Three of its seven queries are stand-ins and each is a named [`Gap`]: `unit_collides`
/// (`Unit::detect_unit_collision` `0x00617060` is unported), `needs_transport`
/// (`0x00609920`), and `invalid_loc`, which retail evaluates per tile and this maps onto
/// the WCoord `blocked` byte the terrain lane owns.
struct MapView<'a> {
    map: &'a MapState,
}

impl<'a> MapView<'a> {
    #[inline]
    fn in_w(&self, wx: i32, wy: i32) -> bool {
        wx >= 0 && wy >= 0 && wx < self.map.world.xs && wy < self.map.world.ys
    }
}

impl<'a> movement::UnitWorld for MapView<'a> {
    fn tiles_w(&self) -> i32 {
        self.map.world.tile_xs
    }
    fn tiles_h(&self) -> i32 {
        self.map.world.tile_ys
    }
    fn wcells_w(&self) -> i32 {
        self.map.world.xs
    }
    fn invalid_loc(&self, tile_x: i32, tile_y: i32) -> bool {
        let (wx, wy) = (tile_x >> 2, tile_y >> 2);
        if !self.in_w(wx, wy) {
            return true;
        }
        let c = &self.map.world.wdata[self.map.world.w_index(wx, wy)];
        c.blocked != 0 || c.bad != 0
    }
    fn unit_collides(&self, _x: i32, _y: i32) -> bool {
        false
    }
    fn needs_transport(&self, _fx: i32, _fy: i32, _tx: i32, _ty: i32) -> i32 {
        0
    }
    fn tregion(&self, tile_x: i32, tile_y: i32) -> i32 {
        let (wx, wy) = (tile_x >> 2, tile_y >> 2);
        if !self.in_w(wx, wy) {
            return -1;
        }
        self.map.world.wdata[self.map.world.w_index(wx, wy)].region as i32
    }
}

/// The `AmmoEnv` `Objects::inc_time` flies projectiles through.
///
/// `terrain_z` is flat zero — the terrain-height lane owns `TerrainOut::find_data_z`
/// `0x00866560` and there is no height plane to read. Every arc therefore lands on its
/// computed arrival frame and never clips into a hillside early, which is a stated
/// divergence, not an approximation of one.
struct AmmoView<'a> {
    map: &'a MapState,
    units: &'a crate::generated::state::UnitCols,
    objects: &'a crate::objects::ObjectRegistry,
    unit_type: &'a [i32],
    shooter_rules: &'a [(i32, ammo::ShooterRules)],
    live: usize,
}

impl<'a> AmmoView<'a> {
    /// `(who, o)` is the engine's object address: `o` indexes the owner's band, not the
    /// column row. Resolving it here is what keeps `check_hit`'s `find_unit_near` result
    /// usable by `object()`.
    fn row_of(&self, who: i32, o: i32) -> Option<usize> {
        if who < 0 || o < 0 || who as usize >= crate::objects::OWNER_SLOTS {
            return None;
        }
        self.objects
            .slot(who as usize)
            .band(Band::Unit)
            .get(o as usize)
            .copied()
            .map(|r| r as usize)
            .filter(|&r| r < self.live)
    }

    fn rules_of(&self, row: usize) -> ammo::ShooterRules {
        let t = self.unit_type.get(row).copied().unwrap_or(0);
        self.shooter_rules
            .iter()
            .find(|(id, _)| *id == t)
            .map(|(_, r)| *r)
            .unwrap_or_default()
    }
}

impl<'a> ammo::AmmoEnv for AmmoView<'a> {
    fn object(&self, who: i32, o: i32) -> Option<ammo::ObjView> {
        let row = self.row_of(who, o)?;
        Some(ammo::ObjView {
            alive: self.units.get_flags(row) & OBJ_FLAG_ACTIVE != 0,
            is_unit: true,
            x: self.units.x_internal()[row],
            y: self.units.y_internal()[row],
            z: 0,
            guy_mark: self.units.guy_mark()[row] as i32,
            guy0_z: 0,
            rules: self.rules_of(row),
        })
    }
    fn terrain_z(&self, _x: i32, _y: i32) -> i32 {
        0
    }
    fn world_tiles(&self) -> (i32, i32) {
        (self.map.world.tile_xs, self.map.world.tile_ys)
    }
    /// `ObjectsData::find_unit(x, y, .., radius 0x180, ..)` as `check_hit` calls it:
    /// the nearest unit within two tiles, returned as `(who, o, dist)`.
    fn find_unit_near(&self, x: i32, y: i32, shooter_who: i32) -> Option<(i32, i32, i32)> {
        let mut best: Option<(i32, i32, i32)> = None;
        for who in 0..crate::objects::OWNER_SLOTS {
            for (o, &row) in self.objects.slot(who).band(Band::Unit).iter().enumerate() {
                let row = row as usize;
                if row >= self.live || self.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
                    continue;
                }
                if who as i32 == shooter_who {
                    continue;
                }
                let d = ammo::vector_dist(
                    self.units.x_internal()[row] - x,
                    self.units.y_internal()[row] - y,
                );
                if d > 2 * 192 {
                    continue;
                }
                if best.is_none_or(|(_, _, bd)| d < bd) {
                    best = Some((who as i32, o as i32, d));
                }
            }
        }
        best
    }
    fn find_building_at(&self, _tx: i32, _ty: i32) -> Option<(i32, i32)> {
        None
    }
    fn is_water_tile(&self, tx: i32, ty: i32) -> bool {
        let (wx, wy) = (tx >> 2, ty >> 2);
        if wx < 0 || wy < 0 || wx >= self.map.world.xs || wy >= self.map.world.ys {
            return false;
        }
        self.map.world.wdata[self.map.world.w_index(wx, wy)].land < 0
    }
}

/// A projectile in flight, plus the identity the pool slot does not carry.
#[derive(Clone, Copy, Debug, Default)]
pub struct AmmoShot {
    /// Damage to apply on impact, already through `ObjectData::get_damage`.
    pub damage: i32,
}

// =======================================================================================
// The simulation
// =======================================================================================

/// A world plus the state its tick needs, and the executable `Game::do_frame`.
///
/// Deliberately not `Clone`: `movement::PathFinder` owns search containers that are scratch
/// rather than state, and cloning them would make two sims look different when they are not.
pub struct Sim {
    pub world: World,

    // ---- step 8 / 12: the economy -----------------------------------------------------
    pub econ_rules: economy::EconRules,
    pub leaders: [LeaderSlot; NUM_LEADERS],
    pub market: economy::MarketState,

    // ---- step 11 / 12: score and victory ----------------------------------------------
    pub vic_match: victory_score::Match,
    pub vic_leaders: victory_score::Leaders,

    // ---- step 12: fog, borders, groups ------------------------------------------------
    pub map: MapState,
    pub groups: groups_guys::Groups,

    // ---- step 14: the object bands ----------------------------------------------------
    pub prod_rules: production::ProdRules,
    pub combat_rules: combat::CombatConstants,
    /// Band 2000, indexed by the row an [`crate::objects::ObjectRegistry`] entry carries.
    pub builds: Vec<production::BuildData>,
    /// Band 3000.
    pub walls: Vec<walls::WallState>,
    pub herds: Vec<casters_animals::HerdData>,
    /// `UnitData::ptype`'s referent id, per unit row. `World` keeps its own copy privately.
    pub unit_type: Vec<i32>,
    /// Per-type `ObjectType` projectile fields. A type present here is a **ranged**
    /// shooter: `Unit::do_attack` fires ammo at it instead of applying damage in place,
    /// which is what moves the kill to step 15.
    pub shooter_rules: Vec<(i32, ammo::ShooterRules)>,
    /// `Stack<PathData>` at `UnitData+0xB8`, per unit row.
    pub paths: Vec<movement::PathStack>,
    pub path_unit: Vec<movement::PathUnit>,
    pub pathfinder: movement::PathFinder,

    // ---- step 15: projectiles and corpses ---------------------------------------------
    pub ammo: ammo::AmmoPool,
    pub shots: Vec<AmmoShot>,
    pub deaths: combat::DeathRing,

    pub cover: Coverage,
    /// Reused every tick: allocating the traversal order was measurably the largest cost
    /// in the object pass.
    traversal_buf: Vec<(usize, Band, u32, u32)>,
    seen_buf: Vec<(i32, i32)>,
}

impl Sim {
    /// A sim over a `wcells` x `wcells` WCoord map (4 tiles per cell, 192 fine units per
    /// tile — so 64 gives the 256-tile square [`MAP_SPAN`] describes).
    pub fn new(seed: u64, wcells: u16) -> Sim {
        let mut map = MapState::new(wcells);
        map.single_region();
        let mut vic_match = victory_score::Match {
            world_xs: (wcells as i32) * 4,
            world_land_size: (wcells as i32) * (wcells as i32),
            ..Default::default()
        };
        vic_match.num_nations = NUM_LEADERS as i32;
        vic_match.num_sides = 2;
        let types =
            victory_score::TypeTable::with_default_kinds(victory_score::ScoreConstants::default());
        Sim {
            world: World::new(seed),
            econ_rules: economy::EconRules::shipped(),
            leaders: Default::default(),
            market: economy::MarketState::default(),
            vic_match,
            vic_leaders: victory_score::Leaders::new(types),
            map,
            groups: groups_guys::Groups::default(),
            prod_rules: production::ProdRules::shipped(),
            combat_rules: combat::CombatConstants::shipped(),
            builds: Vec::new(),
            walls: Vec::new(),
            herds: Vec::new(),
            unit_type: Vec::new(),
            shooter_rules: Vec::new(),
            paths: Vec::new(),
            path_unit: Vec::new(),
            pathfinder: movement::PathFinder::new(),
            ammo: ammo::AmmoPool::new(),
            shots: vec![AmmoShot::default(); ammo::AMMO_POOL_SLOTS],
            deaths: combat::DeathRing::with_capacity(64),
            cover: Coverage::default(),
            traversal_buf: Vec::new(),
            seen_buf: Vec::new(),
        }
    }

    // -- population -------------------------------------------------------------------

    /// Activate a player slot everywhere the tick tests for it.
    pub fn activate(&mut self, who: usize) {
        self.leaders[who].active = true;
        self.leaders[who].border.active = true;
        self.vic_leaders.slots[who].leader_flags |=
            victory_score::leader_flag::VALID | victory_score::leader_flag::ACTIVE;
        self.world.objects.set_active(who, true);
        self.map.fog.leaders[who].player_mask = 1u8 << who;
    }

    /// Spawn a unit and give it everything the driven systems read.
    ///
    /// `los_tiles` lands in `ObjectData::mylos` (+60), a real field with no derived
    /// source in this driver — it is scenario input, exactly like `World::spawn`'s
    /// random placement.
    pub fn spawn_unit(
        &mut self,
        who: usize,
        type_id: i32,
        x: i32,
        y: i32,
        los_tiles: i8,
    ) -> Option<Handle> {
        let h = self.world.spawn_typed(who as u8, type_id)?;
        let row = self.world.row_of(h)?;
        self.world.set_pos(row, x, y);
        self.world.units.mylos_mut()[row] = los_tiles;
        self.world.units.guy_mark_mut()[row] = 1;
        while self.unit_type.len() <= row {
            self.unit_type.push(0);
            self.paths.push(movement::PathStack::new());
            self.path_unit.push(movement::PathUnit::default());
        }
        self.unit_type[row] = type_id;
        Some(h)
    }

    /// Register a building in owner `who`'s band 2000.
    pub fn spawn_build(&mut self, who: usize, mut bd: production::BuildData) -> usize {
        let row = self.builds.len();
        bd.who = who as u8;
        let o = self.world.objects.insert(who, Band::Build, row as u32);
        let _ = o;
        self.builds.push(bd);
        row
    }

    /// Register a wall segment in owner `who`'s band 3000.
    pub fn spawn_wall(&mut self, who: usize, mut st: walls::WallState) -> usize {
        let row = self.walls.len();
        st.who = who as u8;
        let o = self.world.objects.insert(who, Band::Wall, row as u32);
        st.o = (o - crate::objects::WALL_BAND_BASE) as i16;
        self.walls.push(st);
        row
    }

    /// Issue an order and, for a locomotion order, run the pathfinder the way
    /// `Unit::repath` does — `find_upath` first, `astar_path` only if the straight-line
    /// probe fails.
    pub fn issue(&mut self, h: Handle, o: Order) -> bool {
        let Some(row) = self.world.row_of(h) else {
            return false;
        };
        let kind = o.kind;
        let (dx, dy) = (o.x, o.y);
        self.world.orders_mut(row).replace(o);
        if matches!(kind, OrderIndex::MoveTo | OrderIndex::FleeTo) {
            self.repath(row, dx, dy);
        }
        true
    }

    /// `Unit::repath` — rebuild the unit's `Stack<PathData>` toward `(dx, dy)`.
    fn repath(&mut self, row: usize, dx: i32, dy: i32) {
        let x = self.world.units.x_internal()[row];
        let y = self.world.units.y_internal()[row];
        let tol = self.world.units.tolerance()[row];
        let path = &mut self.paths[row];
        path.clear();
        // The three-record protocol `astar_path` expects: [anchor, start, goal]. The
        // anchor carries the arrival tolerance, which is read from below `start`.
        path.push(movement::PathData {
            to_x: dx,
            to_y: dy,
            tolerance: tol,
            flags: 0,
        });
        path.push(movement::PathData {
            to_x: x,
            to_y: y,
            tolerance: 0,
            flags: movement::PathData::FLAG_MORE,
        });
        path.push(movement::PathData {
            to_x: dx,
            to_y: dy,
            tolerance: tol,
            flags: 0,
        });

        let view = MapView { map: &self.map };
        let unit = self.path_unit[row];
        let outcome = self
            .pathfinder
            .find_upath_prepare(&view, path, &unit, dx, dy);
        self.cover.paths_searched += 1;
        if outcome == movement::UPathOutcome::NeedsSearch {
            let args = movement::SearchArgs {
                unit: &unit,
                quick: 0,
            };
            let r = self.pathfinder.astar_path_unit(&view, path, &args);
            if !matches!(r, movement::SearchResult::Found) {
                self.cover.path_search_failures += 1;
            }
        }
        if path.is_empty() {
            // Off-map or stalled: fall back to the order's own destination so the
            // integrator still has a waypoint. Recorded, not hidden.
            path.push(movement::PathData {
                to_x: dx,
                to_y: dy,
                tolerance: tol,
                flags: 0,
            });
        }
    }

    // -- the tick ---------------------------------------------------------------------

    /// `Game::do_frame` `0x00591EF0`, all 29 steps, in retail's order.
    pub fn do_frame(&mut self) -> TickTrace {
        let mut t = TickTrace {
            frame: self.world.frame,
            ..Default::default()
        };

        // 0..3 — autosave, the desync log, the debug-lag draw, the speed command.
        for s in 0..4 {
            t.steps[s] = StepRun::OutOfScope;
        }
        // 4 — RunTimeEnv::run_script, twice.
        t.steps[4] = StepRun::Unimplemented(Gap::RunScript);
        // 5..7 — Conquer-the-World, tutorial, Steam.
        for s in 5..8 {
            t.steps[s] = StepRun::OutOfScope;
        }

        // 8 — Leaders::process_all: the per-player economy, before anything moves.
        let (r, w) = self.leaders_process_all();
        t.steps[8] = r;
        t.work[8] = w;

        // 9, 10 — the socket pump and AI diplomacy chat.
        t.steps[9] = StepRun::OutOfScope;
        t.steps[10] = StepRun::OutOfScope;

        // 11 — Leaders::strategy_all.
        let (r, w) = self.leaders_strategy_all();
        t.steps[11] = r;
        t.work[11] = w;

        // 12 — GameDaemon::process_all: victory, danger, fog, markets, borders, groups.
        let (r, w) = self.game_daemon_process_all();
        t.steps[12] = r;
        t.work[12] = w;

        // 13 — Armies::process_all.
        t.steps[13] = StepRun::Unimplemented(Gap::ArmiesProcessAll);

        // 14 — Objects::process_all. The rotation lives here.
        let (r, w) = self.objects_process_all();
        t.steps[14] = r;
        t.work[14] = w;

        // 15 — Objects::inc_time. Projectiles fly, impact and kill AFTER every object.
        let (r, w) = self.objects_inc_time();
        t.steps[15] = r;
        t.work[15] = w;

        // 16 — the FX event queue.
        t.steps[16] = StepRun::OutOfScope;
        // 17, 18, 19.
        t.steps[17] = StepRun::Unimplemented(Gap::LeadersEndProcessAll);
        t.steps[18] = StepRun::OutOfScope;
        t.steps[19] = StepRun::Unimplemented(Gap::LeaderProcessEventFrame);

        // 20 — Game::frame++. After the object pass, which is why the rotation used the
        // pre-increment value.
        self.world.frame = self.world.frame.wrapping_add(1);
        self.vic_match.frame = self.world.frame;
        t.steps[20] = StepRun::Executed;
        t.work[20] = 1;

        // 21 — OrdersMemManager::cycle: 28 recycler pools, no analogue here.
        t.steps[21] = StepRun::OutOfScope;
        // 22 — stray roads.
        t.steps[22] = StepRun::Unimplemented(Gap::RoadsScanStray);

        // 23 — 15 sim frames is one game second, exactly.
        t.steps[23] = StepRun::Executed;
        if self.world.frame % FRAMES_PER_SECOND == 0 {
            self.world.seconds = self.world.seconds.wrapping_add(1);
            self.vic_match.tick = self.world.seconds;
            t.work[23] = 1;
        }

        // 24..28.
        t.steps[24] = StepRun::Unimplemented(Gap::TurnControlCheckCannonTime);
        t.steps[25] = StepRun::OutOfScope;
        t.steps[26] = StepRun::OutOfScope;
        t.steps[27] = StepRun::Unimplemented(Gap::GameProcessEndGame);
        t.steps[28] = StepRun::OutOfScope;

        self.cover.record(&t);
        t
    }

    /// `n` ticks, returning the last trace.
    pub fn run(&mut self, n: usize) -> TickTrace {
        let mut last = TickTrace::default();
        for _ in 0..n {
            last = self.do_frame();
        }
        last
    }

    // -- step 8 -----------------------------------------------------------------------

    /// `Leaders::process_all` `0x006ED2A0` — for each active player, in **slot order**:
    /// `Leader::gather` -> `calc_wall_stats` -> `calc_unit_stats` -> `process_elimination`
    /// -> `process_taunt`. Two of the five are ported.
    fn leaders_process_all(&mut self) -> (StepRun, u32) {
        let frame = self.world.frame;
        let mut n = 0u32;
        for who in 0..NUM_LEADERS {
            if !self.leaders[who].active {
                continue;
            }
            let l = &mut self.leaders[who];
            // Leader::gather 0x006CE280 -> calc_gather / calc_resource_caps / do_gather.
            let inputs = l.gather_inputs.clone();
            let gates = l.cap_gates;
            let ctx = l.gather_ctx;
            let mut last = l.last_calc_frame;
            let mut dirty = l.dirty;
            economy::leader_gather(
                &self.econ_rules,
                &mut l.econ,
                frame,
                who as i32,
                &mut last,
                &mut dirty,
                &inputs,
                &gates,
                &ctx,
            );
            l.last_calc_frame = last;
            l.dirty = dirty;
            self.cover.leader_gathers += 1;
            // Leader::process_elimination 0x006EC?? -- victory_score owns it.
            self.vic_leaders
                .process_elimination(&mut self.vic_match, who);
            n += 1;
        }
        // The three children with no port.
        self.cover.gaps[Gap::LeaderCalcWallStats.index()] += 1;
        self.cover.gaps[Gap::LeaderCalcUnitStats.index()] += 1;
        self.cover.gaps[Gap::LeaderProcessTaunt.index()] += 1;
        if n == 0 {
            (StepRun::Vacuous, 0)
        } else {
            (StepRun::Executed, n)
        }
    }

    // -- step 11 ----------------------------------------------------------------------

    /// `Leaders::strategy_all` `0x006ED430` — `check_explore`, `plan_strategy`,
    /// `compute_score`, `diplomacy`, `Game::check_victory`. Of the five, the two that are
    /// not AI behaviour are ported.
    fn leaders_strategy_all(&mut self) -> (StepRun, u32) {
        let mut n = 0u32;
        for who in 0..NUM_LEADERS {
            if !self.leaders[who].active {
                continue;
            }
            self.vic_leaders.compute_score(&self.vic_match, who, 0);
            n += 1;
        }
        if n > 0 {
            // Game::check_victory game.cpp:1561.
            self.vic_leaders.check_victory(&mut self.vic_match);
        }
        self.cover.gaps[Gap::LeaderCheckExplore.index()] += 1;
        self.cover.gaps[Gap::LeaderPlanStrategy.index()] += 1;
        self.cover.gaps[Gap::LeaderDiplomacy.index()] += 1;
        if n == 0 {
            (StepRun::Vacuous, 0)
        } else {
            (StepRun::Executed, n)
        }
    }

    // -- step 12 ----------------------------------------------------------------------

    /// `GameDaemon::process_all` `0x00732700` — `process_victory`, `calc_danger`,
    /// `update_all_seen`, `calc_markets`, `check_borders`, `process_coll_blocks`,
    /// `Groups::process`. Five of the seven run.
    ///
    /// Order is retail's, and it matters: fog, markets and borders are all recomputed
    /// **before** any unit moves, and group normalisation is the tail of this pass rather
    /// than a pass of its own.
    fn game_daemon_process_all(&mut self) -> (StepRun, u32) {
        let mut work = 0u32;
        let frame = self.world.frame;

        // Retail runs this pass unconditionally. With no leader and no object there is
        // nothing for any of its seven children to read, and doing the work anyway would
        // let an empty world inflate the number this file exists to report.
        let any_active = self.leaders.iter().any(|l| l.active);
        if !any_active && self.world.live_count() == 0 {
            return (StepRun::Vacuous, 0);
        }

        // GameDaemon::process_victory gamedaemon.cpp:585.
        let zeros = [0i32; NUM_LEADERS];
        self.vic_leaders
            .process_victory(&mut self.vic_match, &zeros, &zeros);
        work += 1;

        // calc_danger: no port.
        self.cover.gaps[Gap::GameDaemonCalcDanger.index()] += 1;

        // update_all_seen gamedaemon.cpp:232 — World::clear_seen first, then every seeing
        // object stamps its disc.
        self.map.world.clear_seen();
        let live = self.world.live_count() as usize;
        let mut revealed = 0u64;
        let mut buf = std::mem::take(&mut self.seen_buf);
        for row in 0..live {
            if self.world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
                continue;
            }
            let obj = borders_fog::SeeingObject {
                fine_x: self.world.units.x_internal()[row],
                fine_y: self.world.units.y_internal()[row],
                owner: self.world.units.get_who(row),
                los_tiles: self.world.units.mylos()[row] as i32,
                detector: false,
                grant_seen2_to: 0,
            };
            buf.clear();
            borders_fog::update_seen(
                &self.map.fog,
                &mut self.map.world,
                &self.map.circle,
                &obj,
                &mut buf,
            );
            revealed += buf.len() as u64;
            work += 1;
        }
        self.seen_buf = buf;
        self.cover.fog_cells_revealed += revealed;

        // GameDaemon::calc_markets 0x00732?? — one RNG consumer, on the sim stream.
        let before = self.market.cycle;
        economy::calc_markets(
            &self.econ_rules,
            &mut self.market,
            &mut self.world.random,
            frame,
        );
        if self.market.cycle != before {
            self.cover.market_cycles += 1;
            work += 1;
        }

        // GameDaemon::check_borders 0x00732060.
        let mut inputs = [borders_fog::LeaderBorderInput::default(); NUM_LEADERS];
        for (i, l) in self.leaders.iter().enumerate() {
            inputs[i] = l.border;
        }
        // The limit triples now come off `World +0x38..0x4c` rather than being passed in,
        // so a caller can no longer hand `check_borders` limits that differ from the ones
        // the checksum walks.
        let tiles = borders_fog::check_borders(
            &mut self.map.regions,
            &mut self.map.world,
            &self.map.border_sources,
            &inputs,
            &self.map.territory,
        );
        self.cover.border_tiles += tiles as u64;
        work += tiles as u32;

        // process_coll_blocks: no port.
        self.cover.gaps[Gap::GameDaemonProcessCollBlocks.index()] += 1;

        // Groups::process groups.cpp:12529 — the tail of this pass.
        let mut active = [false; NUM_LEADERS];
        for (i, l) in self.leaders.iter().enumerate() {
            active[i] = l.active;
        }
        let any_active = active.iter().any(|&a| a);
        if any_active {
            let keep = |_who: usize, _o: i16| groups_guys::MemberState::Keep;
            let speed = |_who: usize, _g: &groups_guys::GroupData| None;
            self.groups.process(&active, &keep, &speed);
            self.cover.group_normalises += 1;
            work += 1;
        }

        if work == 0 {
            (StepRun::Vacuous, 0)
        } else {
            (StepRun::Executed, work)
        }
    }

    // -- step 14 ----------------------------------------------------------------------

    /// `Objects::process_all` `0x0065DCE0`.
    ///
    /// The unit band is walked in `(frame + i) % 10` owner order; the building and wall
    /// bands in fixed 0..8 order, buildings before walls. That is not a detail: a
    /// construction site's `helpers` divisor makes the *order builders arrive in* change
    /// the work applied, and every RNG-drawing object update shifts the stream.
    fn objects_process_all(&mut self) -> (StepRun, u32) {
        let frame = self.world.frame;
        let mut order = std::mem::take(&mut self.traversal_buf);
        self.world.objects.traversal_into(frame, &mut order);
        let mut work = 0u32;

        for &(_who, band, o, row) in order.iter() {
            match band {
                Band::Unit => {
                    let row = row as usize;
                    if row >= self.world.live_count() as usize {
                        continue;
                    }
                    if self.world.units.get_flags(row) & OBJ_FLAG_ACTIVE != 0 {
                        self.unit_process(row);
                        work += 1;
                    } else {
                        let hf = self.world.units.get_hold_frames(row);
                        if hf != 0 {
                            self.world.units.set_hold_frames(row, hf - 1);
                            self.cover.hold_decrements += 1;
                        }
                    }
                }
                Band::Build => {
                    let _ = o;
                    self.build_process(row as usize, frame);
                    work += 1;
                }
                Band::Wall => {
                    self.wall_process(row as usize, frame);
                    work += 1;
                }
            }
        }
        self.traversal_buf = order;

        // frame % 32: the wildlife spawn draws game_random an unknown number of times.
        // Drawing the wrong count is worse than drawing none; the frames are counted.
        if frame % WILDLIFE_PERIOD == 0 {
            self.cover.gaps[Gap::ObjectsWildlifeSpawn.index()] += 1;
            self.cover.rng_draws_missing += 1;
        }

        // frame % 64: one herd, round-robin. This one IS ported, and it draws the sim
        // stream twice before any bounds test — including on rejection.
        if frame % HERD_PERIOD == 0 && !self.herds.is_empty() {
            if let Some(i) = casters_animals::scheduled_active_herd(frame as u32, &self.herds) {
                let (xs, ys) = (self.map.world.xs, self.map.world.ys);
                let rng = &mut self.world.random;
                let tworld = &self.map.world;
                casters_animals::process_herd(
                    &mut self.herds[i],
                    xs,
                    ys,
                    |lo, hi| rng.get(lo, hi),
                    |wx, wy| {
                        let c = &tworld.wdata[tworld.w_index(wx, wy)];
                        casters_animals::HerdCell {
                            flags: c.flags,
                            owner: c.who.max(0) as u8,
                        }
                    },
                );
                self.cover.herd_steps += 1;
                work += 1;
            }
        }

        if work == 0 {
            (StepRun::Vacuous, 0)
        } else {
            (StepRun::Executed, work)
        }
    }

    /// `Unit::process` `0x00610BC0`, vtable slot 39 (+0x9C).
    ///
    /// Retail's prelude — attrition, spells, healing, cloak, supply — is absent and
    /// counted. What runs is `Unit::work` (slot 98, +0x188) into the `do_job` table.
    fn unit_process(&mut self, row: usize) {
        self.cover.unit_process += 1;
        self.unit_work(row);
        // Guy::process 0x005E0230 -> Guy::move: the per-guy bodies under the unit are not
        // populated, so the unit's own position is the only body that moves.
        self.cover.gaps[Gap::GuyProcess.index()] += 1;
    }

    /// `Unit::work` `0x0060D180` -> `Unit::do_job` `0x00617A10`, the 28-entry jump table
    /// at `0x00617B94` indexed directly by `OrderIndex`.
    ///
    /// # This is the seam to replace next
    ///
    /// A sibling lane landed [`crate::systems::order_dispatch`] during this wave: a real
    /// `Unit::work` port with the `(frame + o) % 32 / % 16 / % 64` phasing, `update_order`,
    /// `repath`, `check_target_path`, `kill_current_order`, and a `WorkWorld` host trait.
    /// It is strictly better than the five arms below and it is what step 14 should
    /// dispatch into. Wiring it needs three things this driver does not yet have: a
    /// `UnitWork` record per unit row kept in sync with the columns, a `WorkWorld` impl
    /// (`frame`, `target`, `attack`, `gather`, `draw_path_retry_delay`) over `Sim`, and a
    /// decision about `draw_path_retry_delay`, which **must** consume
    /// `Random::get(0, 0xFFFF) % 3 + 6` from the sim stream or every later draw in the tick
    /// desyncs. It was not wired here because that module's own suite was still red while
    /// this file was being written; the note is the handoff, not an excuse.
    fn unit_work(&mut self, row: usize) {
        let kind = self.world.orders(row).order_type();
        match kind {
            // Arm 0: the virtual at [unit+0x184]. An idle unit holds position.
            OrderIndex::None => {}
            // Arms 1 and 4 share one executor, `Unit::do_move` 0x005F7B30 [measured].
            OrderIndex::MoveTo | OrderIndex::FleeTo => self.do_move(row),
            // Arm 10, `Unit::do_attack` 0x005F1B80.
            OrderIndex::Attack => self.do_attack(row),
            // Arm 6, `Unit::do_build` 0x005EEBF0 -> Wall::do_construct.
            OrderIndex::BuildAt => self.do_build(row),
            // Arm 5 falls to the default arm and does nothing. Faithfully empty.
            OrderIndex::Patrol => {}
            _ => {}
        }
    }

    /// `Unit::do_move` `0x005F7B30` -> `Unit::move_step` `0x005FAF30`.
    ///
    /// The integrator is the ported one in [`crate::systems::movement`], driven off the
    /// unit's own `Stack<PathData>`, rather than the straight-line placeholder in
    /// `World::do_move`.
    fn do_move(&mut self, row: usize) {
        let Some(ord) = self.world.orders(row).current().copied() else {
            return;
        };
        let speed = self.world.units.myspeed()[row] as i32;
        if speed <= 0 {
            return;
        }
        let target = {
            let p = &self.paths[row];
            match p.peek() {
                Some(r) => (r.to_x, r.to_y),
                None => (ord.x, ord.y),
            }
        };
        let mut body = movement::Body {
            x: self.world.units.x_internal()[row],
            y: self.world.units.y_internal()[row],
            angle: self.world.units.angle()[row],
            stuck_budget: 0,
        };
        // Field borrows are disjoint: the view reads `map`, the step writes `paths`.
        let mut view = MapView { map: &self.map };
        let path = &mut self.paths[row];
        // `turn_rate` reads Unit+0xA1/+0x8C/+0xA2 through 0x005DE340 and is unmodelled;
        // a full turn per frame makes the arm reduce to the translation half.
        let outcome = movement::move_step(&mut view, &mut body, path, target, speed, i32::MAX);
        self.cover.unit_move_step += 1;
        self.world.units.x_internal_mut()[row] = body.x.rem_euclid(MAP_SPAN);
        self.world.units.y_internal_mut()[row] = body.y.rem_euclid(MAP_SPAN);
        self.world.units.angle_mut()[row] = body.angle;
        self.world.units.set_idle(row, 0);
        if matches!(outcome, movement::MoveStep::Arrived) && self.paths[row].is_empty() {
            self.world.orders_mut(row).kill_current();
            self.world.units.set_idle(row, 1);
        }
    }

    /// `Unit::do_attack` `0x005F1B80` — the range and recharge gate around
    /// `ObjectData::get_damage` `0x00644130`.
    ///
    /// The arithmetic is the derived pipeline fed by the real balance entry; the
    /// predicates are all default, so only the spine runs. Target *selection*
    /// (`Unit::fight` `0x005FD4D0`, `find_attack_pos` `0x00601280`) is uncited, so the
    /// order must already name its target.
    fn do_attack(&mut self, row: usize) {
        let Some(ord) = self.world.orders(row).current().copied() else {
            return;
        };
        if ord.target_who < 0 || ord.target_o < 0 {
            self.world.orders_mut(row).kill_current();
            return;
        }
        let recharging = self.world.units.get_recharging(row);
        if recharging > 0 {
            self.world.units.set_recharging(row, recharging - 1);
            return;
        }
        let Some(balance) = self.world.rules.balance.clone() else {
            return;
        };
        let trow = match self
            .world
            .objects
            .slot(ord.target_who as usize)
            .band(Band::Unit)
            .get(ord.target_o as usize)
            .copied()
        {
            Some(r) => r as usize,
            None => {
                self.world.orders_mut(row).kill_current();
                return;
            }
        };
        if trow >= self.world.live_count() as usize || trow == row {
            self.world.orders_mut(row).kill_current();
            return;
        }
        if self.world.units.get_flags(trow) & OBJ_FLAG_ACTIVE == 0 {
            self.world.orders_mut(row).kill_current();
            return;
        }

        let (atk_type, def_type) = (self.unit_type[row], self.unit_type[trow]);
        let (Some(atk), Some(def)) = (self.type_stats(atk_type), self.type_stats(def_type)) else {
            return;
        };

        let dx = self.world.units.x_internal()[trow] - self.world.units.x_internal()[row];
        let dy = self.world.units.y_internal()[trow] - self.world.units.y_internal()[row];
        // `combat::vector_dist` is the engine's own octagonal metric, not a hypot.
        let dist = combat::vector_dist(dx, dy);
        if dist > atk.max_range {
            // Out of range: close. Retail reaches `do_move` via `check_target_path`.
            let angle = crate::trig::find_angle(dx, dy);
            self.world.units.angle_mut()[row] = angle;
            let mut body = movement::Body {
                x: self.world.units.x_internal()[row],
                y: self.world.units.y_internal()[row],
                angle,
                stuck_budget: 0,
            };
            let mut view = MapView { map: &self.map };
            let path = &mut self.paths[row];
            let tx = self.world.units.x_internal()[trow];
            let ty = self.world.units.y_internal()[trow];
            let speed = self.world.units.myspeed()[row] as i32;
            movement::move_step(&mut view, &mut body, path, (tx, ty), speed.max(1), i32::MAX);
            self.cover.unit_move_step += 1;
            self.world.units.x_internal_mut()[row] = body.x.rem_euclid(MAP_SPAN);
            self.world.units.y_internal_mut()[row] = body.y.rem_euclid(MAP_SPAN);
            self.world.units.angle_mut()[row] = body.angle;
            return;
        }

        let Some(balance_pct) = balance.get(atk.type_id, def.type_id) else {
            return;
        };
        let input = crate::mechanics::DamageInput {
            balance_pct,
            attack: crate::mechanics::get_attack(atk.attack, false, 0, 0),
            armor: crate::mechanics::get_armor(def.armor, false, 0, 0),
            attack_dir: crate::trig::find_angle(dx, dy),
            attacker_player: self.world.units.get_who(row) as u32,
            attacker_type_id: atk.type_id,
            defender_type_id: def.type_id,
            defender_facing: self.world.units.angle()[trow],
            defender_facing_entrench: self.world.units.angle()[trow],
            current_frame: self.world.frame,
            ..Default::default()
        };
        let d = crate::mechanics::damage(
            &input,
            &crate::mechanics::DamagePredicates::default(),
            &self.world.rules.combat,
            &crate::mechanics::UnreachedTerms::default(),
        );
        self.cover.unit_attack += 1;
        match self.shooter_rules_for(atk.type_id) {
            // Object::do_launch -> Object::fire_ammo 0x0064C8B0: the damage rides the
            // projectile and lands at step 15, in pool-slot order, after every object.
            Some(rules) => self.fire_ammo(row, trow, rules, d),
            None => self.apply_damage(trow, d),
        }
        // `recharge()` is written back through a BYTE store at 0x005FF0A4, so a recharge
        // over 255 frames wraps. Reproduced rather than clamped.
        let rc = combat::recharge_frames(
            &combat::RechargeInput {
                base_recharge: atk.recharge,
                ..Default::default()
            },
            &self.combat_rules,
        );
        self.world
            .units
            .set_recharging(row, (rc as u32 & 0xFF) as u8);
    }

    /// Apply damage to a unit row and, on death, file a corpse in the ring.
    fn apply_damage(&mut self, trow: usize, d: i32) {
        self.cover.damage_applied += 1;
        self.cover.damage_total += d as i64;
        let hp = self.world.units.myhits()[trow].wrapping_sub(d);
        self.world.units.myhits_mut()[trow] = hp;
        if hp > 0 {
            return;
        }
        // Clearing the active bit is what stops `Objects::process_all` ticking it;
        // `Objects::kill_object` proper is not ported.
        let f = self.world.units.get_flags(trow);
        self.world.units.set_flags(trow, f & !OBJ_FLAG_ACTIVE);
        let rec = combat::DeathRecord {
            valid: 1,
            first_frame: self.world.frame,
            x: self.world.units.x_internal()[trow],
            y: self.world.units.y_internal()[trow],
            who: self.world.units.get_who(trow) as i32,
            o: self.world.units.o()[trow] as i32,
            ..Default::default()
        };
        let never_blocks = |_: &combat::DeathRecord| false;
        self.deaths.add_death(self.world.frame, rec, &never_blocks);
        self.cover.deaths += 1;
    }

    /// `Unit::do_build` `0x005EEBF0` -> `Wall::do_construct` `0x006434D0`.
    ///
    /// The order names a band-2000 row in `target_o`. Because this runs from inside the
    /// object traversal, the `helpers` divisor sees builders in exactly the rotated order
    /// retail sees them in — which is the input `production::construct_frame` documents as
    /// belonging to the scheduler rather than to its own module.
    fn do_build(&mut self, row: usize) {
        let Some(ord) = self.world.orders(row).current().copied() else {
            return;
        };
        let site = ord.target_o as usize;
        if ord.target_o < 0 || site >= self.builds.len() {
            return;
        }
        let rate = production::construct_rate(
            self.builds[site].is_under_attack(),
            false,
            &self.prod_rules,
        );
        let bd = &mut self.builds[site];
        let ct = production::construct_time(
            bd.constr_time,
            false,
            &production::ConstructQueryGates::default(),
            &self.prod_rules,
        );
        let step = production::do_construct(
            rate,
            1,
            bd.is_active(),
            bd.job_counter,
            bd.job_counter_2,
            bd.helpers,
            ct,
        );
        bd.job_counter = step.job_counter;
        bd.job_counter_2 = step.job_counter_2;
        bd.helpers = step.helpers;
        self.cover.build_construct_steps += 1;
        if step.completed && !bd.is_active() {
            // Build::activate(0,1,1).
            bd.flags |= production::flag::ACTIVE;
            self.cover.builds_completed += 1;
            self.world.orders_mut(row).kill_current();
        }
    }

    /// `Build::process` `0x0061EDF0`, the deterministic head we have: the `Wall::process`
    /// helper latch plus the under-construction hit-point recompute.
    fn build_process(&mut self, row: usize, _frame: i32) {
        let bd = &mut self.builds[row];
        bd.begin_frame_construction();
        let ct = production::construct_time(
            bd.constr_time,
            false,
            &production::ConstructQueryGates::default(),
            &self.prod_rules,
        );
        bd.construct_hits =
            production::construct_hits(bd.myhits, bd.is_active(), false, bd.job_counter, ct);
        self.cover.build_process += 1;
    }

    /// `Wall::process` `0x00640450` — the whole deterministic bookkeeping half, phased on
    /// `frame & 7 == who` and `(frame + o) % 32 / % 16`.
    fn wall_process(&mut self, row: usize, frame: i32) {
        let _fx = self.walls[row].process(frame);
        self.cover.wall_process += 1;
    }

    fn shooter_rules_for(&self, type_id: i32) -> Option<ammo::ShooterRules> {
        self.shooter_rules
            .iter()
            .find(|(t, _)| *t == type_id)
            .map(|(_, r)| *r)
    }

    /// `Object::fire_ammo` `0x0064C8B0` -> `Objects::add_ammo` -> `Ammo::init`.
    ///
    /// A unit shooter emits **one projectile per live guy** from that guy's position and
    /// draws no RNG for the muzzle; `Ammo::init` then draws twice for the aim scatter.
    /// Our units carry `guy_mark = 1` with no `Guy` bodies, so guy 0 stands at the unit's
    /// own position — the same stand-in the [`Gap::GuyProcess`] entry names.
    fn fire_ammo(&mut self, row: usize, trow: usize, rules: ammo::ShooterRules, damage: i32) {
        let shooter = ammo::ObjView {
            alive: true,
            is_unit: true,
            x: self.world.units.x_internal()[row],
            y: self.world.units.y_internal()[row],
            z: 0,
            guy_mark: self.world.units.guy_mark()[row].max(1) as i32,
            guy0_z: 0,
            rules,
        };
        let target = ammo::ObjView {
            alive: true,
            is_unit: true,
            x: self.world.units.x_internal()[trow],
            y: self.world.units.y_internal()[trow],
            z: 0,
            guy_mark: 1,
            guy0_z: 0,
            rules: ammo::ShooterRules::default(),
        };
        let guys = [ammo::GuyPos {
            x: shooter.x,
            y: shooter.y,
            z: 0,
        }];
        let dist = combat::vector_dist(target.x - shooter.x, target.y - shooter.y);
        let mut spawns: Vec<ammo::SpawnPoint> = Vec::new();
        let mut rng = ammo::Rng(self.world.random.state() as u32);
        ammo::fire_ammo_spawns(&shooter, &guys, &mut rng, &mut spawns);
        self.world.random.reseed(rng.0 as i32);
        let target_o = self.world.units.o()[trow] as i32;
        let target_who = self.world.units.get_who(trow) as i32;
        for sp in spawns {
            let ord = ammo::LaunchOrder {
                gpiece: 1,
                start: sp,
                who: self.world.units.get_who(row) as i32,
                o: self.world.units.o()[row] as i32,
                whom: target_who,
                ox: target_o,
                angle: crate::trig::find_angle(target.x - shooter.x, target.y - shooter.y),
                cosmetic: false,
            };
            self.launch_ammo(&ord, &shooter, Some(&target), dist, damage);
        }
    }

    fn type_stats(&self, type_id: i32) -> Option<crate::world::UnitTypeStats> {
        if type_id <= 0 {
            return None;
        }
        self.world
            .rules
            .unit_stats
            .iter()
            .find(|s| s.type_id == type_id)
            .copied()
    }

    // -- step 15 ----------------------------------------------------------------------

    /// `Objects::inc_time` `0x0065DB70`.
    ///
    /// Retail advances every object's animation clock here and flies every projectile.
    /// Only the projectile half is ported. It runs in **pool-slot order**, after every
    /// unit, building and wall has processed — so an impact that kills a unit does so at
    /// the end of the tick, and the corpse enters the death ring in slot order.
    fn objects_inc_time(&mut self) -> (StepRun, u32) {
        self.cover.gaps[Gap::UnitIncTime.index()] += 1;
        if self.ammo.live() == 0 {
            return (StepRun::Vacuous, 0);
        }
        let mut work = 0u32;
        // `(slot, call)` in pool-slot order. `Object::do_damage` is not this module's, so
        // the calls are collected and applied below.
        let mut calls: Vec<(usize, ammo::DamageCall)> = Vec::new();
        {
            let view = AmmoView {
                map: &self.map,
                units: &self.world.units,
                objects: &self.world.objects,
                unit_type: &self.unit_type,
                shooter_rules: &self.shooter_rules,
                live: self.world.live_count() as usize,
            };
            // `Ammo::do_damage`'s ground jitter draws the sim stream twice; bridge the
            // state in and back out so the pool shares one stream with everything else.
            let mut rng = ammo::Rng(self.world.random.state() as u32);
            for slot in 0..self.ammo.slots.len() {
                if !self.ammo.slots[slot].occupied() {
                    continue;
                }
                let step = {
                    let a = &mut self.ammo.slots[slot];
                    // `hit_target` takes `&mut AmmoWalk` while `ammo_inc_time`'s hook is
                    // `Fn(&AmmoWalk)`, so the test runs on a copy: the boolean is right and
                    // the mutation it makes on failure (forgetting the target) is deferred
                    // to `ammo_do_damage_single`, which re-runs it properly. See the report.
                    let probe = |w: &ammo::AmmoWalk| {
                        let mut c = *w;
                        let t = ammo::AmmoEnv::object(&view, c.whom, c.ox);
                        ammo::hit_target(&mut c, t.as_ref(), true)
                    };
                    ammo::ammo_inc_time(a, &view, probe, |w| ammo::check_hit(w, &view))
                };
                self.cover.ammo_steps += 1;
                work += 1;
                match step {
                    ammo::Step::Impact => {
                        let (whom, ox) = {
                            let w = &self.ammo.slots[slot].w;
                            (w.whom, w.ox)
                        };
                        let target = ammo::AmmoEnv::object(&view, whom, ox);
                        let imp = ammo::ammo_do_damage_single(
                            &mut self.ammo.slots[slot],
                            &view,
                            target.as_ref(),
                            true,
                            &mut rng,
                        );
                        self.cover.ammo_impacts += 1;
                        if imp.closed {
                            self.cover.ammo_closed += 1;
                        }
                        for c in imp.calls {
                            calls.push((slot, c));
                        }
                    }
                    ammo::Step::Closed => self.cover.ammo_closed += 1,
                    ammo::Step::Flying => {}
                }
            }
            self.world.random.reseed(rng.0 as i32);
        }
        // `Object::do_damage` — the damage each projectile carried, applied in the order
        // the engine issued the calls, which is pool-slot order at the end of the tick.
        for (slot, c) in calls {
            let dmg = self.shots.get(slot).map(|s| s.damage).unwrap_or(0);
            if dmg == 0 {
                continue;
            }
            let Some(&trow) = self
                .world
                .objects
                .slot(c.victim_who.max(0) as usize)
                .band(Band::Unit)
                .get(c.victim_o.max(0) as usize)
            else {
                continue;
            };
            let trow = trow as usize;
            if trow < self.world.live_count() as usize
                && self.world.units.get_flags(trow) & OBJ_FLAG_ACTIVE != 0
            {
                self.apply_damage(trow, dmg);
            }
        }
        (StepRun::Executed, work)
    }

    /// Launch a projectile into the pool the way `Objects::add_ammo` + `Ammo::init` do,
    /// **on the simulation RNG stream**.
    ///
    /// `ammo.rs` carries its own [`ammo::Rng`], bit-identical to [`crate::rng::Random`]
    /// (same LCG, same low-16 mapping). Bridging the state in and back out keeps the two
    /// scatter draws inside the one stream instead of forking it.
    pub fn launch_ammo(
        &mut self,
        ord: &ammo::LaunchOrder,
        shooter: &ammo::ObjView,
        target: Option<&ammo::ObjView>,
        dist: i32,
        damage: i32,
    ) -> usize {
        let slot = self.ammo.alloc_slot();
        // Ammo::init's anti-air dud roll draws 1-2 values before anything else and is
        // unported; every launch is therefore a point where our stream leaves retail's.
        self.cover.gaps[Gap::AmmoAntiAirDud.index()] += 1;
        self.cover.rng_draws_missing += 1;
        let mut rng = ammo::Rng(self.world.random.state() as u32);
        let mut index = self.ammo.ammo_index;
        let a = ammo::ammo_init(
            slot,
            &mut index,
            ord,
            shooter,
            target,
            ammo::MissRadius::Formula,
            dist,
            &mut rng,
        );
        self.ammo.ammo_index = index;
        self.world.random.reseed(rng.0 as i32);
        if slot >= self.shots.len() {
            self.shots.resize(slot + 1, AmmoShot::default());
        }
        self.shots[slot] = AmmoShot { damage };
        self.ammo.slots[slot] = a;
        slot
    }

    // -- reporting --------------------------------------------------------------------

    /// The `check_all`-shaped digest of the channels this driver produces. **Not**
    /// comparable with a retail checksum: only some channels have runtime producers, and
    /// `CheckSums::check_all` `0x00936560` returns the fifteenth channel's accumulator
    /// while the sum goes on the wire from `CommandManager::issue_check_sums` `0x00940770`.
    pub fn channel_digest(&self) -> u64 {
        let mut h: u64 = 0;
        let mut mix = |v: u32| {
            h = h
                .wrapping_mul(0x100_0000_01B3)
                .wrapping_add(v as u64)
                .rotate_left(7);
        };
        mix(self.world.digest() as u32);
        mix((self.world.digest() >> 32) as u32);
        mix(self.ammo.checksum());
        for l in self.leaders.iter() {
            mix(l.econ.adler32());
        }
        mix(economy::market_adler32(&self.market));
        mix(self.map.world.checksum());
        for w in self.walls.iter() {
            mix(adler32(1, &w.walk_bytes()));
        }
        for b in self.builds.iter() {
            mix(adler32(1, &b.image()));
        }
        mix(self.world.frame as u32);
        mix(self.world.random.state() as u32);
        h
    }

    /// The line the wave is measured by, plus the honest denominators around it.
    pub fn coverage_report(&self) -> String {
        let c = &self.cover;
        let (imp, stub, oos) = crate::schedule::ScheduleCoverage::tally();
        let mut s = String::new();
        s.push_str(&format!(
            "ticks {}\n\
             steps executed on every tick : {} / {}\n\
             steps executed on some tick  : {} / {}\n\
             steps ported but vacuous     : {}\n\
             steps out of scope           : {}\n\
             schedule.rs hand table says  : {} implemented, {} stub, {} out of scope\n",
            c.ticks,
            c.always_executed(),
            NUM_STEPS,
            c.ever_executed(),
            NUM_STEPS,
            c.ever_vacuous(),
            c.out_of_scope.iter().filter(|&&n| n > 0).count(),
            imp,
            stub,
            oos,
        ));
        s.push_str("\nper step:\n");
        for i in 0..NUM_STEPS {
            let d = &DO_FRAME[i];
            let scope = match d.status {
                StepStatus::Implemented => "impl",
                StepStatus::Stub => "stub",
                StepStatus::OutOfScope => "oos ",
            };
            s.push_str(&format!(
                "  {:>2} {:<44} {} exec={:<7} vac={:<7} skip={:<7} work={}\n",
                i, d.name, scope, c.executed[i], c.vacuous[i], c.unimplemented[i], c.work[i]
            ));
        }
        s.push_str("\nwhat the driven systems did:\n");
        for (k, v) in [
            ("Unit::process", c.unit_process),
            ("Unit::move_step", c.unit_move_step),
            ("Unit::do_attack", c.unit_attack),
            ("damage applications", c.damage_applied),
            ("deaths filed", c.deaths),
            ("hold_frames decrements", c.hold_decrements),
            ("Build::process", c.build_process),
            ("Wall::do_construct steps", c.build_construct_steps),
            ("buildings completed", c.builds_completed),
            ("Wall::process", c.wall_process),
            ("Ammo::inc_time", c.ammo_steps),
            ("ammo impacts", c.ammo_impacts),
            ("fog cells newly explored", c.fog_cells_revealed),
            ("territory tiles claimed", c.border_tiles),
            ("Groups::process passes", c.group_normalises),
            ("Leader::gather calls", c.leader_gathers),
            ("market cycles", c.market_cycles),
            ("pathfinder searches", c.paths_searched),
            ("pathfinder failures", c.path_search_failures),
            ("Herd::process steps", c.herd_steps),
        ] {
            s.push_str(&format!("  {k:<28} {v}\n"));
        }
        s.push_str("\nnamed gaps (retail calls with no port), by times skipped:\n");
        let mut rows: Vec<(u64, &str)> = (0..Gap::COUNT)
            .map(|i| (c.gaps[i], GAP_NOTES[i]))
            .filter(|(n, _)| *n > 0)
            .collect();
        rows.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
        for (n, note) in rows {
            s.push_str(&format!("  {n:>8}  {note}\n"));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small populated world: four players, units, a construction site, walls, herds.
    fn populated(seed: u64) -> Sim {
        let mut sim = Sim::new(seed, 16);
        for who in 0..4 {
            sim.activate(who);
        }
        for who in 0..4 {
            for k in 0..6 {
                let x = 3000 + (who as i32) * 700 + k * 130;
                let y = 3000 + (k % 3) * 260;
                let h = sim.spawn_unit(who, 0, x, y, 4).unwrap();
                if k % 2 == 0 {
                    sim.issue(h, Order::move_to(x + 900, y + 640, 48));
                }
            }
            let mut bd = production::BuildData {
                constr_time: 240,
                myhits: 400,
                ..Default::default()
            };
            bd.flags |= production::flag::VALID | production::flag::STARTED;
            sim.spawn_build(who, bd);
            let mut w = walls::WallState::default();
            w.flags |= 1;
            sim.spawn_wall(who, w);
        }
        sim.herds.push(casters_animals::HerdData {
            cx: 8,
            cy: 8,
            type_id: 0,
            herd_flags: 1,
            ..Default::default()
        });
        sim
    }

    #[test]
    fn the_tick_executes_and_reports_what_it_executed() {
        let mut sim = populated(1);
        let t = sim.do_frame();
        // The steps this driver claims to run.
        for s in [8, 11, 12, 14, 20, 23] {
            assert!(
                t.steps[s].ran(),
                "step {s} ({}) did not execute: {:?}",
                DO_FRAME[s].name,
                t.steps[s]
            );
        }
        assert!(
            t.executed() >= 6,
            "expected at least six executed steps, got {}",
            t.executed()
        );
        // Every step is accounted for exactly once.
        assert_eq!(
            t.executed() + t.vacuous() + t.unimplemented() + t.out_of_scope(),
            NUM_STEPS
        );
    }

    /// Steps 15 and 14's herd arm are population-gated, so a run long enough to reach
    /// frame 64 with a projectile in the air must light them up too.
    fn with_a_projectile(sim: &mut Sim) {
        let shooter = ammo::ObjView {
            alive: true,
            is_unit: true,
            x: 3000,
            y: 3000,
            ..Default::default()
        };
        let target = ammo::ObjView {
            alive: true,
            is_unit: true,
            x: 4000,
            y: 3600,
            ..Default::default()
        };
        let ord = ammo::LaunchOrder {
            gpiece: 1,
            start: ammo::SpawnPoint {
                x: 3000,
                y: 3000,
                z: 40,
            },
            who: 0,
            o: 0,
            whom: 1,
            ox: 1,
            angle: 0,
            cosmetic: false,
        };
        sim.launch_ammo(&ord, &shooter, Some(&target), 1200, 7);
    }

    #[test]
    fn projectiles_fly_at_step_15_after_every_object() {
        let mut sim = populated(2);
        with_a_projectile(&mut sim);
        let t = sim.do_frame();
        assert!(t.steps[15].ran(), "Objects::inc_time did not run");
        assert!(sim.cover.ammo_steps > 0);
    }

    /// The whole point of the file: the same seed produces the same trace and the same
    /// state, tick for tick.
    #[test]
    fn stepping_is_deterministic() {
        let mut a = populated(7);
        let mut b = populated(7);
        with_a_projectile(&mut a);
        with_a_projectile(&mut b);
        for f in 0..40 {
            let ta = a.do_frame();
            let tb = b.do_frame();
            assert_eq!(ta.steps, tb.steps, "trace diverged at frame {f}");
            assert_eq!(ta.work, tb.work, "work diverged at frame {f}");
            assert_eq!(
                a.channel_digest(),
                b.channel_digest(),
                "state diverged at frame {f}"
            );
        }
    }

    /// Sims are independent, so stepping many of them on threads must reproduce serial
    /// stepping exactly — the same guarantee `Batch` gives for `World::step`.
    #[test]
    fn parallel_sims_reproduce_serial() {
        let serial: Vec<u64> = (0..8)
            .map(|i| {
                let mut s = populated(i as u64);
                s.run(25);
                s.channel_digest()
            })
            .collect();
        for threads in [1usize, 2, 4] {
            let mut sims: Vec<Sim> = (0..8).map(|i| populated(i as u64)).collect();
            let chunk = sims.len().div_ceil(threads);
            std::thread::scope(|scope| {
                for part in sims.chunks_mut(chunk) {
                    scope.spawn(move || {
                        for s in part.iter_mut() {
                            s.run(25);
                        }
                    });
                }
            });
            let got: Vec<u64> = sims.iter().map(|s| s.channel_digest()).collect();
            assert_eq!(got, serial, "thread count {threads} changed the result");
        }
    }

    /// `Objects::process_all` visits owner slots in `(frame + i) % 10` order, and the
    /// frame counter increments *after* the pass — so tick `n` uses rotation `n`.
    #[test]
    fn the_object_pass_uses_the_pre_increment_frame() {
        let mut sim = populated(3);
        for expected in 0..12 {
            let t = sim.do_frame();
            assert_eq!(t.frame, expected, "trace frame is the pre-increment value");
        }
        assert_eq!(sim.world.frame, 12);
    }

    /// A step with a body but nothing to do reports `Vacuous`, never `Executed`. An empty
    /// world must not be able to inflate the number this wave is judged by.
    #[test]
    fn an_empty_world_executes_only_the_counters() {
        let mut sim = Sim::new(5, 8);
        let t = sim.do_frame();
        assert!(!t.steps[8].ran(), "no active leaders, so no economy ran");
        assert!(!t.steps[14].ran(), "no objects, so no object pass ran");
        assert!(!t.steps[15].ran(), "no projectiles, so no flight ran");
        assert!(t.steps[20].ran() && t.steps[23].ran());
        assert!(
            t.executed() <= 2,
            "an empty world executed {} steps",
            t.executed()
        );
    }

    /// Construction is driven from inside the object traversal, so the `helpers` divisor
    /// sees builders in the rotated order. Two builders on one site in one frame must
    /// apply `rate + rate/2`, not `2*rate`.
    #[test]
    fn two_builders_in_one_frame_divide_the_rate() {
        let mut sim = Sim::new(11, 16);
        sim.activate(0);
        let mut bd = production::BuildData {
            constr_time: 100_000,
            myhits: 400,
            ..Default::default()
        };
        bd.flags |= production::flag::VALID | production::flag::STARTED;
        let site = sim.spawn_build(0, bd);
        for k in 0..2 {
            let h = sim.spawn_unit(0, 0, 2000 + k * 100, 2000, 2).unwrap();
            let mut o = Order::default();
            o.kind = OrderIndex::BuildAt;
            o.target_who = 0;
            o.target_o = site as i16;
            sim.issue(h, o);
        }
        sim.do_frame();
        let rate = production::construct_rate(false, false, &sim.prod_rules);
        assert_eq!(
            sim.builds[site].job_counter,
            (rate + rate / 2) as u32,
            "the second builder must divide by helpers+1"
        );
        // And `helpers` is a single-frame accumulator: `Build::process` clears it.
        sim.do_frame();
        assert_eq!(
            sim.builds[site].job_counter,
            2 * (rate + rate / 2) as u32,
            "helpers must reset every frame"
        );
    }

    /// Fog is stamped by `update_seen` at step 12, before anything moves.
    #[test]
    fn units_explore_the_fog_plane() {
        let mut sim = Sim::new(13, 16);
        sim.activate(0);
        sim.spawn_unit(0, 0, 6000, 6000, 6).unwrap();
        assert_eq!(sim.cover.fog_cells_revealed, 0);
        sim.do_frame();
        assert!(
            sim.cover.fog_cells_revealed > 0,
            "no fog cell was newly explored"
        );
    }

    /// Every `Gap` has a note, and every note names its step.
    #[test]
    fn every_gap_is_named() {
        assert_eq!(GAP_NOTES.len(), Gap::COUNT);
        for n in GAP_NOTES.iter() {
            assert!(n.starts_with("step "), "gap note lacks its step: {n}");
        }
    }
}
