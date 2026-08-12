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
//! Sub-calls inside a step are counted the same way. `Leaders::process_all` now executes
//! its recovered dispatcher and object-band traversals; resolved base Object virtuals
//! write real state and remaining bodies are charged at the call sites, so "step 8 executed"
//! does not silently mean "every second-level body exists". [`Coverage::gaps`] is that
//! ledger.
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
//! * **Step 15's own object walk does not rotate and never visits the wall band**
//!   (`0x0065DB87`/`0x0065DBFF`: ten leader records at `0x00E3A390` stride `0x6EEC`, bounded
//!   only by `Objects+0x15C` and `Objects+0x184`). Reusing step 14's rotated traversal here
//!   would be wrong, and step 15 covers the building band for all ten owners rather than the
//!   eight step 14 uses. Inside step 15 the object bands run **before** the ammo pool, and
//!   the death ring runs after it.
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
use crate::script_runtime::{ScriptRunError, ScriptRuntime};
use crate::systems::{
    ammo, borders_fog, canonical_strafe_runtime, casters_animals, collision_blocks_live, combat,
    defeat_cleanup, economy, game_daemon_step12, groups_guys, leaders, movement, movement_driver,
    movement_live, order_dispatch, production,
    sparse_object_bands_authority_frontier::{RetailBand, SparseSlotLifecycle, TraversalEntry},
    special_anim_executor, step12_visibility_producer_frontier, step12_visibility_runtime,
    tech_cities, unit_inctime, victory_score, walls, wonders,
};
use crate::world::{Handle, World, WorldObjectIdentity, MAP_SPAN, OBJ_FLAG_ACTIVE};

/// The `world` channel's own store, consolidated into `map_terrain` by the sibling lane.
/// Aliased because this file also names [`crate::world::World`], which is the unit SoA.
pub use crate::systems::map_terrain::World as TerrainWorld;

#[path = "systems/leader_match_host.rs"]
pub mod leader_match_host;
/// The `Sim`-side host for the recovered player-lifecycle command tails — the link between
/// the command wire and `Leader::defeat` `0x006ECB00` / `Game::check_victory` `0x005926B0`.
///
/// Declared here rather than in `systems/mod.rs` for two reasons: it is a child of this
/// module, so it can reach `Sim`'s private terminal-cleanup drain the way step 11 does; and
/// it follows `command.rs`'s `#[path]` idiom for host modules owned by one driver.
#[path = "systems/lifecycle_host.rs"]
pub mod lifecycle_host;
/// CityPool-backed resolution of the last opcode-70/71 simulation boundary,
/// `LeaderData::find_capital` `0x006EB930`.
#[path = "systems/lifecycle_opcode_cohort.rs"]
pub mod lifecycle_opcode_cohort;

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
    GameDaemonUpdateAllSeen,
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
    RoadsScanStray,
    WonderValueWorld,
    NukeDoDamage,
    UnitExecuteEvents,
    WallIncTime,
    DeathObjIncTime,
    FarmsIncTime,
}

impl Gap {
    pub const COUNT: usize = Gap::FarmsIncTime as usize + 1;
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
    "step 4  RunTimeEnv::run_script 0x0043D0E0 - runtime is wired; unrecovered ScenarioFuncSet builtins fail the tick closed",
    "step 8  calc_wall_stats - +0x4c and plain-wall +0x15c/+0x160 execute; Wall override pair and update_construct_time remain",
    "step 8  calc_unit_stats - +0xe8/+0x15c/+0x160, ObjectData::armor, and Unit::update_armor suffix execute; Unit::update_speed and automatic armor gate population remain",
    "step 8  Leader::process_taunt 0x006b8cc0 - exact table dispatch executes; AI-chat body absent",
    "step 11 Leader::check_explore leaders.cpp:26413 - uncited",
    "step 11 Leader::plan_strategy leaders.cpp:26880 (11 KB) - uncited",
    "step 11 Leader::diplomacy 0x006BC950 (20,348 B) - deliberately not ported; a self-play agent replaces it",
    "step 12 GameDaemon::calc_danger 0x00732D10 - body absent; exact scheduler charges only frame % 200 == 0",
    "step 12 GameDaemon::update_all_seen 0x00732840 - exact Unit pass preflights; plane clear remains blocked on Build/Wall/reveal_fog ownership",
    "step 12 GameDaemon::process_coll_blocks 0x00731F90 - body and persistent live cursor execute; dormant trace slot records only bridge-invariant failure",
    "step 13 Armies::process_all 0x006F3B00 - exact dispatcher/prefix executes; valid armies require their complete Group/Unit/City/type host and reached AI bodies remain explicit",
    "step 14 Unit::suffer_attrition - borders_fog::step_attrition exists but needs supply/territory state this driver does not build",
    "step 14 Unit::process_supply unit.cpp:29845 - uncited",
    "step 14 Guy::process 0x005E0230 / Guy::move 0x005D9240 - groups_guys::GuyData exists; per-guy bodies are not populated",
    "step 14 Unit::detect_unit_collision 0x00617060 - detector/resolver/driver execute when every live unit supplies authoritative type/Guy/order/spatial facts; missing sources and repath suspension fail closed",
    "step 14 Ammo::init anti-air dud roll - unported; it draws game_random 1-2 times per launch, so every launch shifts the stream",
    "step 14 UnitData::needs_transport 0x00609920 - unported; the UnitWorld view answers 0",
    "step 14 Objects::process_all wildlife spawn (frame%32) - draws game_random an unknown number of times; drawing wrongly is worse than not drawing",
    "step 15 Unit::inc_time 0x00610B40 (vtable +0xA0) - the exact 0x00610B43 gate executes over live inside_up/TypeIndex; the guy clocks it owes stay absent because Guy::inc_time 0x005D9E10 runs Guy::set_anim 0x005DA300, whose head resolves the animation and draws game_random 0-1 times per activation with unbounded recursion",
    "step 22 Roads::scan_and_kill_stray_roads - exact scanner executes; live road tiles without their renderer-owned RoadElementCandidate fail closed",
    "step 12 Wonder value/net supply - completed records exist, but a missing/stale object-type world blocks the Wonder victory subpass",
    "step 15 Nuke::do_damage 0x0092BC80 - called unconditionally at 0x0065DB82 ahead of every loop; reaches Object::do_damage and Leader::action_declare, and no nuke registry exists here (Nuke::add_nuke 0x0092BA30 is only reachable from Ammo::do_damage)",
    "step 15 Unit::execute_events 0x0060EDC0 (vtable +0x154) - the exact path branch executes; the ExecuteGuyEvents arm needs live Guy storage plus the shipped GraphicEvents tables and the RELEASE/RELEASE_PLANE sinks",
    "step 15 Wall::inc_time 0x0063FB60 - the building band 2000..build_mark is walked, but the 2,273-byte body is unported; its simulation payload is Wall::update_hits 0x0063F0D0 on the !is_active arm plus GraphicEvents::execute_game_events",
    "step 15 DeathObj::inc_time 0x008D5240 - the exact body is systems::death_inctime, still unadmitted: no authoritative gpiece/type pack, no clear_blocking terrain adapter, no Scene::recalc_deaths field",
    "step 15 Farms::inc_time 0x008D8600 - the tail call is unconditional but its body is empty when Farms::num is 0 and its two game_random sites are per-farm and conditional; this Sim has no Farms array to establish either",
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
    /// SPECIAL_ANIM frames which reached a complete local transaction (currently object-free
    /// EXIT).
    pub special_anim_completed: u64,
    /// Host-free SPECIAL_UNIT no-op frames reached through the real object pass.
    pub special_anim_working: u64,
    /// ENTER/Airbase-EXIT frames refused before mutation because a required world surface
    /// is not installed.
    pub special_anim_host_refused: u64,
    /// Malformed payload or broken host attestations rejected without publication.
    pub special_anim_malformed: u64,
    /// Exact `Unit::do_trade` subpaths committed through the canonical typed order owner.
    pub trade_route_committed: u64,
    /// TRADE_ROUTE activations held before mutation because their required branch is not owned.
    pub trade_route_refused: u64,
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
    /// Step-15 unit-band `Unit::inc_time` `0x00610B40` entries whose `0x00610B43` gate
    /// passed, i.e. units that owe guy clocks this tick.
    pub inc_time_units: u64,
    /// Unit-band entries the same gate rejected. Retail does nothing for these, so this
    /// count is exactly reproduced work, not a gap.
    pub inc_time_units_gated: u64,
    /// Step-15 building-band `Wall::inc_time` `0x0063FB60` call sites reached.
    pub inc_time_builds: u64,
    /// `Unit::execute_events` `0x0060EDC0` entries that took the `VerifyGraphicLoads` arm —
    /// `GraphicEvents::verify_load` `0x008E4780` over the guy prefix, which is graphics
    /// resource admission rather than simulation.
    pub inc_time_verify_paths: u64,
    /// Nonzero-`valid` corpses the step-15 death loop reached.
    pub inc_time_deaths_visited: u64,
    pub crash_spawned: u64,
    pub crash_ineligible: u64,
    pub crash_missing_facts: u64,
    pub fog_cells_revealed: u64,
    pub border_tiles: u64,
    pub group_normalises: u64,
    pub leader_gathers: u64,
    pub leader_hostile_frames: u64,
    pub leader_wall_stat_passes: u64,
    pub leader_unit_stat_passes: u64,
    pub leader_stat_objects_visited: u64,
    /// `Wall::update_construct_time` `0x0063D560` calls that resolved their type package
    /// and stored `constr_time`. Both stat-pass bands direct-call the same body.
    pub leader_construct_time_updates: u64,
    pub leader_timer_creeps: u64,
    pub leader_taunt_dispatches: u64,
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
            special_anim_completed: 0,
            special_anim_working: 0,
            special_anim_host_refused: 0,
            special_anim_malformed: 0,
            trade_route_committed: 0,
            trade_route_refused: 0,
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
            inc_time_units: 0,
            inc_time_units_gated: 0,
            inc_time_builds: 0,
            inc_time_verify_paths: 0,
            inc_time_deaths_visited: 0,
            crash_spawned: 0,
            crash_ineligible: 0,
            crash_missing_facts: 0,
            fog_cells_revealed: 0,
            border_tiles: 0,
            group_normalises: 0,
            leader_gathers: 0,
            leader_hostile_frames: 0,
            leader_wall_stat_passes: 0,
            leader_unit_stat_passes: 0,
            leader_stat_objects_visited: 0,
            leader_construct_time_updates: 0,
            leader_timer_creeps: 0,
            leader_taunt_dispatches: 0,
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
/// This compatibility view remains for setup pathfinding and the not-yet-migrated attack chase.
/// Live `MOVE_TO`/`FLEE_TO` integration uses [`movement_live::InstalledMoveWorld`] and the typed
/// collision driver instead. `needs_transport` is still a named gap; this view's boolean
/// `unit_collides` answer is never authoritative for an installed movement actor.
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
    world: &'a World,
    unit_type: &'a [i32],
    shooter_rules: &'a [(i32, ammo::ShooterRules)],
}

impl<'a> AmmoView<'a> {
    /// `(who, o)` is the engine's object address: `o` indexes the owner's band, not the
    /// column row. Resolving it here is what keeps `check_hit`'s `find_unit_near` result
    /// usable by `object()`.
    fn row_of(&self, who: i32, o: i32) -> Option<usize> {
        self.world.unit_row_at(who, o)
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
            alive: self.world.units.get_flags(row) & OBJ_FLAG_ACTIVE != 0,
            is_unit: true,
            x: self.world.units.x_internal()[row],
            y: self.world.units.y_internal()[row],
            z: 0,
            guy_mark: self.world.units.guy_mark()[row] as i32,
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
            let mark = self.world.unit_mark(who).unwrap_or(0);
            for o in 0..mark {
                let Some(row) = self.world.unit_row_at(who as i32, o) else {
                    continue;
                };
                if self.world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
                    continue;
                }
                if who as i32 == shooter_who {
                    continue;
                }
                let d = ammo::vector_dist(
                    self.world.units.x_internal()[row] - x,
                    self.world.units.y_internal()[row] - y,
                );
                if d > 2 * 192 {
                    continue;
                }
                if best.is_none_or(|(_, _, bd)| d < bd) {
                    best = Some((who as i32, o, d));
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

/// Exact live Guy storage and the owning Object virtual result needed by the aircraft-crash
/// death adapter. This is optional because the current lightweight Sim does not synthesize PDB
/// Guy bodies; an absent source must suppress the wreck without consuming RNG.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CrashUnitSource {
    pub guys: groups_guys::UnitGuys,
    pub gpiece: Option<i32>,
}

/// Result of attempting the retail `Objects::kill_guy` wreck arm from the live driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveCrashOutcome {
    Spawned(usize),
    Ineligible,
    MissingFacts,
}

/// The checksum-relevant-independent state touched by `TurnControl::check_cannon_time`.
///
/// Field order mirrors `TurnControl +0x24..+0x30`: active player, start frame, pending
/// speed, current speed. UI messages, sound, and camera notification are presentation
/// effects and deliberately stay outside the headless core.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CannonTimeState {
    pub active_player: i32,
    pub start_frame: i32,
    pub pending_speed: i32,
    pub current_speed: i32,
}

impl Default for CannonTimeState {
    fn default() -> Self {
        CannonTimeState {
            active_player: -1,
            start_frame: 0,
            pending_speed: 0,
            current_speed: crate::schedule::SPEED_NORMAL as i32,
        }
    }
}

impl CannonTimeState {
    /// `TurnControl::check_cannon_time` `0x009579E0`, followed by the state-writing
    /// portion of `TurnControl::end_cannon_time` `0x00956470` [measured].
    ///
    /// Retail expires when signed `Game::frame - start_frame >= 75`, sets the active
    /// player to `-1`, adopts `pending_speed` when it differs, and finally clears the
    /// pending value. Returns whether expiry occurred this frame.
    pub fn check(&mut self, frame: i32) -> bool {
        if self.active_player < 0 || frame.wrapping_sub(self.start_frame) < 75 {
            return false;
        }
        self.active_player = -1;
        if self.current_speed != self.pending_speed {
            self.current_speed = self.pending_speed;
        }
        self.pending_speed = 0;
        true
    }
}

// =======================================================================================
// The simulation
// =======================================================================================

/// Why a runtime Unit-type source cannot be installed into this [`Sim`] lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitTypeStatSourceInstallError {
    AlreadyInstalled {
        installed: leaders::UnitTypeStatSourceProvenance,
        offered: leaders::UnitTypeStatSourceProvenance,
    },
    SimulationStarted {
        frame: i32,
        completed_ticks: u64,
    },
    SimulationPopulated {
        live_units: u32,
        registered_objects: usize,
        unit_type_rows: usize,
        builds: usize,
        walls: usize,
        herds: usize,
    },
    LeadersActivated,
}

/// A world plus the state its tick needs, and the executable `Game::do_frame`.
///
/// Deliberately not `Clone`: `movement::PathFinder` owns search containers that are scratch
/// rather than state, and cloning them would make two sims look different when they are not.
pub struct Sim {
    pub world: World,

    // ---- step 8 / 12: the economy -----------------------------------------------------
    pub econ_rules: economy::EconRules,
    /// The instruction-derived `Leader` slice driven by retail step 8. `LeaderSlot`
    /// remains the shared economy/border façade used by the later tick steps; economy
    /// inputs and outputs are synchronized at the step-8 boundary until those consumers
    /// migrate onto this exact layout.
    pub step8: leaders::Leaders,
    pub step8_env: leaders::Step8Env,
    pub step8_rules: leaders::Step8Rules,
    /// Most recent receipt-complete step-19 pass. This is diagnostic/product-boundary
    /// state, not part of the retail checksum walk.
    pub last_event_frame_trace: leaders::EventFrameTrace,
    pub leaders: [LeaderSlot; NUM_LEADERS],
    pub market: economy::MarketState,

    /// Retained scalar ScenarioData configuration used by exact BHS mutations.
    pub scenario_data: crate::script_runtime::ScenarioDataState,

    // ---- step 11 / 12: score and victory ----------------------------------------------
    pub vic_match: victory_score::Match,
    pub vic_leaders: victory_score::Leaders,
    /// Checksum-owned `Cities` pool (`0x00C09960`) and the eight authoritative
    /// `LeaderData::city_mark` high-water marks. Capture, trade, economy, and checksum
    /// consumers share this one store.
    pub cities: tech_cities::CityPool,
    /// Live `GameInfo::player[8]` (`Game+0x44`, stride `0x8C`) plus the `Game`/`Console`/
    /// `DropControl` scalars the player-lifecycle command tails read, which
    /// [`victory_score::Match`] does not model.
    ///
    /// `None` is the fail-closed default: without it [`Sim::tail_command_facts`] reports
    /// `NoExternalFacts` and rows 70/71/80 keep their whole-row boundary. Install one to
    /// let a resign/quit/drop actually reach `Leader::defeat`.
    pub players: Option<lifecycle_host::PlayerTable>,
    /// Completed-Wonder records called by `Build::activate` and read by Wonder victory.
    pub wonders: wonders::Wonders,
    /// Exact object/type/game store for Wonder initialization and live value queries.
    /// It is optional at construction time, but any active Wonder makes it mandatory.
    pub wonder_world: Option<Box<dyn wonders::WonderWorld + Send>>,
    /// Last fail-closed Wonder supply error. Cleared after a successful supply pass.
    pub wonder_error: Option<wonders::WonderError>,
    /// Last fail-closed defeated-player Unit sweep error. The owner request remains armed
    /// until its installed type/path facts can be preflighted as one transaction.
    pub defeat_cleanup_error: Option<defeat_cleanup::DefeatCleanupError>,
    pub cannon_time: CannonTimeState,

    // ---- step 12: fog, borders, groups ------------------------------------------------
    pub map: MapState,
    /// PDB-shaped `GameDaemon` state driven by the exact step-12 shell.
    pub game_daemon: game_daemon_step12::GameDaemonState,
    /// Revisioned join of instance detector provenance, visibility type facts, exact leader
    /// counts/HeroesData, Constants, and projection facts for the Unit visibility subpass.
    pub step12_visibility: step12_visibility_runtime::Step12VisibilityAuthority,
    /// Most recent scheduled full-refresh refusal. This is diagnostic state, not a retail
    /// walked field; the checksum-visible planes remain unchanged when it is populated.
    pub step12_visibility_error: Option<step12_visibility_runtime::Step12VisibilityPreflightError>,
    /// Exclusive persistent cursor/live-world adapter for `process_coll_blocks`.
    /// `game_daemon.empty_colls` mirrors this runtime and is preflighted every pass.
    pub collision_blocks: collision_blocks_live::CollisionBlockRuntime,
    pub road_scan: crate::systems::roads::RoadScanState,
    pub groups: groups_guys::Groups,
    /// Receive-side opcode-0 selection cache, keyed by network `play`. This is persisted in
    /// DoNSave v13; package-local Group scratch and transport serials are not.
    pub command_package_state: crate::systems::canonical_group_move_host::CommandPackageState,
    /// Revision/digest-bound object/type answers absent from the generated World columns.
    /// This is an installed content adapter, not a second persistent gameplay owner.
    pub group_move_authority: crate::systems::canonical_group_move_host::GroupMoveAuthority,
    /// Revision/digest-bound Board/Repair/Trade facts and sparse object bindings. Like the
    /// movement projection, this is reinstalled after load and is not a second gameplay owner.
    pub economy_group_authority:
        crate::systems::canonical_economy_group_host::EconomyRuntimeAuthority,
    /// Revision/digest-bound registry/caravan/terrain facts for admitted `Unit::do_trade`
    /// branches. This is a load-time adapter, not a second persistent order owner.
    pub trade_route_authority:
        crate::systems::canonical_trade_route_runtime::TradeRouteRuntimeAuthority,
    /// Last exact production activation or refusal. Diagnostic only; canonical effects live in
    /// World/order/path owners and this record is not part of DoNSave.
    pub last_trade_route_receipt:
        Option<crate::systems::canonical_trade_route_runtime::TradeRouteActivationReceipt>,
    pub last_trade_route_error:
        Option<crate::systems::canonical_trade_route_runtime::TradeRouteRuntimeError>,
    /// Reinstalled content/search projection and transaction epochs for STRAFE row 16.
    pub strafe_runtime_authority: canonical_strafe_runtime::StrafeRuntimeAuthority,
    /// Most recent canonical STRAFE commit or refusal. Diagnostic only; gameplay effects live
    /// in World/order/path/RNG/Unit/ammo owners.
    pub last_strafe_receipt: Option<canonical_strafe_runtime::CanonicalStrafeCommitReceipt>,
    pub last_strafe_error: Option<canonical_strafe_runtime::CanonicalStrafeRuntimeError>,

    // ---- step 13: standing AI armies -------------------------------------------------
    pub armies: crate::systems::armies::Armies,
    pub army_leader_flags2: [u32; NUM_LEADERS],

    // ---- step 14: the object bands ----------------------------------------------------
    pub prod_rules: production::ProdRules,
    pub production_runtime: production::runtime::LiveProductionRuntime,
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
    /// Authoritative generated-column/ObjectRegistry/Guy-stamp collision store.
    pub movement_collision: movement_live::LiveCollisionRuntime,

    // ---- step 15: projectiles and corpses ---------------------------------------------
    pub ammo: ammo::AmmoPool,
    pub shots: Vec<AmmoShot>,
    pub deaths: combat::DeathRing,
    /// Per-unit exact Guy sources. Rows without a source fail the crash arm closed.
    pub crash_units: Vec<Option<CrashUnitSource>>,
    /// PDB type facts used by [`ammo::plan_ammo_crash`].
    pub crash_type_rules: Vec<ammo::CrashTypeRule>,
    /// Exact TerrainOut provider. The normal flat-terrain ammo compatibility view is not exact
    /// enough for a checksum-visible crash constructor, so absence suppresses the transaction.
    pub crash_env: Option<Box<dyn ammo::CrashEnv + Send>>,

    pub cover: Coverage,
    /// Reused every tick: allocating the traversal order was measurably the largest cost
    /// in the object pass.
    traversal_buf: Vec<TraversalEntry<WorldObjectIdentity>>,
}

/// Production step-14 host for the portion of `Unit::do_spec_anim` whose complete mutation
/// surface is already owned by [`Sim`].
///
/// Object-free EXIT touches only the canonical walked order, queue, Unit columns, and path stack.
/// Target-backed EXIT first resolves canonical object identity/current type, but its non-strict
/// `ObjectData::is(AIRBASE, 0)` relation is not owned by `Sim`; ENTER and every target-backed EXIT
/// therefore remain typed-unavailable before mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SimSpecialAnimBefore {
    who: u8,
    o: i16,
    uid: u16,
    x: i32,
    y: i32,
    angle: i32,
    unit_masks: u32,
    dest_angle: i32,
    orders_x: i32,
    orders_y: i32,
    orders: order_dispatch::OrderQueue,
    path: movement::PathStack,
}

/// Exact identity/current-type surface reached by target-backed EXIT.
///
/// The registry band/row proves the `(who, o)` lookup, `uid` binds object lifetime, and each
/// band's canonical current-type owner identifies the relation row. The separate non-strict
/// relation list remains external; a different `type_index` must never be treated as proof that
/// `is(AIRBASE, 0)` is false.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SimSpecialAnimTargetBefore {
    identity: special_anim_executor::ObjectIdentity,
    band: Band,
    row: usize,
    type_index: i32,
}

impl SimSpecialAnimBefore {
    fn capture(actor: &order_dispatch::UnitWork) -> Self {
        Self {
            who: actor.who,
            o: actor.o,
            uid: actor.uid,
            x: actor.body.x,
            y: actor.body.y,
            angle: actor.body.angle,
            unit_masks: actor.unit_masks,
            dest_angle: actor.dest_angle,
            orders_x: actor.orders_x,
            orders_y: actor.orders_y,
            orders: actor.orders.clone(),
            path: actor.path.clone(),
        }
    }
}

#[derive(Clone)]
struct SimSpecialAnimHost<'a> {
    frame: i32,
    rng_state: i32,
    before: Option<SimSpecialAnimBefore>,
    target_before: Option<SimSpecialAnimTargetBefore>,
    world: &'a World,
    builds: &'a [production::BuildData],
    walls: &'a [walls::WallState],
    unit_types: &'a [i32],
    build_types: &'a [Option<i32>],
}

fn special_anim_debug_digest(value: &impl std::fmt::Debug) -> u64 {
    u64::from(adler32(1, format!("{value:?}").as_bytes()))
}

fn special_anim_snapshot(
    host: &SimSpecialAnimHost<'_>,
    actor: &order_dispatch::UnitWork,
    order: &order_dispatch::OrderRec,
) -> Result<special_anim_executor::SpecialAnimHostSnapshot, order_dispatch::SpecialAnimHostError> {
    let current =
        actor
            .orders
            .front()
            .ok_or(order_dispatch::SpecialAnimHostError::InvalidState(
                "SPECIAL_ANIM queue became empty before snapshot",
            ))?;
    if current != order {
        return Err(order_dispatch::SpecialAnimHostError::InvalidState(
            "SPECIAL_ANIM head changed before snapshot",
        ));
    }
    let state = current
        .special_anim
        .map(order_dispatch::special_anim_state)
        .ok_or(order_dispatch::SpecialAnimHostError::InvalidState(
            "missing walked SPECIAL_ANIM payload",
        ))?;
    let identity = special_anim_executor::ObjectIdentity {
        o: i32::from(actor.o),
        who: i32::from(actor.who),
        uid: actor.uid,
    };
    let target = host.resolve_exit_target(state)?;
    Ok(special_anim_executor::SpecialAnimHostSnapshot {
        actor: special_anim_executor::ObjectSnapshot {
            identity,
            version: special_anim_debug_digest(actor),
        },
        target: target.map(|target| special_anim_executor::ObjectSnapshot {
            identity: target.identity,
            version: special_anim_debug_digest(&target),
        }),
        current_order: state,
        current_order_digest: special_anim_debug_digest(&state),
        queue_digest: special_anim_debug_digest(&actor.orders),
        path_digest: u64::from(adler32(1, &actor.path.walk_bytes())),
        primary_guy_digest: special_anim_debug_digest(&actor.lead_guy),
        object_epoch: host.frame as u32 as u64,
        // No terrain/external byte is observed by the accepted EXIT branch. Zero is a
        // deliberate not-observed token, not a guessed epoch.
        terrain_epoch: 0,
        external_epoch: 0,
        rng_epoch: host.rng_state as u32 as u64,
    })
}

impl SimSpecialAnimHost<'_> {
    /// Resolve the exact object identity and current relation-row index reached before the
    /// external non-strict Airbase predicate. Missing facts remain typed-unavailable.
    fn resolve_exit_target(
        &self,
        state: special_anim_executor::SpecialAnimState,
    ) -> Result<Option<SimSpecialAnimTargetBefore>, order_dispatch::SpecialAnimHostError> {
        if state.special_type != special_anim_executor::SpecialAnimKind::Exit || state.ox < 0 {
            return Ok(None);
        }
        let owner = usize::try_from(state.whom)
            .ok()
            .filter(|&owner| owner < crate::objects::OWNER_SLOTS)
            .ok_or(order_dispatch::SpecialAnimHostError::Unavailable)?;
        let object_id = u32::try_from(state.ox)
            .map_err(|_| order_dispatch::SpecialAnimHostError::Unavailable)?;
        let band = if object_id < crate::objects::BUILD_BAND_BASE {
            Band::Unit
        } else if object_id < crate::objects::WALL_BAND_BASE {
            Band::Build
        } else {
            Band::Wall
        };
        let offset = object_id
            .checked_sub(band.base())
            .ok_or(order_dispatch::SpecialAnimHostError::Unavailable)?
            as usize;
        let row = self
            .world
            .objects
            .slot(owner)
            .band(band)
            .get(offset)
            .copied()
            .map(|row| row as usize)
            .ok_or(order_dispatch::SpecialAnimHostError::Unavailable)?;
        let (uid, type_index) = match band {
            Band::Unit => {
                if row >= self.world.live_count() as usize
                    || usize::from(self.world.units.get_who(row)) != owner
                    || i32::from(self.world.units.o()[row]) != state.ox
                {
                    return Err(order_dispatch::SpecialAnimHostError::InvalidState(
                        "SPECIAL_ANIM target registry disagrees with Unit columns",
                    ));
                }
                let type_index = self
                    .unit_types
                    .get(row)
                    .copied()
                    .ok_or(order_dispatch::SpecialAnimHostError::Unavailable)?;
                (self.world.units.get_uid(row), type_index)
            }
            Band::Build => {
                let build = self
                    .builds
                    .get(row)
                    .ok_or(order_dispatch::SpecialAnimHostError::Unavailable)?;
                if usize::from(build.who) != owner {
                    return Err(order_dispatch::SpecialAnimHostError::InvalidState(
                        "SPECIAL_ANIM target registry owner disagrees with BuildData",
                    ));
                }
                let type_index = self
                    .build_types
                    .get(row)
                    .copied()
                    .flatten()
                    .ok_or(order_dispatch::SpecialAnimHostError::Unavailable)?;
                (build.uid, type_index)
            }
            Band::Wall => {
                let wall = self
                    .walls
                    .get(row)
                    .ok_or(order_dispatch::SpecialAnimHostError::Unavailable)?;
                if usize::from(wall.who) != owner {
                    return Err(order_dispatch::SpecialAnimHostError::InvalidState(
                        "SPECIAL_ANIM target registry owner disagrees with WallData",
                    ));
                }
                let type_index = wall
                    .ptype
                    .ok_or(order_dispatch::SpecialAnimHostError::Unavailable)?;
                (wall.uid, type_index)
            }
        };
        Ok(Some(SimSpecialAnimTargetBefore {
            identity: special_anim_executor::ObjectIdentity {
                o: state.ox,
                who: state.whom,
                uid,
            },
            band,
            row,
            type_index,
        }))
    }
}

impl order_dispatch::SpecialAnimWorld for SimSpecialAnimHost<'_> {
    fn special_anim_preflight(
        &mut self,
        actor: &order_dispatch::UnitWork,
        order: &order_dispatch::OrderRec,
    ) -> Result<
        special_anim_executor::SpecialAnimExecutorReceipt,
        order_dispatch::SpecialAnimHostError,
    > {
        let state = order
            .special_anim
            .map(order_dispatch::special_anim_state)
            .ok_or(order_dispatch::SpecialAnimHostError::InvalidState(
                "missing walked SPECIAL_ANIM payload",
            ))?;
        if state.special_type != special_anim_executor::SpecialAnimKind::Exit {
            return Err(order_dispatch::SpecialAnimHostError::Unavailable);
        }
        let target = self.resolve_exit_target(state)?;
        if let Some(target) = target {
            // `ObjectData::is(type, 0)` delegates to the canonical non-strict TypeData
            // relation. Equality proves the Airbase true case, but inequality does not prove
            // false; a related/grafted row can still contain AIRBASE in its `is_list`.
            let _resolved_surface = (target.identity, target.band, target.row, target.type_index);
            return Err(order_dispatch::SpecialAnimHostError::Unavailable);
        }
        let snapshot = special_anim_snapshot(self, actor, order)?;
        let request = special_anim_executor::SpecialAnimExecutorRequest {
            order: state,
            actor: special_anim_executor::ActorFacts {
                identity: snapshot.actor.identity,
            },
            enter_target: None,
            exit_target: None,
            random_draws: None,
            helicopter_samples: None,
            terrain_z: None,
        };
        let receipt = special_anim_executor::preflight_special_anim_executor(snapshot, request)
            .map_err(|_| {
                order_dispatch::SpecialAnimHostError::InvalidState(
                    "locally owned EXIT preflight rejected",
                )
            })?;
        if receipt.plan.branch != special_anim_executor::SpecialAnimBranch::ExitWithoutAirbase
            || receipt.plan.steps.iter().any(|step| {
                !matches!(
                    step,
                    special_anim_executor::SpecialAnimHostStep::StoreFrames(_)
                        | special_anim_executor::SpecialAnimHostStep::StoreStarted(_)
                        | special_anim_executor::SpecialAnimHostStep::KillCurrentOrder(0)
                )
            })
        {
            return Err(order_dispatch::SpecialAnimHostError::InvalidState(
                "locally owned EXIT escaped its effect set",
            ));
        }
        self.before = Some(SimSpecialAnimBefore::capture(actor));
        self.target_before = None;
        Ok(receipt)
    }

    fn special_anim_commit(
        &mut self,
        actor: &mut order_dispatch::UnitWork,
        order: &order_dispatch::OrderRec,
        preflight: &special_anim_executor::SpecialAnimExecutorReceipt,
    ) -> Result<order_dispatch::SpecialAnimCommitReceipt, order_dispatch::SpecialAnimHostError>
    {
        if self.before.as_ref() != Some(&SimSpecialAnimBefore::capture(actor)) {
            return Err(order_dispatch::SpecialAnimHostError::InvalidState(
                "SPECIAL_ANIM local owner changed before commit",
            ));
        }
        let state = order
            .special_anim
            .map(order_dispatch::special_anim_state)
            .ok_or(order_dispatch::SpecialAnimHostError::InvalidState(
                "missing walked SPECIAL_ANIM payload",
            ))?;
        if self.resolve_exit_target(state)? != self.target_before {
            return Err(order_dispatch::SpecialAnimHostError::InvalidState(
                "SPECIAL_ANIM target lookup or type changed before commit",
            ));
        }
        let current = special_anim_snapshot(self, actor, order)?;
        let plan = special_anim_executor::validate_special_anim_receipt(preflight, current)
            .map_err(|_| {
                order_dispatch::SpecialAnimHostError::InvalidState(
                    "SPECIAL_ANIM snapshot changed before commit",
                )
            })?;
        let mut after = actor.clone();
        for step in &plan.steps {
            match *step {
                special_anim_executor::SpecialAnimHostStep::StoreFrames(frames) => {
                    let payload = after
                        .orders
                        .front_mut()
                        .and_then(|current| current.special_anim.as_mut())
                        .ok_or(order_dispatch::SpecialAnimHostError::InvalidState(
                            "SPECIAL_ANIM payload disappeared before frames store",
                        ))?;
                    payload.frames = frames;
                }
                special_anim_executor::SpecialAnimHostStep::StoreStarted(started) => {
                    let payload = after
                        .orders
                        .front_mut()
                        .and_then(|current| current.special_anim.as_mut())
                        .ok_or(order_dispatch::SpecialAnimHostError::InvalidState(
                            "SPECIAL_ANIM payload disappeared before started store",
                        ))?;
                    payload.started = started;
                }
                special_anim_executor::SpecialAnimHostStep::KillCurrentOrder(0) => {
                    order_dispatch::kill_current_order(
                        &mut after,
                        order_dispatch::KillReason::Completed,
                    );
                }
                _ => {
                    return Err(order_dispatch::SpecialAnimHostError::InvalidState(
                        "external SPECIAL_ANIM effect reached local commit",
                    ));
                }
            }
        }
        *actor = after;
        Ok(order_dispatch::SpecialAnimCommitReceipt::applied(preflight))
    }
}

/// Borrow-split bridge from the exact step-12 scheduler into `Sim`'s authoritative stores.
///
/// `game_daemon_step12::process_all` temporarily owns the daemon record and 64-region slice;
/// every other child store remains reachable through this host. Keeping the bridge here also
/// lets the victory callback retain the existing synchronous terminal-cleanup transaction.
struct SimGameDaemonHost<'a> {
    sim: &'a mut Sim,
    expected_empty_colls: i32,
    work: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SimGameDaemonBridgeFault {
    CollisionCursor {
        daemon_empty_colls: i32,
        runtime_cursor: i32,
    },
    Visibility(step12_visibility_runtime::Step12VisibilityPreflightError),
}

impl game_daemon_step12::GameDaemonProcessAllHost for SimGameDaemonHost<'_> {
    type Fault = SimGameDaemonBridgeFault;

    fn preflight(&self, schedule: &game_daemon_step12::CallSchedule) -> Result<(), Self::Fault> {
        let runtime_cursor = self.sim.collision_blocks.cursor();
        if runtime_cursor != self.expected_empty_colls {
            return Err(SimGameDaemonBridgeFault::CollisionCursor {
                daemon_empty_colls: self.expected_empty_colls,
                runtime_cursor,
            });
        }
        if schedule.contains(game_daemon_step12::GameDaemonCall::UpdateAllSeen) {
            let leader_active = std::array::from_fn(|who| self.sim.leaders[who].active);
            self.sim
                .step12_visibility
                .preflight_full_producer(
                    &self.sim.world,
                    &self.sim.unit_type,
                    leader_active,
                    self.sim.map.fog.option.0,
                    step12_visibility_producer_frontier::Step12VisibilityTrigger::ScheduledStep12,
                )
                .map_err(SimGameDaemonBridgeFault::Visibility)?;
        }
        Ok(())
    }

    fn process_victory(&mut self) {
        let sim = &mut *self.sim;

        // `GameDaemon::process_victory` queries `LeaderData::has_preq(0x2B9)` live for
        // each qualifying Wonder alliance.
        for who in 0..NUM_LEADERS {
            sim.vic_leaders.slots[who].has_preq_2b9 =
                sim.production_runtime.leader_has_prerequisites(who, 0x2b9);
        }

        let wonder_mode = matches!(
            victory_score::Victory::from_u8(sim.vic_match.options.victory),
            Some(
                victory_score::Victory::Standard
                    | victory_score::Victory::SuddenDeath
                    | victory_score::Victory::Wonder
            )
        );
        let zeros = [0i32; NUM_LEADERS];
        let inputs = if wonder_mode && sim.wonders.has_active() {
            match sim.wonder_world.as_deref_mut() {
                Some(world) => sim.wonders.victory_inputs(world, &sim.vic_leaders),
                None => Err(wonders::WonderError::MissingWorld),
            }
        } else {
            Ok((zeros, zeros))
        };
        match inputs {
            Ok((wonder_net, wonder_value)) => {
                sim.wonder_error = None;
                sim.vic_leaders
                    .process_victory(&mut sim.vic_match, &wonder_net, &wonder_value);
                sim.flush_terminal_queue_cleanup();
                self.work = self.work.saturating_add(1);
            }
            Err(error) => {
                sim.wonder_error = Some(error);
                sim.cover.gaps[Gap::WonderValueWorld.index()] += 1;
            }
        }
    }

    fn calc_danger(&mut self) {
        // The exact shell reaches this only at `frame % 200 == 0`. Retain the red child
        // honestly without shifting every ordinary frame's gap count.
        self.sim.cover.gaps[Gap::GameDaemonCalcDanger.index()] += 1;
    }

    fn update_all_seen(&mut self) {
        // A successful scheduled callback is currently possible only through the producer's
        // `fog_option == 3` early return. Every ordinary full refresh was rejected by preflight
        // before the GameDaemon shell or any channel-12 plane could mutate.
        debug_assert_eq!(self.sim.map.fog.option.0, 3);
    }

    fn calc_markets(&mut self) {
        let sim = &mut *self.sim;
        let before = sim.market.cycle;
        let frame = sim.world.frame;
        economy::calc_markets(
            &sim.econ_rules,
            &mut sim.market,
            &mut sim.world.random,
            frame,
        );
        if sim.market.cycle != before {
            sim.cover.market_cycles += 1;
            self.work = self.work.saturating_add(1);
        }
    }

    fn check_borders(&mut self, regions: &mut [borders_fog::RegionBorderState]) -> i32 {
        let sim = &mut *self.sim;
        let mut inputs = [borders_fog::LeaderBorderInput::default(); NUM_LEADERS];
        for (i, leader) in sim.leaders.iter().enumerate() {
            inputs[i] = leader.border;
        }
        let tiles = borders_fog::check_borders(
            regions,
            &mut sim.map.world,
            &sim.map.border_sources,
            &inputs,
            &sim.map.territory,
        );
        sim.cover.border_tiles += tiles.max(0) as u64;
        self.work = self.work.saturating_add(tiles.max(0) as u32);
        tiles
    }

    fn process_coll_blocks(&mut self, empty_colls: &mut i32) {
        let sim = &mut *self.sim;
        let pass = sim.collision_blocks.process_step12(&mut sim.map.world);
        // Preflight proved the two views began equal. Commit the exclusive runtime's result
        // back to the PDB-shaped daemon field as part of the same scheduler callback.
        *empty_colls = pass.end_cursor;
        self.work = self.work.saturating_add(1);
    }

    fn groups_process(&mut self) {
        let sim = &mut *self.sim;
        let active = std::array::from_fn(|who| sim.leaders[who].active);
        let any_active = active.iter().any(|&value| value);
        let keep = |_who: usize, _o: i16| groups_guys::MemberState::Keep;
        let speed = |_who: usize, _group: &groups_guys::GroupData| None;
        // Retail advances `proc_group` even with no active leader.
        sim.groups.process(&active, &keep, &speed);
        if any_active {
            sim.cover.group_normalises += 1;
        }
        self.work = self.work.saturating_add(1);
    }
}

impl Sim {
    /// A sim over a `wcells` x `wcells` WCoord map (4 tiles per cell, 192 fine units per
    /// tile — so 64 gives the 256-tile square [`MAP_SPAN`] describes).
    pub fn new(seed: u64, wcells: u16) -> Sim {
        let mut map = MapState::new(wcells);
        map.single_region();
        // Retail's step-12 rollover walks 64 Region records unconditionally. Preserve the
        // reduced driver's populated region in slot zero and materialize the empty suffix.
        map.regions
            .resize_with(game_daemon_step12::REGION_SLOTS, Default::default);
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
            step8: leaders::Leaders::new(),
            step8_env: leaders::Step8Env::default(),
            step8_rules: leaders::Step8Rules::shipped(),
            last_event_frame_trace: leaders::EventFrameTrace::default(),
            leaders: Default::default(),
            market: economy::MarketState::default(),
            scenario_data: crate::script_runtime::ScenarioDataState::default(),
            vic_match,
            vic_leaders: victory_score::Leaders::new(types),
            cities: tech_cities::CityPool::new(),
            players: None,
            wonders: wonders::Wonders::new(),
            wonder_world: None,
            wonder_error: None,
            defeat_cleanup_error: None,
            cannon_time: CannonTimeState::default(),
            map,
            game_daemon: game_daemon_step12::GameDaemonState::default(),
            step12_visibility: step12_visibility_runtime::Step12VisibilityAuthority::default(),
            step12_visibility_error: None,
            collision_blocks: collision_blocks_live::CollisionBlockRuntime::new(),
            road_scan: crate::systems::roads::RoadScanState::default(),
            groups: crate::systems::canonical_group_move_host::retail_fresh_groups(),
            command_package_state:
                crate::systems::canonical_group_move_host::CommandPackageState::default(),
            group_move_authority:
                crate::systems::canonical_group_move_host::GroupMoveAuthority::default(),
            economy_group_authority:
                crate::systems::canonical_economy_group_host::EconomyRuntimeAuthority::default(),
            trade_route_authority:
                crate::systems::canonical_trade_route_runtime::TradeRouteRuntimeAuthority::default(),
            last_trade_route_receipt: None,
            last_trade_route_error: None,
            strafe_runtime_authority: canonical_strafe_runtime::StrafeRuntimeAuthority::default(),
            last_strafe_receipt: None,
            last_strafe_error: None,
            armies: crate::systems::armies::Armies::new(),
            army_leader_flags2: [0; NUM_LEADERS],
            prod_rules: production::ProdRules::shipped(),
            production_runtime: production::runtime::LiveProductionRuntime::default(),
            combat_rules: combat::CombatConstants::shipped(),
            builds: Vec::new(),
            walls: Vec::new(),
            herds: Vec::new(),
            unit_type: Vec::new(),
            shooter_rules: Vec::new(),
            paths: Vec::new(),
            path_unit: Vec::new(),
            pathfinder: movement::PathFinder::new(),
            movement_collision: movement_live::LiveCollisionRuntime::new(),
            ammo: ammo::AmmoPool::new(),
            shots: vec![AmmoShot::default(); ammo::AMMO_POOL_SLOTS],
            deaths: combat::DeathRing::with_capacity(64),
            crash_units: Vec::new(),
            crash_type_rules: Vec::new(),
            crash_env: None,
            cover: Coverage::default(),
            traversal_buf: Vec::new(),
        }
    }

    /// Install the exact Handle-bound formation/type projection consumed by the canonical
    /// Group→Move package host. Loaded simulations intentionally start without this external
    /// projection and fail closed until their content owner installs it again.
    pub fn replace_group_move_authority(
        &mut self,
        authority: crate::systems::canonical_group_move_host::GroupMoveAuthority,
    ) {
        self.group_move_authority = authority;
    }

    /// Install the exact object/fact projection consumed by Board/Repair/Trade packages.
    /// Loaded simulations deliberately start without it and fail closed until content installs
    /// the same revision/digest-bound projection again.
    pub fn replace_economy_group_authority(
        &mut self,
        authority: crate::systems::canonical_economy_group_host::EconomyRuntimeAuthority,
    ) {
        self.economy_group_authority = authority;
    }

    /// Install the fact snapshot used by exact, fail-closed `TRADE_ROUTE` activations.
    pub fn replace_trade_route_authority(
        &mut self,
        authority: crate::systems::canonical_trade_route_runtime::TradeRouteRuntimeAuthority,
    ) {
        self.trade_route_authority = authority;
    }

    /// Install the revision-bound type/search projection used by canonical STRAFE activations.
    pub fn replace_strafe_runtime_authority(
        &mut self,
        authority: canonical_strafe_runtime::StrafeRuntimeAuthority,
    ) {
        self.strafe_runtime_authority = authority;
    }

    /// Process the bounded canonical command cohort: exactly opcode 0 Group followed by opcode 7
    /// MoveTo. The package is planned from all owners, revalidated, and assignment-committed as one
    /// transaction. `lockstep_serial` is retained in the receipt; Group stamps use Game frame.
    pub fn process_command_package(
        &mut self,
        play: usize,
        lockstep_serial: i32,
        bytes: &[u8],
    ) -> Result<
        crate::systems::canonical_group_move_host::GroupMovePackageReceipt,
        crate::systems::canonical_group_move_host::PackageError,
    > {
        use crate::systems::canonical_group_move_host::{
            commit_group_move_package, prepare_group_move_package, NETWORK_PLAYERS,
        };

        let player_who: [Option<u8>; NETWORK_PLAYERS] = std::array::from_fn(|slot| {
            self.players.as_ref().and_then(|players| {
                let row = players.players[slot];
                (usize::from(row.play) == slot
                    && row.flags & crate::systems::player_lifecycle_tails::PLAYER_PRESENT != 0
                    && usize::from(row.who) < NUM_LEADERS)
                    .then_some(row.who)
            })
        });
        let prepared = prepare_group_move_package(
            &self.world,
            &self.groups,
            &self.paths,
            &self.command_package_state,
            &self.group_move_authority,
            &player_who,
            (self.map.world.tile_xs, self.map.world.tile_ys),
            self.world.frame,
            play,
            lockstep_serial,
            bytes,
        )?;
        commit_group_move_package(
            &mut self.world,
            &mut self.groups,
            &mut self.paths,
            &mut self.command_package_state,
            &self.group_move_authority,
            prepared,
        )
    }

    /// Process exactly one opcode-0 Group followed by the currently admitted simple action,
    /// UNITMASK (32). Selection/cache/allocation and every reached Unit/order/path mutation are
    /// prepared and revalidated against the canonical owners before one assignment-only commit.
    pub fn process_simple_group_package(
        &mut self,
        play: usize,
        lockstep_serial: i32,
        bytes: &[u8],
    ) -> Result<
        crate::systems::canonical_simple_group_host::SimpleGroupPackageReceipt,
        crate::systems::canonical_simple_group_host::SimpleGroupPackageError,
    > {
        use crate::systems::canonical_group_move_host::NETWORK_PLAYERS;
        use crate::systems::canonical_simple_group_host::{
            commit_simple_group_package, prepare_simple_group_package,
        };

        let player_who: [Option<u8>; NETWORK_PLAYERS] = std::array::from_fn(|slot| {
            self.players.as_ref().and_then(|players| {
                let row = players.players[slot];
                (usize::from(row.play) == slot
                    && row.flags & crate::systems::player_lifecycle_tails::PLAYER_PRESENT != 0
                    && usize::from(row.who) < NUM_LEADERS)
                    .then_some(row.who)
            })
        });
        let prepared = prepare_simple_group_package(
            &self.world,
            &self.groups,
            &self.paths,
            &self.command_package_state,
            &self.group_move_authority,
            &player_who,
            self.world.frame,
            play,
            lockstep_serial,
            bytes,
        )?;
        commit_simple_group_package(
            &mut self.world,
            &mut self.groups,
            &mut self.paths,
            &mut self.command_package_state,
            &self.group_move_authority,
            &player_who,
            prepared,
        )
    }

    /// Process exactly one opcode-0 Group followed by BoardShip (15), Repair (16), or Trade
    /// (17). Selection/cache/allocation and every reached order/path/Group mutation publish at
    /// one revalidated boundary; malformed or stale input consumes no RNG and writes nothing.
    pub fn process_economy_command_package(
        &mut self,
        play: usize,
        lockstep_serial: i32,
        bytes: &[u8],
    ) -> Result<
        crate::systems::canonical_economy_group_host::EconomyGroupPackageReceipt,
        crate::systems::canonical_economy_group_host::EconomyPackageError,
    > {
        use crate::systems::canonical_economy_group_host::{
            commit_economy_group_package, prepare_economy_group_package,
        };
        use crate::systems::canonical_group_move_host::NETWORK_PLAYERS;

        let player_who: [Option<u8>; NETWORK_PLAYERS] = std::array::from_fn(|slot| {
            self.players.as_ref().and_then(|players| {
                let row = players.players[slot];
                (usize::from(row.play) == slot
                    && row.flags & crate::systems::player_lifecycle_tails::PLAYER_PRESENT != 0
                    && usize::from(row.who) < NUM_LEADERS)
                    .then_some(row.who)
            })
        });
        let prepared = prepare_economy_group_package(
            &self.world,
            &self.groups,
            &self.paths,
            &self.command_package_state,
            &self.group_move_authority,
            &self.economy_group_authority,
            &player_who,
            self.world.frame,
            play,
            lockstep_serial,
            bytes,
        )?;
        commit_economy_group_package(
            &mut self.world,
            &mut self.groups,
            &mut self.paths,
            &mut self.command_package_state,
            &self.group_move_authority,
            &self.economy_group_authority,
            prepared,
        )
    }

    /// Install/replace the revision-bound type and Constants projection consumed by the
    /// step-12 Unit visibility authority.
    pub fn replace_step12_visibility_type_source(
        &mut self,
        type_revision: u64,
        composition_digest: u64,
        constants: step12_visibility_runtime::VisibilityConstants,
        types: Vec<step12_visibility_runtime::VisibilityTypeProjection>,
    ) -> Result<(), step12_visibility_runtime::VisibilityAuthorityInstallError> {
        self.step12_visibility.replace_type_source(
            type_revision,
            composition_digest,
            constants,
            types,
        )
    }

    /// Install one complete owner-local `num_units` table and ordered HeroesData projection.
    pub fn replace_step12_visibility_leader(
        &mut self,
        who: usize,
        leader: step12_visibility_runtime::VisibilityLeaderAuthority,
    ) -> Result<(), step12_visibility_runtime::VisibilityAuthorityInstallError> {
        self.step12_visibility.replace_leader(who, leader)
    }

    /// Apply `Object::init`'s detector-bit suffix and retain its mask provenance for a newly
    /// allocated Unit. The shared spawn anchor will call this directly once sparse allocation
    /// owns the whole creation transaction; callers can use it explicitly in the meantime.
    pub fn materialize_step12_object_init(
        &mut self,
        handle: Handle,
    ) -> Result<usize, step12_visibility_runtime::VisibilityAuthorityInstallError> {
        self.step12_visibility
            .materialize_object_init(&mut self.world, &self.unit_type, handle)
    }

    /// Remove count and instance provenance immediately before the corresponding World despawn.
    pub fn retire_step12_visibility_unit(
        &mut self,
        handle: Handle,
    ) -> Result<usize, step12_visibility_runtime::VisibilityAuthorityInstallError> {
        self.step12_visibility
            .retire_unit(&self.world, &self.unit_type, handle)
    }

    /// Bind a loaded or explicitly mutated full instance flags byte to the current authority
    /// revision without re-deriving its detector bit from the current type.
    pub fn record_step12_authoritative_instance(
        &mut self,
        handle: Handle,
    ) -> Result<usize, step12_visibility_runtime::VisibilityAuthorityInstallError> {
        self.step12_visibility
            .record_authoritative_instance(&self.world, handle)
    }

    /// Expose the exact non-mutating Unit preflight for diagnostics and product composition.
    pub fn prepare_step12_visibility_unit_pass(
        &self,
        trigger: step12_visibility_producer_frontier::Step12VisibilityTrigger,
    ) -> Result<
        step12_visibility_producer_frontier::LiveStep12Preparation,
        step12_visibility_runtime::VisibilityAuthorityFault,
    > {
        self.step12_visibility.prepare_unit_pass(
            &self.world,
            &self.unit_type,
            std::array::from_fn(|who| self.leaders[who].active),
            self.map.fog.option.0,
            trigger,
        )
    }

    /// Diagnostic digest of the joined authority. It is not mixed into retail World checksum
    /// channel 12; only committed fog/WData bytes belong to that channel.
    pub fn step12_visibility_authority_digest(&self) -> u64 {
        self.step12_visibility.digest()
    }

    /// Install the validated local post-load Unit type source used by step 8 stat refreshes.
    ///
    /// Redistributable builds intentionally start without this retail-derived source. Until the
    /// product host supplies one, automatic Unit type queries fail closed and preserve the prior
    /// walked speed/armor values.
    pub fn install_unit_type_stat_source(
        &mut self,
        source: leaders::UnitTypeStatSource,
    ) -> Result<(), UnitTypeStatSourceInstallError> {
        if let Some(installed) = self.step8_env.unit_type_stats.as_ref() {
            return Err(UnitTypeStatSourceInstallError::AlreadyInstalled {
                installed: installed.provenance(),
                offered: source.provenance(),
            });
        }
        if self.world.frame != 0 || self.cover.ticks != 0 {
            return Err(UnitTypeStatSourceInstallError::SimulationStarted {
                frame: self.world.frame,
                completed_ticks: self.cover.ticks,
            });
        }
        let registered_objects = self.world.objects.total_objects();
        if self.world.live_count() != 0
            || registered_objects != 0
            || !self.unit_type.is_empty()
            || !self.builds.is_empty()
            || !self.walls.is_empty()
            || !self.herds.is_empty()
        {
            return Err(UnitTypeStatSourceInstallError::SimulationPopulated {
                live_units: self.world.live_count(),
                registered_objects,
                unit_type_rows: self.unit_type.len(),
                builds: self.builds.len(),
                walls: self.walls.len(),
                herds: self.herds.len(),
            });
        }
        if self.leaders.iter().any(|leader| leader.active)
            || self
                .step8
                .leaders
                .iter()
                .any(|leader| leader.flags & (leaders::flag::IN_GAME | leaders::flag::PROCESS) != 0)
            || self.world.objects.active_slots().next().is_some()
        {
            return Err(UnitTypeStatSourceInstallError::LeadersActivated);
        }
        self.step8_env.unit_type_stats = Some(source);
        Ok(())
    }

    pub fn unit_type_stat_source_provenance(
        &self,
    ) -> Option<leaders::UnitTypeStatSourceProvenance> {
        self.step8_env
            .unit_type_stats
            .as_ref()
            .map(leaders::UnitTypeStatSource::provenance)
    }

    // -- population -------------------------------------------------------------------

    /// Activate a player slot everywhere the tick tests for it.
    pub fn activate(&mut self, who: usize) {
        self.leaders[who].active = true;
        self.step8.leaders[who].activate();
        self.leaders[who].border.active = true;
        self.vic_leaders.slots[who].leader_flags |=
            victory_score::leader_flag::VALID | victory_score::leader_flag::ACTIVE;
        assert!(self.world.set_object_owner_active(who, true));
        self.map.fog.leaders[who].player_mask = 1u8 << who;
    }

    /// Register the completed-Wonder slice reached at `Build::activate` `0x00625B5B`.
    /// The object/type/game store is mandatory because `Wonder::init` performs a real
    /// prerequisite-bit write; an absent store cannot create a local-only record.
    pub fn init_completed_wonder(&mut self, who: i32, o: i32) -> Result<i16, wonders::WonderError> {
        let world = self
            .wonder_world
            .as_deref_mut()
            .ok_or(wonders::WonderError::MissingWorld)?;
        self.wonders.init_wonder(world, who, o)
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
            self.crash_units.push(None);
        }
        self.unit_type[row] = type_id;
        self.movement_collision
            .ensure_rows(self.world.live_count() as usize);
        Some(h)
    }

    /// Attach the exact non-column movement/collision facts for one live unit. Installation is
    /// atomic: validation precedes WData linking and Guy footprint stamps.
    pub fn install_movement_collision_source(
        &mut self,
        h: Handle,
        source: movement_live::LiveCollisionSource,
    ) -> Result<usize, movement_live::LiveCollisionFault> {
        self.movement_collision
            .install(&mut self.world, &mut self.map.world, h, source)
    }

    /// Snapshot the identity- and revision-bound movement action state for one actor.
    pub fn movement_source_state(
        &self,
        actor: Handle,
    ) -> Result<movement_live::MovementSourceState, movement_live::LiveCollisionFault> {
        self.movement_collision.source_state(&self.world, actor)
    }

    /// Apply one revision-checked transition to the Sim-owned movement source.
    pub fn compare_exchange_movement_source_state(
        &mut self,
        actor: Handle,
        expected_revision: u64,
        moving: bool,
        action: OrderIndex,
    ) -> Result<movement_live::MovementSourceStateReceipt, movement_live::LiveCollisionFault> {
        self.movement_collision.compare_exchange_source_state(
            &self.world,
            actor,
            expected_revision,
            moving,
            action,
        )
    }

    /// Compatibility wrapper for callers which already own the surrounding transaction.
    ///
    /// Exact adapters should prefer the snapshot plus compare-exchange pair so a request
    /// prepared against an earlier source image cannot overwrite a newer transition.
    pub fn set_movement_source_state(
        &mut self,
        actor: Handle,
        moving: bool,
        action: OrderIndex,
    ) -> Result<usize, movement_live::LiveCollisionFault> {
        let expected_revision = self.movement_source_state(actor)?.revision;
        self.compare_exchange_movement_source_state(actor, expected_revision, moving, action)
            .map(|receipt| receipt.after.row)
    }

    /// Attach an exact PDB Guy array and owner `get_gpiece` result to a live unit row.
    pub fn install_crash_unit_source(&mut self, h: Handle, source: CrashUnitSource) -> bool {
        let Some(row) = self.world.row_of(h) else {
            return false;
        };
        while self.crash_units.len() <= row {
            self.crash_units.push(None);
        }
        self.crash_units[row] = Some(source);
        true
    }

    /// Insert or replace the static crash fields for one global PDB type index.
    pub fn install_crash_type_rule(&mut self, rule: ammo::CrashTypeRule) {
        if let Some(old) = self
            .crash_type_rules
            .iter_mut()
            .find(|r| r.type_index == rule.type_index)
        {
            *old = rule;
        } else {
            self.crash_type_rules.push(rule);
        }
    }

    /// Install the exact TerrainOut adapter required by crash endpoint construction.
    pub fn install_crash_env<E: ammo::CrashEnv + Send + 'static>(&mut self, env: E) {
        self.crash_env = Some(Box::new(env));
    }

    /// Register a building in owner `who`'s band 2000.
    pub fn spawn_build(&mut self, who: usize, mut bd: production::BuildData) -> usize {
        let row = self.builds.len();
        bd.who = who as u8;
        let o = self.world.objects.insert(who, Band::Build, row as u32);
        self.builds.push(bd);
        self.world
            .mirror_dense_non_unit_append(who, Band::Build, row as u32, o)
            .expect("build insertion committed a gap-free dense append");
        row
    }

    /// Register a wall segment in owner `who`'s band 3000.
    pub fn spawn_wall(&mut self, who: usize, mut st: walls::WallState) -> usize {
        let row = self.walls.len();
        st.who = who as u8;
        let o = self.world.objects.insert(who, Band::Wall, row as u32);
        st.o = (o - crate::objects::WALL_BAND_BASE) as i16;
        self.walls.push(st);
        self.world
            .mirror_dense_non_unit_append(who, Band::Wall, row as u32, o)
            .expect("wall insertion committed a gap-free dense append");
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
        // With no attached script producer step 4 is vacuous and cannot fail. Keep
        // the script-free Sim `Send` for the existing parallel batch path: don-bhs
        // values are intentionally thread-confined `Rc` graphs.
        self.do_frame_inner(None)
            .expect("a script-free tick has no BHS failure path")
    }

    /// The same tick with a persistent BHS runtime attached at retail step 4.
    /// Unsupported host semantics return before any later subsystem or frame++.
    pub fn do_frame_with_scripts(
        &mut self,
        scripts: &mut ScriptRuntime,
    ) -> Result<TickTrace, ScriptRunError> {
        self.do_frame_inner(Some(scripts))
    }

    fn do_frame_inner(
        &mut self,
        scripts: Option<&mut ScriptRuntime>,
    ) -> Result<TickTrace, ScriptRunError> {
        let mut t = TickTrace {
            frame: self.world.frame,
            ..Default::default()
        };

        // 0..3 — autosave, the desync log, the debug-lag draw, the speed command.
        for s in 0..4 {
            t.steps[s] = StepRun::OutOfScope;
        }
        // 4 — RunTimeEnv::run_script, twice: selected game script first on every
        // frame, then general powers only when the pre-increment frame is positive.
        // Any missing host semantic aborts here, before leaders, objects, or frame++.
        let (r, w) = match scripts {
            Some(runtime) => self.run_scripts(runtime)?,
            None => (StepRun::Vacuous, 0),
        };
        t.steps[4] = r;
        t.work[4] = w;
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
        let (r, w) = self.armies_process_all();
        t.steps[13] = r;
        t.work[13] = w;

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
        // 17 — Leaders::end_process_all.
        let (r, w) = self.leaders_end_process_all();
        t.steps[17] = r;
        t.work[17] = w;
        // 18, 19.
        t.steps[18] = StepRun::OutOfScope;
        let (r, w) = self.leaders_process_event_frames();
        t.steps[19] = r;
        t.work[19] = w;

        // 20 — Game::frame++. After the object pass, which is why the rotation used the
        // pre-increment value.
        self.world.frame = self.world.frame.wrapping_add(1);
        self.vic_match.frame = self.world.frame;
        t.steps[20] = StepRun::Executed;
        t.work[20] = 1;

        // 21 — OrdersMemManager::cycle: 28 recycler pools, no analogue here.
        t.steps[21] = StepRun::OutOfScope;
        // 22 — stray roads.
        let (r, w) = self.roads_scan_and_kill_stray();
        t.steps[22] = r;
        t.work[22] = w;

        // 23 — 15 sim frames is one game second, exactly.
        t.steps[23] = StepRun::Executed;
        if self.world.frame % FRAMES_PER_SECOND == 0 {
            self.world.seconds = self.world.seconds.wrapping_add(1);
            self.vic_match.tick = self.world.seconds;
            t.work[23] = 1;
        }

        // 24 — TurnControl::check_cannon_time. The frame read is the post-step-20 value.
        let cannon_active = self.cannon_time.active_player >= 0;
        let cannon_expired = self.cannon_time.check(self.world.frame);
        t.steps[24] = if cannon_active {
            StepRun::Executed
        } else {
            StepRun::Vacuous
        };
        t.work[24] = cannon_expired as u32;
        // 25..28.
        t.steps[25] = StepRun::OutOfScope;
        t.steps[26] = StepRun::OutOfScope;
        // 27 — `Game::do_frame` gates this call on semaphore bit 22. The headless
        // state transition consumes that one-shot latch; statistics/UI/menu work is
        // deliberately outside the simulation core.
        let end_game_pending = self
            .vic_match
            .sem(victory_score::game_sem::VICTORY_RESOLVED);
        let end_game_processed = self.vic_match.process_end_game();
        t.steps[27] = if end_game_pending {
            StepRun::Executed
        } else {
            StepRun::Vacuous
        };
        t.work[27] = end_game_processed as u32;
        t.steps[28] = StepRun::OutOfScope;

        self.cover.record(&t);
        Ok(t)
    }

    /// `n` ticks, returning the last trace.
    pub fn run(&mut self, n: usize) -> TickTrace {
        let mut last = TickTrace::default();
        for _ in 0..n {
            last = self.do_frame();
        }
        last
    }

    /// Run `n` script-bearing ticks, stopping on the first unsupported builtin or
    /// retail runtime error.
    pub fn run_with_scripts(
        &mut self,
        scripts: &mut ScriptRuntime,
        n: usize,
    ) -> Result<TickTrace, ScriptRunError> {
        let mut last = TickTrace::default();
        for _ in 0..n {
            last = self.do_frame_with_scripts(scripts)?;
        }
        Ok(last)
    }

    // -- step 4 -----------------------------------------------------------------------

    fn run_scripts(
        &mut self,
        runtime: &mut ScriptRuntime,
    ) -> Result<(StepRun, u32), ScriptRunError> {
        let run = runtime.run_frame(self)?;
        if run.calls == 0 {
            Ok((StepRun::Vacuous, 0))
        } else {
            // Each VM call is capped at 50,000,000 instructions, so the two measured
            // call sites cannot overflow the trace's u32 work counter.
            Ok((StepRun::Executed, run.bytecodes as u32))
        }
    }

    // -- step 8 -----------------------------------------------------------------------

    /// Synchronize the shared tick state into step 8's instruction-derived layout.
    ///
    /// This is an adapter, not a second implementation. The exact dispatcher owns flags,
    /// diplomacy, timers, population cap, rare-mask edge state and stat-pass history.
    /// `LeaderSlot` remains
    /// authoritative only for the economy fields that steps 11/12 still consume. The
    /// object bands are rebuilt from `ObjectRegistry` so the stat passes visit the same
    /// owner-local rows as the real object tick; unresolved virtual answers and their call
    /// counters are preserved in place.
    fn sync_step8_inputs(&mut self) {
        for who in 0..NUM_LEADERS {
            let src = &self.leaders[who];
            let dst = &mut self.step8.leaders[who];
            let policy = &self.vic_leaders.slots[who];
            dst.econ = src.econ;
            dst.last_calc_frame = src.last_calc_frame;
            dst.econ_dirty = src.dirty;
            dst.attrition_off = policy.give_attrition_disabled;
            dst.anti_attrition_off = policy.take_attrition_disabled;
            dst.neutral_attrition = policy.neutral_attrition;
            dst.building_attrition_off = policy.building_attrition_disabled;
            dst.pop_cap = policy.population_cap;

            let env = &mut self.step8_env.leaders[who];
            env.gather = src.gather_inputs.clone();
            env.caps = src.cap_gates;
            env.payout = src.gather_ctx;

            // `GatherInputs::rares` mirrors the payload at Leader+0x6DCC. The effective
            // mask and the other union operand stay persistent on the exact Leader.
            let mut rare_b = leaders::RareMask::empty();
            for (bit, present) in src.gather_inputs.rares.iter().copied().enumerate() {
                rare_b.set(bit, present);
            }
            dst.rare_b = rare_b;
        }

        for who in 0..NUM_LEADERS {
            let slot = self.world.objects.slot(who);
            let objects = &mut self.step8_env.leaders[who].objects;

            let unit_rows = slot.band(Band::Unit);
            objects
                .units
                .resize(unit_rows.len(), leaders::StatObject::default());
            for (view, &row) in objects.units.iter_mut().zip(unit_rows) {
                let row = row as usize;
                view.active = self.world.units.get_flags(row) & OBJ_FLAG_ACTIVE != 0;
                view.captain = self.world.units.o_up()[row] < 0;
                view.unit_query_source = Some(leaders::UnitQuerySource {
                    type_id: self.unit_type[row],
                    unit_masks2: self.world.units.unit_masks2()[row] as u32,
                });
                view.owner_in_game = self.step8.leaders[who].flags & leaders::flag::IN_GAME != 0;
                view.myhits = self.world.units.myhits()[row];
                view.mylos = self.world.units.mylos()[row];
                let down = self.world.units.o_down()[row];
                view.o_down = (down >= 0).then_some(down as usize);
                view.myspeed = self.world.units.myspeed()[row];
                view.speed_written = false;
                view.myarmor = self.world.units.myarmor()[row];
                view.armor_written = false;
            }

            let build_rows = slot.band(Band::Build);
            objects
                .band_2000
                .resize(build_rows.len(), leaders::StatObject::default());
            for (view, &row) in objects.band_2000.iter_mut().zip(build_rows) {
                if let Some(build) = self.builds.get(row as usize) {
                    view.active = build.is_valid();
                    view.wall_active = build.is_active();
                    view.wall_started = build.flags & production::flag::STARTED != 0;
                    view.wall_city_flag = build.flags & production::flag::CAPTURED != 0;
                    view.owner_in_game =
                        self.step8.leaders[who].flags & leaders::flag::IN_GAME != 0;
                    view.myhits = build.myhits;
                    view.mylos = build.other[0x3c] as i8;
                    view.job_counter = build.job_counter;
                    view.constr_time = build.constr_time;
                    view.construct_hits = build.construct_hits;
                    view.damage = build.damage;
                    view.inside_down = i16::from_le_bytes([build.other[0x28], build.other[0x29]]);
                    view.wall_hits_written = false;
                    view.wall_los_written = false;
                    view.construct_time_written = false;
                    view.eject_contents_requested = false;
                } else {
                    view.active = false;
                }
            }

            let wall_rows = slot.band(Band::Wall);
            objects
                .band_3000
                .resize(wall_rows.len(), leaders::StatObject::default());
            for (view, &row) in objects.band_3000.iter_mut().zip(wall_rows) {
                if let Some(wall) = self.walls.get(row as usize) {
                    view.active = wall.is_alive();
                    view.wall_active = wall.is_active();
                    view.owner_in_game =
                        self.step8.leaders[who].flags & leaders::flag::IN_GAME != 0;
                    view.myhits = wall.myhits;
                    view.mylos = wall.mylos;
                    // The 3000 band reaches `Wall::update_construct_time` through the same
                    // direct call as the building band (`0x006CF908`), so its `+0x50` is
                    // live state here too.
                    view.constr_time = wall.constr_time;
                    view.construct_time_written = false;
                } else {
                    view.active = false;
                }
            }
        }
    }

    /// Commit the resolved Object and Wall stat virtuals back into walked object state.
    /// Building-band writes are marker-gated so missing query packages leave live state intact.
    fn sync_step8_stat_outputs(&mut self, trace: &leaders::Step8Trace) {
        for who in 0..NUM_LEADERS {
            let slot = self.world.objects.slot(who);
            let objects = &self.step8_env.leaders[who].objects;

            if trace.unit_stats_ran[who] {
                for (view, &row) in objects.units.iter().zip(slot.band(Band::Unit)) {
                    let row = row as usize;
                    if view.active && view.captain && view.hit_inputs.is_some() {
                        self.world.units.myhits_mut()[row] = view.myhits;
                    }
                    if view.active && view.type_los.is_some() {
                        self.world.units.mylos_mut()[row] = view.mylos;
                    }
                    if view.armor_written {
                        self.world.units.myarmor_mut()[row] = view.myarmor;
                    }
                    if view.speed_written {
                        self.world.units.myspeed_mut()[row] = view.myspeed;
                    }
                }
            }
            if trace.wall_stats_ran[who] {
                for (view, &row) in objects.band_2000.iter().zip(slot.band(Band::Build)) {
                    if let Some(build) = self.builds.get_mut(row as usize) {
                        if view.construct_time_written {
                            build.constr_time = view.constr_time;
                        }
                        if view.wall_hits_written {
                            build.myhits = view.myhits;
                            build.construct_hits = view.construct_hits;
                        }
                        if view.wall_los_written {
                            build.other[0x3c] = view.mylos as u8;
                        }
                    }
                }
                for (view, &row) in objects.band_3000.iter().zip(slot.band(Band::Wall)) {
                    if let Some(wall) = self.walls.get_mut(row as usize) {
                        if view.construct_time_written {
                            wall.constr_time = view.constr_time;
                        }
                        if view.active && view.hit_inputs.is_some() {
                            wall.myhits = view.myhits;
                        }
                        if view.active && view.type_los.is_some() {
                            wall.mylos = view.mylos;
                        }
                    }
                }
            }
        }
    }

    /// `Leaders::process_all` `0x006ED2A0`, the recovered 387-byte dispatcher: outer
    /// `flags & 2` gate, per-frame resets, hostile scan, gather, edge-triggered wall/unit
    /// stat traversals, elimination, grace timers, taunt-table dispatch, and tail-bit clear.
    /// The base Object virtual bodies, the Wall override pair, and
    /// `Wall::update_construct_time` execute when their type rows are present. Automatic
    /// Wall/base-Object query population and the `Leader::process_taunt` AI-chat body
    /// remain call-site-counted gaps.
    fn leaders_process_all(&mut self) -> (StepRun, u32) {
        let frame = self.world.frame;
        self.sync_step8_inputs();
        let trace = leaders::process_all(
            &mut self.step8,
            frame,
            &self.step8_rules,
            &self.econ_rules,
            &mut self.step8_env,
        );
        self.sync_step8_stat_outputs(&trace);

        // `process_all` records this boundary instead of duplicating the already-ported
        // elimination function. Invoke it here, at its exact place and in retail slot order.
        for &who in trace.elimination_calls.iter() {
            self.vic_leaders
                .process_elimination(&mut self.vic_match, who);
        }

        // Later tick steps still read `LeaderSlot`; return the economy transaction to that
        // shared façade before step 11 begins.
        for who in 0..NUM_LEADERS {
            let src = &self.step8.leaders[who];
            let dst = &mut self.leaders[who];
            dst.econ = src.econ;
            dst.last_calc_frame = src.last_calc_frame;
            dst.dirty = src.econ_dirty;
        }

        let n = trace.leaders_processed() as u32;
        self.cover.leader_gathers += n as u64;
        self.cover.leader_hostile_frames +=
            trace.hostile_seen.iter().filter(|seen| **seen).count() as u64;
        self.cover.leader_wall_stat_passes +=
            trace.wall_stats_ran.iter().filter(|ran| **ran).count() as u64;
        self.cover.leader_unit_stat_passes +=
            trace.unit_stats_ran.iter().filter(|ran| **ran).count() as u64;
        self.cover.leader_stat_objects_visited += trace
            .wall_pass
            .iter()
            .chain(trace.unit_pass.iter())
            .map(|pass| pass.visited as u64)
            .sum::<u64>();
        self.cover.leader_construct_time_updates += trace
            .wall_pass
            .iter()
            .map(|pass| pass.construct_time_resolved as u64)
            .sum::<u64>();
        self.cover.leader_timer_creeps += trace
            .timers_crept
            .iter()
            .map(|count| *count as u64)
            .sum::<u64>();
        self.cover.leader_taunt_dispatches += trace.taunts.len() as u64;

        // Charge only genuinely unresolved calls. The four slot addresses are resolved;
        // plain Object hit/LOS bodies and `Wall::update_construct_time` run when their
        // type rows are supplied, while automatic Wall/base-Object query population
        // remains explicit.
        self.cover.gaps[Gap::LeaderCalcWallStats.index()] += trace
            .wall_pass
            .iter()
            .map(|pass| pass.unresolved_calls as u64)
            .sum::<u64>();
        self.cover.gaps[Gap::LeaderCalcUnitStats.index()] += trace
            .unit_pass
            .iter()
            .map(|pass| pass.unresolved_calls as u64)
            .sum::<u64>();
        // Charge unresolved CALLS, not dispatches. `Leader::process_taunt` is now recovered
        // whole, so a build/rush/need dispatch executes end to end and owes nothing; only a
        // tribute dispatch still charges, and exactly once, for `Leader::action_respond`
        // `0x006D03C0`. Counting dispatches kept charging a body that runs.
        self.cover.gaps[Gap::LeaderProcessTaunt.index()] +=
            trace.taunt_pass.unresolved_calls as u64;

        if n == 0 {
            (StepRun::Vacuous, 0)
        } else {
            (StepRun::Executed, n)
        }
    }

    // -- step 11 ----------------------------------------------------------------------

    /// Apply `Armies::leader_defeated`/`Army::stop` and `Leader::defeat`'s Unit-band sweep
    /// after preflighting the complete owner transaction.
    pub(crate) fn clean_defeated_unit_band(
        &mut self,
        runtime: &production::runtime::LiveProductionRuntime,
        owner: usize,
    ) -> Result<defeat_cleanup::DefeatCleanupReceipt, defeat_cleanup::DefeatCleanupError> {
        use defeat_cleanup::{DefeatCleanupError as Error, DefeatedUnitAction as Action};

        // `Armies::leader_defeated` is first in retail. Resolve its complete Group/member
        // transaction without changing an Army, Group, Unit, order, or path byte.
        let army_targets = self.armies.leader_defeated_targets(owner);
        let mut army_plans = Vec::with_capacity(army_targets.groups.len());
        for target in army_targets.groups.iter().copied() {
            let group_id = target.group_id as usize;
            let Some(group) = self.groups.list.get(group_id).cloned() else {
                return Err(Error::MissingArmyGroup {
                    owner,
                    army_slot: target.army_slot,
                    group_id: target.group_id,
                });
            };
            // `Army::stop` skips an empty Group before `Group::action_begin`.
            if group.num == 0 {
                continue;
            }
            if group.who as usize != owner {
                return Err(Error::ArmyGroupOwnerMismatch {
                    owner,
                    army_slot: target.army_slot,
                    group_id: target.group_id,
                    group_owner: group.who,
                });
            }
            if group.buildings != 0 {
                let plan = groups_guys::plan_action_halt(&group, 0, &[]).map_err(|error| {
                    Error::ArmyGroupPlan {
                        owner,
                        army_slot: target.army_slot,
                        group_id: target.group_id,
                        error,
                    }
                })?;
                army_plans.push((group_id, plan));
                continue;
            }

            let n = group.num.clamp(0, groups_guys::GROUP_MAX_MEMBERS as i32) as usize;
            let mut members = Vec::with_capacity(n);
            for &member_o in &group.list[..n] {
                let mut facts = groups_guys::HaltMemberFacts {
                    o: member_o,
                    ..Default::default()
                };
                let Ok(object_id) = usize::try_from(member_o) else {
                    members.push(facts);
                    continue;
                };
                let Some(row) = self
                    .world
                    .objects
                    .slot(owner)
                    .band(Band::Unit)
                    .get(object_id)
                    .copied()
                    .map(|row| row as usize)
                else {
                    members.push(facts);
                    continue;
                };
                if self.world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
                    members.push(facts);
                    continue;
                }

                facts.valid_unit = true;
                facts.on_map = crate::systems::air::is_on_map(self.world.units.inside_up()[row]);
                if !facts.on_map {
                    members.push(facts);
                    continue;
                }
                let Some(&type_index) = self.unit_type.get(row) else {
                    return Err(Error::MissingUnitType { owner, object_id });
                };
                let Some(is_plane) = runtime.installed_unit_is_plane(type_index) else {
                    return Err(Error::UnsupportedUnitType {
                        owner,
                        object_id,
                        type_index,
                    });
                };
                facts.is_plane = is_plane;
                if is_plane {
                    // `installed_unit_is_plane` already excludes the UnitType 0x20
                    // helicopter exception, so a true result is precisely this stop arm.
                    facts.domain = 2;
                } else {
                    let Some(entering_or_exiting) = self
                        .world
                        .orders(row)
                        .current()
                        .map_or(Some(false), Order::is_entering_or_exiting)
                    else {
                        return Err(Error::MissingSpecialAnimPayload { owner, object_id });
                    };
                    facts.entering_or_exiting = entering_or_exiting;
                    if !entering_or_exiting && self.paths.get(row).is_none() {
                        return Err(Error::MissingPathState { owner, object_id });
                    }
                }
                members.push(facts);
            }
            let plan = groups_guys::plan_action_halt(&group, 0, &members).map_err(|error| {
                Error::ArmyGroupPlan {
                    owner,
                    army_slot: target.army_slot,
                    group_id: target.group_id,
                    error,
                }
            })?;
            army_plans.push((group_id, plan));
        }

        // Resolve every fallible Unit-band type/path fact before touching a live row.
        // Retail's type pointers are total; the Sim equivalent must fail the whole owner
        // transaction closed rather than stop half an Army and guess at an aircraft.
        let object_rows = self.world.objects.slot(owner).band(Band::Unit).to_vec();
        let mut plan = Vec::with_capacity(object_rows.len());
        let mut invalid_skipped = 0usize;
        for (object_id, row) in object_rows.iter().copied().enumerate() {
            let row = row as usize;
            if self.world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
                invalid_skipped += 1;
                continue;
            }
            let Some(&type_index) = self.unit_type.get(row) else {
                return Err(Error::MissingUnitType { owner, object_id });
            };
            let Some(is_plane) = runtime.installed_unit_is_plane(type_index) else {
                return Err(Error::UnsupportedUnitType {
                    owner,
                    object_id,
                    type_index,
                });
            };
            let action = defeat_cleanup::plan_defeated_unit(true, is_plane)
                .expect("active Unit always receives a defeat action");
            if action == Action::CloseOrders && self.paths.get(row).is_none() {
                return Err(Error::MissingPathState { owner, object_id });
            }
            plan.push((row, action));
        }

        let mut receipt = defeat_cleanup::DefeatCleanupReceipt {
            owner,
            armies_stopped: army_targets.valid_armies,
            groups_stopped: army_plans.len(),
            slots_visited: object_rows.len(),
            invalid_skipped,
            ..Default::default()
        };
        for (group_id, halt) in army_plans {
            self.groups.list[group_id] = halt.group;
            for step in halt.steps {
                let (who, object_id) = match step {
                    groups_guys::HaltStep::ClearUnitMask { who, o, .. }
                    | groups_guys::HaltStep::ClearPathAnchor { who, o }
                    | groups_guys::HaltStep::CloseOrders { who, o, .. }
                    | groups_guys::HaltStep::ClearPartialPath { who, o }
                    | groups_guys::HaltStep::UpdateAction { who, o } => (who as usize, o as usize),
                };
                let row = self.world.objects.slot(who).band(Band::Unit)[object_id] as usize;
                match step {
                    groups_guys::HaltStep::ClearUnitMask { mask, .. } => {
                        let masks = self.world.units.get_unit_masks(row) & !mask;
                        self.world.units.set_unit_masks(row, masks);
                    }
                    groups_guys::HaltStep::ClearPathAnchor { .. }
                    | groups_guys::HaltStep::ClearPartialPath { .. } => {
                        self.paths[row].clear();
                    }
                    groups_guys::HaltStep::CloseOrders { .. } => {
                        self.world.orders_mut(row).clear();
                    }
                    groups_guys::HaltStep::UpdateAction { .. } => {
                        let x = self.world.units.x_internal()[row];
                        let y = self.world.units.y_internal()[row];
                        let angle = self.world.units.angle()[row];
                        self.world.units.orders_x_mut()[row] = x;
                        self.world.units.orders_y_mut()[row] = y;
                        self.world.units.dest_angle_mut()[row] = angle;
                        receipt.army_members_halted += 1;
                    }
                }
            }
        }
        for (row, action) in plan {
            match action {
                Action::DiePlane => {
                    // The existing live death transaction supplies `Unit::die`'s active-bit,
                    // aircraft-wreck and DeathObj effects. Passing current hits lands exactly
                    // on zero, matching the unconditional terminal call.
                    let hits = self.world.units.myhits()[row];
                    self.apply_damage(row, hits.max(0));
                    receipt.planes_killed += 1;
                }
                Action::CloseOrders => {
                    // `Unit::clear_orders` `0x005E3860`: clear its own facing-move latch,
                    // close every order, drop the partial path, then recompute the empty
                    // action endpoint from the Unit's current position/facing.
                    self.world.orders_mut(row).clear();
                    self.paths[row].clear();
                    let masks = self.world.units.get_unit_masks(row) & !0x0400_0000;
                    self.world.units.set_unit_masks(row, masks);
                    let x = self.world.units.x_internal()[row];
                    let y = self.world.units.y_internal()[row];
                    let angle = self.world.units.angle()[row];
                    self.world.units.orders_x_mut()[row] = x;
                    self.world.units.orders_y_mut()[row] = y;
                    self.world.units.dest_angle_mut()[row] = angle;
                    receipt.orders_closed += 1;
                }
            }
            let masks = self.world.units.get_unit_masks(row) & !defeat_cleanup::DEFEAT_UNIT_MASK;
            self.world.units.set_unit_masks(row, masks);
            receipt.unit_masks_cleared += 1;
        }
        Ok(receipt)
    }

    /// Drain only the defeated-player Unit half of a terminal transition against an
    /// explicit installed type store. Step 14 temporarily lends production runtime state
    /// out of `Sim`, so its synchronous Tech Race resolution calls this same adapter with
    /// that live store rather than observing `Sim`'s temporary default placeholder.
    pub(crate) fn flush_defeat_unit_cleanup(
        &mut self,
        runtime: &production::runtime::LiveProductionRuntime,
    ) {
        let unit_owners = self.vic_leaders.take_defeat_unit_cleanup();
        let mut first_error = None;
        for owner in 0..NUM_LEADERS {
            let owner_bit = 1u8 << owner;
            if unit_owners & owner_bit == 0 {
                continue;
            }
            match self.clean_defeated_unit_band(runtime, owner) {
                Ok(_) => {}
                Err(error) => {
                    first_error.get_or_insert(error);
                    self.vic_leaders.defer_defeat_unit_cleanup(owner_bit);
                }
            }
        }
        self.defeat_cleanup_error = first_error;
    }

    /// Apply the concrete Build and defeated-Unit sweeps requested by terminal leader
    /// transitions. Requests accumulate because steps 11, 12, and the step-14 Tech Race
    /// callback can resolve leaders before the object stores are drained.
    fn flush_terminal_queue_cleanup(&mut self) {
        let owners = self.vic_leaders.take_terminal_queue_cleanup();
        for owner in 0..NUM_LEADERS {
            if owners & (1u8 << owner) != 0 {
                self.production_runtime
                    .clean_terminal_build_queues(&mut self.builds, owner);
            }
        }

        let runtime = std::mem::take(&mut self.production_runtime);
        self.flush_defeat_unit_cleanup(&runtime);
        self.production_runtime = runtime;
    }

    /// `Leaders::strategy_all` `0x006ED430` — `check_explore`, `plan_strategy`,
    /// `compute_score`, `diplomacy`, `Game::check_victory`. The exact dispatcher and
    /// exploration body run here; only the two large AI bodies remain call-counted gaps.
    fn leaders_strategy_all(&mut self) -> (StepRun, u32) {
        let has_explore_preq = std::array::from_fn(|who| {
            self.vic_leaders.slots[who]
                .has_tech
                .get(leaders::EXPLORE_ALL_PREREQ)
                .copied()
        });
        let trace = leaders::strategy_all(
            &mut self.step8,
            leaders::StrategyInputs {
                frame: self.world.frame,
                // `GameAccess::ai_speed` is global. Step 8's shared adapter mirrors it in
                // every gather context; slot zero is the canonical copy.
                ai_speed: self.step8_env.leaders[0].payout.ai_speed,
                world: leaders::ExploreWorld {
                    reg_xs: self.map.world.reg_xs,
                    reg_ys: self.map.world.reg_ys,
                    reg_size: self.map.world.reg_size,
                    fog_xs: self.map.world.fog_xs,
                    seen2: &self.map.world.seen2,
                },
                has_explore_preq,
                check_victory_mode: self
                    .vic_match
                    .sem(victory_score::game_sem::CHECK_VICTORY_MODE),
            },
        );

        for call in trace.calls.iter().copied() {
            match call {
                leaders::StrategyCall::CheckExplore { update, .. } => {
                    if update == leaders::ExploreUpdate::MissingFacts {
                        self.cover.gaps[Gap::LeaderCheckExplore.index()] += 1;
                    }
                }
                leaders::StrategyCall::PlanStrategy(_) => {
                    self.cover.gaps[Gap::LeaderPlanStrategy.index()] += 1;
                }
                leaders::StrategyCall::ComputeScore { slot, force } => {
                    self.vic_leaders.compute_score(&self.vic_match, slot, force);
                }
                leaders::StrategyCall::Diplomacy(_) => {
                    self.cover.gaps[Gap::LeaderDiplomacy.index()] += 1;
                }
                leaders::StrategyCall::CheckVictory => {
                    self.vic_leaders.check_victory(&mut self.vic_match);
                }
            }
        }
        self.flush_terminal_queue_cleanup();

        let work = trace.calls.len() as u32;
        if work == 0 {
            (StepRun::Vacuous, 0)
        } else {
            (StepRun::Executed, work)
        }
    }

    // -- step 17 ----------------------------------------------------------------------

    /// `Leaders::end_process_all` `0x006ED070` — warning-flag cleanup plus the
    /// rate-limited local population-cap feedback boundary.
    fn leaders_end_process_all(&mut self) -> (StepRun, u32) {
        self.step8.sync_end_players_from_leaders();
        let trace = leaders::end_process_all(&mut self.step8, self.world.frame);
        let work = trace.leaders_processed() as u32;
        if work == 0 {
            (StepRun::Vacuous, 0)
        } else {
            (StepRun::Executed, work)
        }
    }

    // -- step 19 ----------------------------------------------------------------------

    /// The exact IN_GAME-gated dispatcher and complete deterministic body of
    /// `Leader::process_event_frame` `0x006EC180`.
    fn leaders_process_event_frames(&mut self) -> (StepRun, u32) {
        let age_by_who = std::array::from_fn(|who| Some(self.step8.leaders[who].econ.age));
        let team_scores =
            std::array::from_fn(|who| Some(self.vic_leaders.get_team_score(&self.vic_match, who)));
        let trace = leaders::process_event_frames(
            &mut self.step8,
            leaders::EventFrameInputs {
                frame: self.world.frame,
                age_by_who,
                team_scores,
            },
        );
        let work = trace.leaders_dispatched() as u32;
        self.last_event_frame_trace = trace;
        if work == 0 {
            (StepRun::Vacuous, 0)
        } else {
            (StepRun::Executed, work)
        }
    }

    // -- step 22 ----------------------------------------------------------------------

    /// `Roads::scan_and_kill_stray_roads` `0x008956A0`, including both direct tile
    /// cleanup children. Renderer-owned candidate facts remain explicit fail-closed inputs.
    fn roads_scan_and_kill_stray(&mut self) -> (StepRun, u32) {
        let trace = crate::systems::roads::scan_and_kill_stray_roads(
            &mut self.road_scan,
            &mut self.map.world,
        );
        self.cover.gaps[Gap::RoadsScanStray.index()] += trace.missing_candidate_tiles as u64;
        if trace.tiles_scanned == 0 {
            (StepRun::Vacuous, 0)
        } else {
            (StepRun::Executed, trace.tiles_scanned)
        }
    }

    // -- step 12 ----------------------------------------------------------------------

    /// `GameDaemon::process_all` `0x00732700` — `process_victory`, `calc_danger`,
    /// `update_all_seen`, `calc_markets`, `check_borders`, `process_coll_blocks`,
    /// `Groups::process`. Six children have live bodies; scheduled `calc_danger` remains red.
    ///
    /// Order is retail's, and it matters: fog, markets and borders are all recomputed
    /// **before** any unit moves, and group normalisation is the tail of this pass rather
    /// than a pass of its own.
    fn game_daemon_process_all(&mut self) -> (StepRun, u32) {
        let frame = self.world.frame;
        let mut daemon = std::mem::take(&mut self.game_daemon);
        let mut regions = std::mem::take(&mut self.map.regions);
        let mut host = SimGameDaemonHost {
            expected_empty_colls: daemon.empty_colls,
            sim: self,
            work: 0,
        };
        let result = game_daemon_step12::process_all(&mut daemon, frame, &mut regions, &mut host);
        let work = host.work;
        drop(host);
        self.game_daemon = daemon;
        self.map.regions = regions;

        match result {
            Ok(_) => {
                if frame % 100 == 33 {
                    self.step12_visibility_error = None;
                }
                (StepRun::Executed, work)
            }
            Err(game_daemon_step12::ProcessAllError::Host(
                SimGameDaemonBridgeFault::Visibility(error),
            )) => {
                // Host preflight runs before the daemon countdown, victory callback, plane
                // clear, markets, regions, borders, collision cursor, or groups. Retain the
                // exact refusal while charging the visibility child rather than collision.
                self.step12_visibility_error = Some(error);
                self.cover.gaps[Gap::GameDaemonUpdateAllSeen.index()] += 1;
                (StepRun::Unimplemented(Gap::GameDaemonUpdateAllSeen), 0)
            }
            Err(_) => {
                // Structural/collision errors are also preflight failures, so the adapter has
                // committed no local or child mutation.
                (StepRun::Unimplemented(Gap::GameDaemonProcessCollBlocks), 0)
            }
        }
    }

    // -- step 13 ----------------------------------------------------------------------

    /// `Armies::process_all` `0x006F3B00`: exact owner/slot dispatcher over the retail
    /// preallocated store. Valid armies fail closed until their full object host is present.
    fn armies_process_all(&mut self) -> (StepRun, u32) {
        let trace =
            self.armies
                .process_step13_dispatch(crate::systems::armies::Step13DispatchInputs {
                    frame: self.world.frame,
                    world_width: self.map.world.tile_xs,
                    world_height: self.map.world.tile_ys,
                    leader_flags: std::array::from_fn(|who| self.step8.leaders[who].flags),
                    leader_flags2: self.army_leader_flags2,
                });
        self.cover.gaps[Gap::ArmiesProcessAll.index()] +=
            trace.missing_live_host_armies as u64 + trace.process.gaps.total();
        if trace.process.slots_examined == 0 {
            (StepRun::Vacuous, 0)
        } else {
            (StepRun::Executed, trace.process.slots_examined)
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
        self.world.object_bands().traversal_into(frame, &mut order);
        let mut work = 0u32;

        for entry in order.iter().copied() {
            match (entry.address.band, entry.lifecycle) {
                (
                    RetailBand::Unit,
                    SparseSlotLifecycle::Live(WorldObjectIdentity::Unit { id, generation }),
                ) => {
                    let Some(row) = self.world.row_of(Handle { id, generation }) else {
                        continue;
                    };
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
                (RetailBand::Unit, SparseSlotLifecycle::Tombstone(facts)) => {
                    if facts.flags & OBJ_FLAG_ACTIVE == 0 && facts.hold_frames != 0 {
                        self.world
                            .tick_unit_tombstone_hold(entry.address)
                            .expect("traversal entry remains the same Unit tombstone");
                        self.cover.hold_decrements += 1;
                    }
                }
                (
                    RetailBand::Build,
                    SparseSlotLifecycle::Live(WorldObjectIdentity::BuildRow(row)),
                ) => {
                    self.build_process(row as usize, frame);
                    work += 1;
                }
                (
                    RetailBand::Wall,
                    SparseSlotLifecycle::Live(WorldObjectIdentity::WallRow(row)),
                ) => {
                    self.wall_process(row as usize, frame);
                    work += 1;
                }
                (_, SparseSlotLifecycle::Reserved { .. }) => {
                    panic!("live Sim traversal observed an outstanding object reservation")
                }
                // Unit tombstones are the phase-2 lifecycle owner. Build/Wall tombstones and
                // wrong-band identities remain unadmitted and have no executable body here.
                _ => {}
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
    /// SPECIAL_ANIM now crosses the canonical order/path/Unit-column boundary into the
    /// recovered dispatcher. The other compact arms remain here until their complete
    /// `WorkWorld` surfaces can be installed without guessing terrain, collision, or RNG.
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
            // Arm 25, `Unit::do_spec_anim` `0x005E5880`, through its narrow atomic host.
            OrderIndex::SpecialAnim => self.do_special_anim(row),
            // Arm 15, `Unit::do_trade` `0x005ED270`. The adapter admits only fully owned
            // branches; every road/city/caravan/selection residual holds without mutation.
            OrderIndex::TradeRoute => self.do_trade_route(row),
            // Arm 16, `Unit::do_strafe` `0x005EAB00`, through the atomic canonical adapter.
            OrderIndex::Strafe => self.do_strafe(row),
            // Arm 5 falls to the default arm and does nothing. Faithfully empty.
            OrderIndex::Patrol => {}
            _ => {}
        }
    }

    fn do_trade_route(&mut self, row: usize) {
        use crate::systems::canonical_trade_route_runtime::{
            commit_trade_route_activation, prepare_trade_route_activation,
        };

        let prepared = match prepare_trade_route_activation(
            &self.world,
            &self.paths,
            &self.trade_route_authority,
            row,
        ) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.last_trade_route_receipt = None;
                self.last_trade_route_error = Some(error);
                self.cover.trade_route_refused += 1;
                return;
            }
        };
        match commit_trade_route_activation(
            &mut self.world,
            &self.paths,
            &self.trade_route_authority,
            prepared,
        ) {
            Ok(receipt) => {
                self.last_trade_route_receipt = Some(receipt);
                self.last_trade_route_error = None;
                self.cover.trade_route_committed += 1;
            }
            Err(error) => {
                self.last_trade_route_receipt = None;
                self.last_trade_route_error = Some(error);
                self.cover.trade_route_refused += 1;
            }
        }
    }

    fn do_strafe(&mut self, row: usize) {
        let prepared = match canonical_strafe_runtime::prepare_strafe_activation(
            &self.world,
            &self.paths,
            &self.unit_type,
            &self.strafe_runtime_authority,
            row,
            (self.map.world.xs * 4, self.map.world.ys * 4),
        ) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.last_strafe_receipt = None;
                self.last_strafe_error = Some(error);
                return;
            }
        };

        let mut ammo_jobs = Vec::new();
        for effect in prepared.effects() {
            let canonical_strafe_runtime::StrafeRuntimeEffect::FireAmmo(target) = *effect else {
                continue;
            };
            let Some(target_row) = self.world.unit_row_at(target.who, target.o) else {
                self.last_strafe_receipt = None;
                self.last_strafe_error =
                    Some(canonical_strafe_runtime::CanonicalStrafeRuntimeError::StaleTarget);
                return;
            };
            if self.world.units.get_uid(target_row) != target.uid
                || self.world.units.get_flags(target_row) & OBJ_FLAG_ACTIVE == 0
            {
                self.last_strafe_receipt = None;
                self.last_strafe_error =
                    Some(canonical_strafe_runtime::CanonicalStrafeRuntimeError::StaleTarget);
                return;
            }
            let actor_type = self.unit_type[row];
            let Some(rules) = self.shooter_rules_for(actor_type) else {
                self.last_strafe_receipt = None;
                self.last_strafe_error = Some(
                    canonical_strafe_runtime::CanonicalStrafeRuntimeError::UnsupportedCone(
                        "STRAFE ammo type has no shooter rules",
                    ),
                );
                return;
            };
            let damage = self
                .type_stats(actor_type)
                .map_or(0, |stats| stats.attack / 10);
            ammo_jobs.push((target_row, rules, damage));
        }

        let receipt = match canonical_strafe_runtime::commit_strafe_activation(
            &mut self.world,
            &mut self.paths,
            &self.unit_type,
            &mut self.strafe_runtime_authority,
            prepared,
        ) {
            Ok(receipt) => receipt,
            Err(error) => {
                self.last_strafe_receipt = None;
                self.last_strafe_error = Some(error);
                return;
            }
        };
        let mut ammo_job = ammo_jobs.into_iter();
        for effect in &receipt.effects {
            match *effect {
                canonical_strafe_runtime::StrafeRuntimeEffect::SetAnimation(animation) => {
                    self.world.units.set_idle(
                        row,
                        u8::from(animation == crate::systems::strafe_order_frontier::IDLE_ANIM),
                    );
                }
                canonical_strafe_runtime::StrafeRuntimeEffect::FireAmmo(_) => {
                    let (target_row, rules, damage) =
                        ammo_job.next().expect("STRAFE ammo jobs were preflighted");
                    self.fire_ammo(row, target_row, rules, damage);
                }
            }
        }
        if self.world.random.state() != receipt.expected_rng_state_after_effects {
            self.last_strafe_receipt = None;
            self.last_strafe_error =
                Some(canonical_strafe_runtime::CanonicalStrafeRuntimeError::StaleCanonicalState);
            return;
        }
        self.last_strafe_error = None;
        self.last_strafe_receipt = Some(receipt);
    }

    /// Build the SPECIAL_ANIM dispatch view from canonical row-owned state.
    fn special_anim_actor(
        &self,
        row: usize,
    ) -> Result<order_dispatch::UnitWork, order_dispatch::SpecialAnimHostError> {
        let units = &self.world.units;
        let mut actor = order_dispatch::UnitWork::at(
            units.get_who(row),
            units.o()[row],
            units.x_internal()[row],
            units.y_internal()[row],
        );
        actor.flags = units.get_flags(row);
        actor.visible = units.visible()[row] as u8;
        actor.uid = units.get_uid(row);
        actor.inside_down = units.inside_down()[row];
        actor.ptype = self.unit_type.get(row).copied().unwrap_or_default();
        actor.body.angle = units.angle()[row];
        actor.dest_angle = units.dest_angle()[row];
        actor.tolerance = units.tolerance()[row];
        actor.orders_x = units.orders_x()[row];
        actor.orders_y = units.orders_y()[row];
        actor.unit_masks = units.get_unit_masks(row);
        actor.unit_masks2 = units.get_unit_masks2(row);
        actor.form = units.form()[row];
        actor.group = units.group()[row];
        actor.inside_up = units.inside_up()[row];
        actor.collide_frame = units.collide_frame()[row];
        actor.spell_time = units.spell_time()[row];
        actor.myspeed = units.myspeed()[row];
        actor.recharging = units.get_recharging(row);
        actor.idle = units.get_idle(row);
        actor.safe = units.safe()[row];
        actor.path = self.paths.get(row).cloned().ok_or(
            order_dispatch::SpecialAnimHostError::InvalidState(
                "live SPECIAL_ANIM row has no canonical path owner",
            ),
        )?;
        actor.orders = order_dispatch::adopt(self.world.orders(row));
        Ok(actor)
    }

    /// Publish the checksum-owned part of a successful local SPECIAL_ANIM transaction.
    fn publish_special_anim_actor(&mut self, row: usize, actor: &order_dispatch::UnitWork) {
        order_dispatch::publish(&actor.orders, self.world.orders_mut(row));
        if let Some(path) = self.paths.get_mut(row) {
            *path = actor.path.clone();
        }
        self.world.units.set_unit_masks(row, actor.unit_masks);
        self.world.units.dest_angle_mut()[row] = actor.dest_angle;
        self.world.units.orders_x_mut()[row] = actor.orders_x;
        self.world.units.orders_y_mut()[row] = actor.orders_y;
    }

    fn do_special_anim(&mut self, row: usize) {
        let Ok(mut actor) = self.special_anim_actor(row) else {
            self.cover.special_anim_malformed += 1;
            return;
        };
        let mut host = SimSpecialAnimHost {
            frame: self.world.frame,
            rng_state: self.world.random.state(),
            before: None,
            target_before: None,
            world: &self.world,
            builds: &self.builds,
            walls: &self.walls,
            unit_types: &self.unit_type,
            build_types: &self.production_runtime.build_types,
        };
        let mut dispatch = order_dispatch::DispatchCoverage::default();
        let result =
            order_dispatch::do_special_anim_with_host(&mut actor, &mut host, &mut dispatch);
        match result {
            order_dispatch::ArmResult::Retired(order_dispatch::KillReason::Completed) => {
                self.publish_special_anim_actor(row, &actor);
                self.cover.special_anim_completed += 1;
            }
            order_dispatch::ArmResult::Working => {
                self.cover.special_anim_working += 1;
            }
            order_dispatch::ArmResult::HostUnavailable => {
                self.cover.special_anim_host_refused += 1;
            }
            order_dispatch::ArmResult::MalformedOrder => {
                self.cover.special_anim_malformed += 1;
            }
            _ => {
                self.cover.special_anim_malformed += 1;
            }
        }
    }

    /// `Unit::do_move` `0x005F7B30` -> `Unit::move_step` `0x005FAF30`.
    ///
    /// The integrator is the ported one in [`crate::systems::movement`], driven off the unit's
    /// own `Stack<PathData>` and the side-effecting collision transaction. Movement without a
    /// complete live collision source holds position and increments the named gap.
    fn do_move(&mut self, row: usize) {
        let Some(ord) = self.world.orders(row).current().cloned() else {
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
        let rows = self.world.live_count() as usize;
        self.movement_collision.snapshot_paths(&self.paths, rows);
        self.movement_collision.begin_frame(self.world.frame);
        if self
            .movement_collision
            .preflight(&self.world, &self.map.world, &self.paths)
            .and_then(|_| self.movement_collision.actor_ready(&self.world, row))
            .is_err()
        {
            self.cover.gaps[Gap::UnitDetectCollision.index()] += 1;
            return;
        }

        let invalid_tiles = self
            .movement_collision
            .source(row)
            .expect("preflight proved movement source")
            .invalid_tiles
            .clone();
        let actor = movement_live::actor_ref(&self.world, row);
        let mut session = movement_driver::CollisionSession::new(actor);
        let mut check = std::mem::take(&mut self.movement_collision.check);
        // LiveCollisionStore mutably borrows the generated World columns. Bridge the one-word
        // Random state through a local object so collision RNG remains the same sim stream.
        let mut rng = crate::rng::Random::new(self.world.random.state());
        let mut path_host = movement_live::InstalledPathHost::new(row, &invalid_tiles);
        let mut move_world = movement_live::InstalledMoveWorld {
            tiles_w: self.map.world.tile_xs,
            tiles_h: self.map.world.tile_ys,
            wcells_w: self.map.world.xs,
            invalid_tiles: &invalid_tiles,
        };
        let mut profile = movement::MoveTurnProfile {
            type_turn_speed: i32::MAX as u32,
            ..movement::MoveTurnProfile::default()
        };
        let mut driver_rejected = false;
        let terrain = &mut self.map.world;
        let runtime = &mut self.movement_collision;
        let world = &mut self.world;
        let path = &mut self.paths[row];
        let mut store = movement_live::LiveCollisionStore::new(
            world,
            &mut runtime.sources,
            &mut runtime.order_state,
            &runtime.path_top_flags,
            &self.step8,
            &self.vic_leaders,
            &mut runtime.repath_budget,
        );
        let mut commit = |terrain: &mut TerrainWorld,
                          store: &mut movement_live::LiveCollisionStore<'_>,
                          write: movement_driver::ActorCommit| {
            movement_live::commit_actor(terrain, store, write);
        };
        // `turn_rate` still lacks the live Guy/constant composition in this compact Sim. Keep
        // the prior full-turn input; collision, persistence, spatial links and stamps are real.
        let outcome = movement::move_step_profile_with_collision(
            &mut move_world,
            &mut body,
            path,
            target,
            speed,
            i32::MAX,
            &mut profile,
            |_move_world, event| {
                let result = session.handle(
                    terrain,
                    &mut check,
                    &mut store,
                    &mut rng,
                    &mut path_host,
                    &mut commit,
                    event,
                );
                driver_rejected |= matches!(
                    result.decision,
                    movement_driver::DriverDecision::Rejected(_)
                );
                result.reply
            },
        );
        let store_fault_before_body = store.take_fault();
        if store_fault_before_body.is_none() && !driver_rejected {
            movement_live::commit_body(terrain, &mut store, actor, &body);
        }
        let store_fault = store_fault_before_body.or_else(|| store.take_fault());
        let path_fault = path_host.take_fault();
        drop(store);
        world.random.reseed(rng.state());
        runtime.check = check;
        self.cover.unit_move_step += 1;
        if driver_rejected || store_fault.is_some() || path_fault.is_some() {
            self.cover.gaps[Gap::UnitDetectCollision.index()] += 1;
        }
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
        let Some(ord) = self.world.orders(row).current().cloned() else {
            return;
        };
        if ord.target_who < 0 || ord.target_o < 0 {
            self.world.orders_mut(row).kill_current();
            return;
        }
        // Authoritative ATTACK orders carry both retail's TargetOrder triple and the stable
        // port Handle which named that exact incarnation. Consume the complete identity before
        // recharge, balance, movement, ammo, damage, or RNG state can change. A legacy order
        // without a Handle keeps the compatibility path below, but it is not authoritative and
        // can never be admitted by don-env.
        let exact_target_row = match ord.exact_target_identity() {
            None => None,
            Some(target) => {
                let object_row = self
                    .world
                    .unit_row_at(i32::from(target.who), i32::from(target.o));
                let handle_row = self.world.row_of(target.handle);
                let exact = handle_row.filter(|&target_row| {
                    Some(target_row) == object_row
                        && target_row != row
                        && target_row < self.world.live_count() as usize
                        && self.world.units.get_flags(target_row) & OBJ_FLAG_ACTIVE != 0
                        && self.world.units.get_who(target_row) as i8 == target.who
                        && self.world.units.o()[target_row] == target.o
                        && self.world.units.get_uid(target_row) == target.uid
                });
                let Some(target_row) = exact else {
                    self.world.orders_mut(row).kill_current();
                    return;
                };
                Some(target_row)
            }
        };
        let recharging = self.world.units.get_recharging(row);
        if recharging > 0 {
            self.world.units.set_recharging(row, recharging - 1);
            return;
        }
        let Some(balance) = self.world.rules.balance.clone() else {
            return;
        };
        let trow = match exact_target_row {
            Some(target_row) => target_row,
            None => match self
                .world
                .unit_row_at(i32::from(ord.target_who), i32::from(ord.target_o))
            {
                Some(row) => row,
                None => {
                    self.world.orders_mut(row).kill_current();
                    return;
                }
            },
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

    /// `Objects::kill_guy`'s category-8 aircraft-wreck arm, adapted from the exact live facts
    /// this Sim has been given. Every fallible read and gate completes before ammo/RNG mutation.
    fn try_spawn_unit_crash(&mut self, row: usize) -> LiveCrashOutcome {
        let Some(source) = self.crash_units.get(row).and_then(Option::as_ref) else {
            self.cover.crash_missing_facts += 1;
            return LiveCrashOutcome::MissingFacts;
        };
        if self.world.units.guy_mark().get(row).copied() != Some(1) || source.guys.guy_mark != 1 {
            // apply_damage currently represents a whole-unit death. It can identify Guy 0
            // exactly only for the one-live-Guy shape; choosing a casualty in a larger squad
            // would be an invented kill_guy policy.
            self.cover.crash_missing_facts += 1;
            return LiveCrashOutcome::MissingFacts;
        }
        let dying_guy = source.guys.guys.first().and_then(Option::as_ref);
        let Some(&owner_type_index) = self.unit_type.get(row) else {
            self.cover.crash_missing_facts += 1;
            return LiveCrashOutcome::MissingFacts;
        };
        let owner = ammo::CrashOwnerView {
            who: self.world.units.get_who(row) as i32,
            o: self.world.units.o()[row] as i32,
            type_index: owner_type_index,
            x: self.world.units.x_internal()[row],
            y: self.world.units.y_internal()[row],
            gpiece: source.gpiece,
            order_type: self
                .world
                .orders(row)
                .current()
                .map_or(-1, |order| order.kind as i32),
            lead_guy: source.guys.guys.first().and_then(Option::as_ref),
        };
        let crash = match ammo::plan_ammo_crash(dying_guy, owner, &self.crash_type_rules) {
            Ok(Some(crash)) => crash,
            Ok(None) => {
                self.cover.crash_ineligible += 1;
                return LiveCrashOutcome::Ineligible;
            }
            Err(_) => {
                self.cover.crash_missing_facts += 1;
                return LiveCrashOutcome::MissingFacts;
            }
        };
        let Some(env) = self.crash_env.as_deref() else {
            self.cover.crash_missing_facts += 1;
            return LiveCrashOutcome::MissingFacts;
        };
        if env.crash_world_wcells() != (self.map.world.xs, self.map.world.ys) {
            self.cover.crash_missing_facts += 1;
            return LiveCrashOutcome::MissingFacts;
        }

        let mut rng = ammo::Rng(self.world.random.state() as u32);
        let slot = ammo::ammo_spawn_crash(&mut self.ammo, &crash, env, &mut rng);
        self.world.random.reseed(rng.0 as i32);
        if slot >= self.shots.len() {
            self.shots.resize(slot + 1, AmmoShot::default());
        }
        // AmmoShot is a port-only damage sidecar. A recycled ordinary projectile must not
        // lend its damage to a wreck, whose retail damage is recovered from object/type state.
        self.shots[slot] = AmmoShot::default();
        self.cover.crash_spawned += 1;
        LiveCrashOutcome::Spawned(slot)
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
        // Retail's `Objects::kill_guy` constructs the wreck while the Guy and owning Unit are
        // both still live. Keep this before the active-bit clear / corpse transaction.
        let _ = self.try_spawn_unit_crash(trow);
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
        let Some(ord) = self.world.orders(row).current().cloned() else {
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
        {
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
        }
        let mut runtime = std::mem::take(&mut self.production_runtime);
        let _production = production::runtime::process_sim_build_queue(self, &mut runtime, row);
        self.production_runtime = runtime;
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
                cruise_pose: ammo::CruiseLaunchPose {
                    lead_angle: self.world.units.angle()[row],
                    // This Sim currently materializes one synthetic lead Guy at the Unit
                    // position; its checksum-visible pitch begins at retail's zero default.
                    lead_pitch: 0.0,
                    speed: self.world.units.myspeed()[row] as i32,
                },
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

    /// `Objects::inc_time` `0x0065DB70` — the whole 360-byte shell, in retail's own order.
    ///
    /// Retail advances every object's animation clock here and flies every projectile. This
    /// driver executes the **shell**: one head call and six loops, at the exact call sites
    /// [measured, capstone over `0x0065DB70..0x0065DCD8`].
    ///
    /// | site | call | here |
    /// |---|---|---|
    /// | `0x0065DB82` | `Nuke::do_damage` `0x0092BC80` | [`Gap::NukeDoDamage`] |
    /// | `0x0065DBBF` | unit band `vt+0xA0` `Unit::inc_time` `0x00610B40` | gate exact; guy clocks [`Gap::UnitIncTime`] |
    /// | `0x0065DBC9` | unit band `vt+0x154` `Unit::execute_events` `0x0060EDC0` | branch exact; body [`Gap::UnitExecuteEvents`] |
    /// | `0x0065DBF1` | build band `vt+0xA0` `Wall::inc_time` `0x0063FB60` | [`Gap::WallIncTime`] |
    /// | `0x0065DCCD` | goods band `vt+0xA0` | **faithfully empty**, see below |
    /// | `0x0065DC6F` | `Ammo::inc_time` `0x0067D380` | ported, [`ammo`] |
    /// | `0x0065DC9D` | `DeathObj::inc_time` `0x008D5240` | [`Gap::DeathObjIncTime`] |
    /// | `0x0065DCB2` | `Farms::inc_time` `0x008D8600` | [`Gap::FarmsIncTime`] |
    /// | `0x0065DCBC` | `Doober::inc_time` `0x00846770` | presentation |
    /// | `0x0065DCC1` | `Surf::inc_time` `0x008A1A00` | presentation, `internal_random` |
    ///
    /// **The goods loop is the one child that is genuinely finished by being empty.** At
    /// `0x0065DC40` retail compares the object's vtable against `Good::vftable` `0x00B447E8`
    /// and *skips the indirect call entirely* when it matches; and `Good::inc_time`
    /// `0x0066D850` is a one-byte `ret` in any case, as are the folded `+0xA0` slots of
    /// `Object`, `SubObject` and `Item` at `0x0041C150`. Goods and items have no per-tick
    /// clock, so there is nothing here to charge.
    ///
    /// Ordering facts this body is obliged to honour, and now does:
    ///
    /// * the object bands run **before** the ammo pool, not after — an impact therefore
    ///   lands after every animation clock of the same tick;
    /// * the owner walk **does not rotate** (step 14 does) and **never visits the wall
    ///   band**, and it covers the building band for all ten owners rather than eight;
    /// * the death loop runs after all ammo, so a corpse filed by an impact this tick is
    ///   visited by the same step 15.
    fn objects_inc_time(&mut self) -> (StepRun, u32) {
        // 0x0065DB82 — Nuke::do_damage, unconditionally and ahead of every loop.
        self.cover.gaps[Gap::NukeDoDamage.index()] += 1;

        // 0x0065DBA0..0x0065DC17 — the ten leader records in address order.
        let mut work = self.inc_time_object_bands();

        // 0x0065DC30 — the goods band. Faithfully empty; nothing to run, nothing to charge.

        // 0x0065DC60 — the ammo pool.
        work = work.wrapping_add(self.inc_time_ammo());

        // 0x0065DC90 — the death ring, after every projectile.
        work = work.wrapping_add(self.inc_time_deaths());

        // 0x0065DCB2 / 0x0065DCBC / 0x0065DCC1 — Farms, Doober, Surf. Doober is an alpha
        // fade over terrain clutter and Surf draws only `internal_random`; neither is
        // simulation. Farms is, and is charged.
        self.cover.gaps[Gap::FarmsIncTime.index()] += 1;

        if work == 0 {
            (StepRun::Vacuous, 0)
        } else {
            (StepRun::Executed, work)
        }
    }

    /// The unit and building bands of `Objects::inc_time`, `0x0065DBA0..0x0065DC17`.
    ///
    /// Retail reads every slot below the mark and gates on the object's own `flags & 1`
    /// (`test byte [edi+8], 1` at `0x0065DBB5`; `test byte [ecx+8], 1` at `0x0065DBE9`), so
    /// this walks the same live registry step 14 walks and applies the same active test.
    /// Unlike step 14 there is no tombstone hold to tick here: `Objects::inc_time` has no
    /// analogue of the `hold_frames` arm.
    fn inc_time_object_bands(&mut self) -> u32 {
        let mut order = std::mem::take(&mut self.traversal_buf);
        unit_inctime::inc_time_band_traversal(self.world.object_bands(), &mut order);
        let mut work = 0u32;

        for entry in order.iter().copied() {
            match (entry.address.band, entry.lifecycle) {
                (
                    RetailBand::Unit,
                    SparseSlotLifecycle::Live(WorldObjectIdentity::Unit { id, generation }),
                ) => {
                    let Some(row) = self.world.row_of(Handle { id, generation }) else {
                        continue;
                    };
                    if self.world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
                        continue;
                    }
                    work += 1;
                    let inside_up = self.world.units.inside_up()[row];
                    let type_index = self.unit_type.get(row).copied().unwrap_or(0);

                    // vtable +0xA0 — Unit::inc_time 0x00610B40. Its 0x00610B43 gate is
                    // exact; the two guy loops it dispatches are not, so a unit the gate
                    // rejects is reproduced work and a unit it accepts is a charged gap.
                    if unit_inctime::unit_inc_time_animates(inside_up, type_index) {
                        self.cover.inc_time_units += 1;
                        self.cover.gaps[Gap::UnitIncTime.index()] += 1;
                    } else {
                        self.cover.inc_time_units_gated += 1;
                    }

                    // vtable +0x154 — Unit::execute_events 0x0060EDC0. The dispatcher's
                    // only branch is decidable from live Unit state: `is_valid_unit` is the
                    // `flags & 1` already tested by the band loop, `is_on_map` 0x0046CE30 is
                    // `(u16)inside_up >> 15`, and bit 0x10 of `unit_masks2` selects
                    // verify-load. The verify arm is graphics resource admission over the
                    // guy prefix, so only the execute arm is charged as simulation.
                    let events = unit_inctime::UnitEventView {
                        unit_is_valid: true,
                        unit_is_on_map: (inside_up as u16) >> 15 != 0,
                        unit_masks2: self.world.units.get_unit_masks2(row),
                    };
                    match events.path() {
                        unit_inctime::UnitEventPath::ExecuteGuyEvents => {
                            self.cover.gaps[Gap::UnitExecuteEvents.index()] += 1;
                        }
                        unit_inctime::UnitEventPath::VerifyGraphicLoads => {
                            self.cover.inc_time_verify_paths += 1;
                        }
                    }
                }
                (
                    RetailBand::Build,
                    SparseSlotLifecycle::Live(WorldObjectIdentity::BuildRow(_)),
                ) => {
                    // vtable +0xA0 — Wall::inc_time 0x0063FB60, 2,273 bytes, unported.
                    work += 1;
                    self.cover.inc_time_builds += 1;
                    self.cover.gaps[Gap::WallIncTime.index()] += 1;
                }
                // Tombstones carry `flags & 1 == 0`, so retail skips their virtual calls;
                // the wall band is never reached by this traversal at all.
                _ => {}
            }
        }

        self.traversal_buf = order;
        work
    }

    /// The death ring loop, `0x0065DC7D..0x0065DCAF`.
    ///
    /// `cmp dword ptr [ecx], 0; je` at `0x0065DC98` is the `DeathObjData::valid` test, and
    /// the loop strides `0xA4` through `Objects+0x14C` for `Objects+0x140` records — slot
    /// order from zero, including a corpse filed moments earlier by the ammo loop above.
    /// The 540-byte body is recovered in [`crate::systems::death_inctime`] but its live
    /// adapter is unadmitted, so each reached corpse is charged rather than advanced.
    fn inc_time_deaths(&mut self) -> u32 {
        let mut work = 0u32;
        for slot in 0..self.deaths.slots.len() {
            if self.deaths.slots[slot].valid == 0 {
                continue;
            }
            work += 1;
            self.cover.inc_time_deaths_visited += 1;
            self.cover.gaps[Gap::DeathObjIncTime.index()] += 1;
        }
        work
    }

    /// The ammo pool loop, `0x0065DC54..0x0065DC7B`: `Ammo::inc_time` `0x0067D380` is a
    /// direct, non-virtual call gated on `flags & 3`.
    fn inc_time_ammo(&mut self) -> u32 {
        if self.ammo.live() == 0 {
            return 0;
        }
        let mut work = 0u32;
        // `(slot, call)` in pool-slot order. `Object::do_damage` is not this module's, so
        // the calls are collected and applied below.
        let mut calls: Vec<(usize, ammo::DamageCall)> = Vec::new();
        {
            let view = AmmoView {
                map: &self.map,
                world: &self.world,
                unit_type: &self.unit_type,
                shooter_rules: &self.shooter_rules,
            };
            // `Ammo::do_damage`'s ground jitter draws the sim stream twice; bridge the
            // state in and back out so the pool shares one stream with everything else.
            let mut rng = ammo::Rng(self.world.random.state() as u32);
            for slot in 0..self.ammo.slots.len() {
                if !self.ammo.slots[slot].occupied() {
                    continue;
                }
                let advance_common = |a: &mut ammo::Ammo| {
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
                let mut step = advance_common(&mut self.ammo.slots[slot]);
                if step == ammo::Step::Flying && self.ammo.slots[slot].has_spline {
                    let w = self.ammo.slots[slot].w;
                    let target = ammo::AmmoEnv::object(&view, w.whom, w.ox);
                    match self.ammo.step_cruise_targeted_slot(slot, target.as_ref()) {
                        Ok(ammo::CruiseTargetStep::SnappedForImmediateLoop { .. }) => {
                            // Retail jumps back to the common increment at 0x0067D392 in
                            // this same call; reuse the ordinary arrival/overshoot path.
                            step = advance_common(&mut self.ammo.slots[slot]);
                        }
                        Ok(_) => {}
                        Err(_) => {
                            // A non-null retail pointer can never lack its object. Fail closed
                            // instead of advancing a fabricated/empty path.
                            self.ammo.close_slot(slot);
                            step = ammo::Step::Closed;
                        }
                    }
                }
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
                            self.ammo.recycle_if_closed(slot);
                        }
                        for c in imp.calls {
                            calls.push((slot, c));
                        }
                    }
                    ammo::Step::Closed => {
                        self.ammo.recycle_if_closed(slot);
                        self.cover.ammo_closed += 1;
                    }
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
            let Some(trow) = self.world.unit_row_at(c.victim_who, c.victim_o) else {
                continue;
            };
            if trow < self.world.live_count() as usize
                && self.world.units.get_flags(trow) & OBJ_FLAG_ACTIVE != 0
            {
                self.apply_damage(trow, dmg);
            }
        }
        work
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
        let spline_family = ammo::select_retail_spline_family(
            self.ammo.graphic_piece_uses_spline(ord.gpiece),
            shooter.rules.obj_masks,
        );
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
            if spline_family == ammo::RetailSplineFamily::Arc {
                ammo::MissRadius::Formula
            } else {
                ammo::MissRadius::Perfect
            },
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

        if spline_family == ammo::RetailSplineFamily::Nuke {
            struct LiveNukeEnv<'a> {
                terrain: Option<&'a (dyn ammo::CrashEnv + Send)>,
                world: &'a TerrainWorld,
            }
            impl ammo::NukeSplineEnv for LiveNukeEnv<'_> {
                fn nuke_terrain(&self, x: i32, y: i32) -> Option<ammo::NukeTerrainSample> {
                    let terrain = self.terrain?;
                    let tx = x / ammo::TILE;
                    let ty = y / ammo::TILE;
                    if !self.world.valid_t(tx, ty) {
                        return None;
                    }
                    Some(ammo::NukeTerrainSample {
                        z: terrain.crash_terrain_z(x, y),
                        flags: self.world.tmask(tx, ty),
                    })
                }
            }

            let w = self.ammo.slots[slot].w;
            let env = LiveNukeEnv {
                terrain: self.crash_env.as_deref(),
                world: &self.map.world,
            };
            if self
                .ammo
                .install_nuke_spline(
                    slot,
                    &env,
                    !shooter.rules.nuke_high_arc,
                    ammo::SplineVec3::new(w.sx as f32, w.sy as f32, w.sz as f32),
                    ammo::SplineVec3::new(w.ex as f32, w.ey as f32, w.ez as f32),
                )
                .is_err()
            {
                self.ammo.close_slot(slot);
                self.shots[slot] = AmmoShot::default();
            }
        } else if spline_family == ammo::RetailSplineFamily::Cruise {
            if self
                .ammo
                .install_cruise_launch(slot, shooter, dist, ord.cruise_pose)
                .is_err()
            {
                self.ammo.close_slot(slot);
                self.shots[slot] = AmmoShot::default();
            }
        }
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
        mix(self
            .ammo
            .checksum_complete_readonly()
            .expect("live spline slots remain aligned with the ammo pool"));
        for l in self.leaders.iter() {
            mix(l.econ.adler32());
        }
        let mut leader_bytes = Vec::new();
        self.vic_leaders.walk_bytes(&mut leader_bytes);
        mix(adler32(1, &leader_bytes));
        let city_leaders_active =
            std::array::from_fn(|who| self.vic_leaders.slots[who].leader_flags & 1 != 0);
        mix(tech_cities::check_cities(
            &self.cities,
            &city_leaders_active,
        ));
        mix(economy::market_adler32(&self.market));
        mix(self.map.world.checksum());
        for w in self.walls.iter() {
            mix(adler32(1, &w.walk_bytes()));
        }
        for b in self.builds.iter() {
            mix(adler32(1, &b.image()));
        }
        // `check_groups` `0x00937530`. Without this the digest is blind to a group desync,
        // so every determinism test built on it — including save/load resume — could pass
        // across a diverged group pool. Requested as HOOK NEEDED by the save_load lane,
        // which had to compare `check_groups` explicitly to get real coverage.
        mix({
            let mut cs = crate::systems::groups_guys::CheckSum::default();
            self.groups.check_groups(&mut cs);
            cs.value
        });
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
            ("aircraft wrecks spawned", c.crash_spawned),
            ("wreck gates rejected", c.crash_ineligible),
            ("wreck facts unavailable", c.crash_missing_facts),
            ("fog cells newly explored", c.fog_cells_revealed),
            ("territory tiles claimed", c.border_tiles),
            ("Groups::process passes", c.group_normalises),
            ("Leader::gather calls", c.leader_gathers),
            ("leader hostile frames", c.leader_hostile_frames),
            ("Leader wall-stat passes", c.leader_wall_stat_passes),
            ("Leader unit-stat passes", c.leader_unit_stat_passes),
            ("leader stat objects visited", c.leader_stat_objects_visited),
            ("leader grace-timer creeps", c.leader_timer_creeps),
            ("leader taunt dispatches", c.leader_taunt_dispatches),
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
    use std::sync::{
        atomic::{AtomicI32, Ordering},
        Arc,
    };

    struct TickWonderWorld {
        who: i32,
        o: i32,
        value: Arc<AtomicI32>,
        prerequisite_set: bool,
    }

    impl wonders::WonderWorld for TickWonderWorld {
        fn init_facts(
            &mut self,
            who: i32,
            o: i32,
        ) -> Result<wonders::ReadReceipt<wonders::WonderInitFacts>, wonders::WonderWorldError>
        {
            if who != self.who || o != self.o {
                return Err("unknown completed Wonder".into());
            }
            Ok(wonders::ReadReceipt {
                value: wonders::WonderInitFacts {
                    who,
                    o,
                    frame: 0,
                    type_index: wonders::WONDER_FIRST,
                    prerequisite_index: 0x220,
                    world_xs: 70,
                    standard_map_xs: 70,
                    wonder_timer: 4500,
                    wonder_age: 0,
                },
                rng_draws: 0,
                world_writes: 0,
            })
        }

        fn set_prerequisite_complete(
            &mut self,
            facts: wonders::WonderInitFacts,
        ) -> Result<wonders::WonderFlagReceipt, wonders::WonderWorldError> {
            self.prerequisite_set = true;
            Ok(wonders::WonderFlagReceipt {
                who: facts.who,
                o: facts.o,
                type_index: facts.type_index,
                prerequisite_index: facts.prerequisite_index,
                flag_is_set: self.prerequisite_set,
                rng_draws: 0,
                world_writes: 1,
            })
        }

        fn wonder_value(
            &mut self,
            who: i32,
            o: i32,
        ) -> Result<wonders::ReadReceipt<wonders::WonderValue>, wonders::WonderWorldError> {
            if who != self.who || o != self.o {
                return Err("stale completed Wonder identity".into());
            }
            Ok(wonders::ReadReceipt {
                value: wonders::WonderValue {
                    who,
                    o,
                    value: self.value.load(Ordering::SeqCst),
                },
                rng_draws: 0,
                world_writes: 0,
            })
        }
    }

    #[derive(Clone, Copy)]
    struct CrashTerrain {
        xs: i32,
        ys: i32,
        z: i32,
    }

    impl ammo::CrashEnv for CrashTerrain {
        fn crash_world_wcells(&self) -> (i32, i32) {
            (self.xs, self.ys)
        }

        fn crash_terrain_z(&self, _x: i32, _y: i32) -> i32 {
            self.z
        }
    }

    fn arm_exact_one_guy_crash(sim: &mut Sim, h: Handle) -> usize {
        let row = sim.world.row_of(h).expect("live test unit");
        let who = sim.world.units.get_who(row);
        let o = sim.world.units.o()[row];
        let guy = groups_guys::GuyData {
            ty: 88,
            x: 2_000,
            y: 5_000,
            z: 1_000,
            angle: 0,
            bank: 13.75,
            who: who as i8,
            o,
            ..Default::default()
        };
        let source = CrashUnitSource {
            guys: groups_guys::UnitGuys {
                guys: vec![Some(guy)],
                size: 1,
                increment: 0,
                flags: 0,
                guy_mark: 1,
            },
            gpiece: Some(41),
        };
        assert!(sim.install_crash_unit_source(h, source));
        sim.install_crash_type_rule(ammo::CrashTypeRule {
            type_index: 88,
            cat: ammo::CRASH_TYPE_CAT,
            moves: 100,
            obj_masks: 0,
        });
        sim.install_crash_type_rule(ammo::CrashTypeRule {
            type_index: 99,
            cat: 0,
            moves: 0,
            obj_masks: 0,
        });
        let mut order = Order::default();
        order.kind = OrderIndex::Strafe;
        assert!(sim.issue(h, order));
        row
    }

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

    fn movement_source(x: i32, y: i32) -> movement_live::LiveCollisionSource {
        movement_live::LiveCollisionSource {
            domain: crate::systems::collision::DOMAIN_LAND,
            block_radius: 1,
            big_radius: 48,
            push_size: 0,
            push_circles: 0,
            unit_flags: 0,
            unit_flags2: 0,
            attack_value: 0,
            spell_id: -1,
            unpacking: false,
            captain: false,
            moving: true,
            searching: false,
            action: OrderIndex::MoveTo as i32,
            invalid_tiles: Vec::new(),
            guys: vec![movement_live::LiveCollisionGuy {
                x,
                y,
                angle: 0,
                block_radius: 1,
            }],
        }
    }

    fn prepare_direct_move(sim: &mut Sim, row: usize, target: (i32, i32), speed: i16) {
        sim.world.units.myspeed_mut()[row] = speed;
        let start = (
            sim.world.units.x_internal()[row],
            sim.world.units.y_internal()[row],
        );
        sim.world.units.angle_mut()[row] =
            crate::trig::find_angle(target.0 - start.0, target.1 - start.1);
        sim.world
            .orders_mut(row)
            .replace(Order::move_to(target.0, target.1, 0));
        sim.paths[row].clear();
        sim.paths[row].push(movement::PathData {
            to_x: target.0,
            to_y: target.1,
            tolerance: 0,
            flags: movement::PathData::FLAG_MORE,
        });
    }

    #[test]
    fn movement_collision_without_authoritative_source_holds_and_charges_gap() {
        let mut sim = Sim::new(0x71, 3);
        sim.activate(0);
        let h = sim.spawn_unit(0, 1, 360, 504, 1).unwrap();
        let row = sim.world.row_of(h).unwrap();
        prepare_direct_move(&mut sim, row, (504, 504), 144);
        sim.do_move(row);
        assert_eq!(
            (
                sim.world.units.x_internal()[row],
                sim.world.units.y_internal()[row]
            ),
            (360, 504)
        );
        assert_eq!(sim.cover.gaps[Gap::UnitDetectCollision.index()], 1);
        assert_eq!(sim.cover.unit_move_step, 0);
    }

    #[test]
    fn movement_collision_installed_clear_move_relocates_anchor_and_guy_stamp() {
        let mut sim = Sim::new(0x72, 3);
        sim.activate(0);
        let h = sim.spawn_unit(0, 1, 360, 504, 1).unwrap();
        let row = sim.world.row_of(h).unwrap();
        sim.install_movement_collision_source(h, movement_source(360, 504))
            .unwrap();
        prepare_direct_move(&mut sim, row, (504, 504), 144);
        sim.do_move(row);

        assert_eq!(
            (
                sim.world.units.x_internal()[row],
                sim.world.units.y_internal()[row]
            ),
            (504, 504)
        );
        let source = sim.movement_collision.source(row).unwrap();
        assert_eq!((source.guys[0].x, source.guys[0].y), (504, 504));
        assert_eq!(sim.cover.gaps[Gap::UnitDetectCollision.index()], 0);
        assert_eq!(sim.cover.unit_move_step, 1);
    }

    #[test]
    fn movement_collision_installed_blocker_persists_detour_order_state() {
        let mut sim = Sim::new(0x73, 3);
        sim.activate(0);
        let actor = sim.spawn_unit(0, 1, 360, 504, 1).unwrap();
        let blocker = sim.spawn_unit(0, 1, 600, 504, 1).unwrap();
        let actor_row = sim.world.row_of(actor).unwrap();
        sim.install_movement_collision_source(actor, movement_source(360, 504))
            .unwrap();
        let mut blocker_source = movement_source(600, 504);
        blocker_source.action = OrderIndex::None as i32;
        blocker_source.moving = false;
        sim.install_movement_collision_source(blocker, blocker_source)
            .unwrap();
        prepare_direct_move(&mut sim, actor_row, (504, 504), 144);
        sim.do_move(actor_row);

        assert_eq!(
            (
                sim.world.units.x_internal()[actor_row],
                sim.world.units.y_internal()[actor_row]
            ),
            (360, 504),
            "the blocked proposed step never becomes a translation"
        );
        assert_eq!(sim.world.units.collide_who()[actor_row], 0);
        assert_eq!(sim.world.units.collide_o()[actor_row], 1);
        assert!(
            sim.movement_collision.order_state[actor_row]
                .detour
                .is_some(),
            "resolver persisted the first clear local detour"
        );
        assert_eq!(sim.cover.gaps[Gap::UnitDetectCollision.index()], 0);
        assert_eq!(sim.cover.unit_move_step, 1);
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
            cruise_pose: ammo::CruiseLaunchPose::default(),
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

    #[test]
    fn live_unit_death_spawns_exact_crash_before_corpse_and_clears_stale_shot_sidecar() {
        let mut sim = Sim::new(0x1234_5678, 16);
        sim.activate(2);
        let h = sim.spawn_unit(2, 99, 2_200, 5_100, 4).unwrap();
        let row = arm_exact_one_guy_crash(&mut sim, h);
        sim.world.units.z_internal_mut()[row] = 900;
        sim.install_crash_env(CrashTerrain {
            xs: sim.map.world.xs,
            ys: sim.map.world.ys,
            z: 37,
        });
        sim.ammo.ammo_index = 77;
        sim.shots[0].damage = 9_999;
        let mut expected_rng = ammo::Rng(sim.world.random.state() as u32);
        expected_rng.draw16();

        sim.apply_damage(row, 200);

        assert_eq!(sim.cover.crash_spawned, 1);
        assert_eq!(sim.cover.crash_missing_facts, 0);
        assert_eq!(sim.cover.deaths, 1);
        assert_eq!(sim.world.units.get_flags(row) & OBJ_FLAG_ACTIVE, 0);
        assert_eq!(sim.world.random.state(), expected_rng.0 as i32);
        assert_eq!(sim.ammo.ammo_index, 78);
        assert_eq!(sim.ammo.live(), 1);
        let wreck = &sim.ammo.slots[0].w;
        assert_eq!((wreck.index, wreck.graph_index), (0, 77));
        assert_eq!((wreck.who, wreck.o), (2, sim.world.units.o()[row] as i32));
        assert_eq!((wreck.sx, wreck.sy, wreck.sz), (2_000, 5_000, 1_000));
        assert_eq!(wreck.ex, 2_200, "endpoint x comes from the owning Unit");
        assert_eq!(wreck.ez, 37);
        assert_eq!(wreck.start_roll_angle, -13);
        assert_eq!(wreck.traj, ammo::TRAJ_ARC);
        assert_eq!(
            sim.shots[0].damage, 0,
            "a recycled ordinary shot cannot leak damage"
        );
        assert!(
            sim.deaths.slots.iter().any(|death| death.valid != 0),
            "corpse transaction still follows"
        );
    }

    #[test]
    fn live_crash_missing_terrain_fails_closed_before_pool_rng_or_sidecar_mutation() {
        let mut sim = Sim::new(0x8765_4321, 16);
        sim.activate(2);
        let h = sim.spawn_unit(2, 99, 2_200, 5_100, 4).unwrap();
        let row = arm_exact_one_guy_crash(&mut sim, h);
        let before_checksum = sim.ammo.checksum();
        let before_index = sim.ammo.ammo_index;
        let before_rng = sim.world.random.state();
        let before_shot = sim.shots[0].damage;

        sim.apply_damage(row, 200);

        assert_eq!(sim.cover.crash_spawned, 0);
        assert_eq!(sim.cover.crash_missing_facts, 1);
        assert_eq!(sim.ammo.checksum(), before_checksum);
        assert_eq!(sim.ammo.ammo_index, before_index);
        assert_eq!(sim.world.random.state(), before_rng);
        assert_eq!(sim.shots[0].damage, before_shot);
        assert_eq!(sim.world.units.get_flags(row) & OBJ_FLAG_ACTIVE, 0);
        assert_eq!(
            sim.cover.deaths, 1,
            "crash suppression must not suppress death"
        );
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

    /// Retail's GameDaemon shell is unconditional even in an empty world: victory,
    /// collision-block cursor maintenance, and the Groups cursor still execute. Every
    /// other empty-body step stays vacuous, leaving exactly those three scheduled steps.
    #[test]
    fn an_empty_world_executes_only_the_counters() {
        let mut sim = Sim::new(5, 8);
        let t = sim.do_frame();
        assert!(!t.steps[8].ran(), "no active leaders, so no economy ran");
        assert!(t.steps[12].ran(), "the GameDaemon shell is unconditional");
        assert!(!t.steps[14].ran(), "no objects, so no object pass ran");
        assert!(
            !t.steps[15].ran(),
            "no objects, projectiles or corpses, so the inc_time shell had nothing to visit"
        );
        assert!(t.steps[20].ran() && t.steps[23].ran());
        assert_eq!(t.executed(), 3);
    }

    #[test]
    fn cannon_time_expires_on_the_exact_post_increment_frame() {
        let mut sim = Sim::new(6, 8);
        sim.cannon_time = CannonTimeState {
            active_player: 3,
            start_frame: 0,
            pending_speed: 4,
            current_speed: 2,
        };
        for _ in 0..74 {
            let t = sim.do_frame();
            assert!(t.steps[24].ran());
            assert_eq!(t.work[24], 0);
            assert_eq!(sim.cannon_time.active_player, 3);
        }
        let t = sim.do_frame();
        assert_eq!(sim.world.frame, 75);
        assert_eq!(t.work[24], 1);
        assert_eq!(sim.cannon_time.active_player, -1);
        assert_eq!(sim.cannon_time.current_speed, 4);
        assert_eq!(sim.cannon_time.pending_speed, 0);
    }

    #[test]
    fn inactive_cannon_time_is_vacuous_and_does_not_reapply_speed() {
        let mut sim = Sim::new(8, 8);
        sim.cannon_time.current_speed = 3;
        sim.cannon_time.pending_speed = 1;
        let t = sim.do_frame();
        assert_eq!(t.steps[24], StepRun::Vacuous);
        assert_eq!(sim.cannon_time.current_speed, 3);
        assert_eq!(sim.cannon_time.pending_speed, 1);
    }

    #[test]
    fn process_end_game_consumes_the_exact_one_shot_latch() {
        let mut sim = Sim::new(9, 8);
        sim.vic_match
            .set_sem(victory_score::game_sem::VICTORY_RESOLVED);

        let first = sim.do_frame();
        assert_eq!(first.steps[27], StepRun::Executed);
        assert_eq!(first.work[27], 1);
        assert!(!sim.vic_match.sem(victory_score::game_sem::VICTORY_RESOLVED));

        let second = sim.do_frame();
        assert_eq!(second.steps[27], StepRun::Vacuous);
        assert_eq!(second.work[27], 0);
    }

    #[test]
    fn completed_wonder_live_value_drives_the_standard_countdown() {
        let value = Arc::new(AtomicI32::new(0));
        let mut sim = Sim::new(10, 8);
        sim.activate(0);
        sim.activate(1);
        sim.vic_match.options.victory = victory_score::Victory::Standard as u8;
        // This 32-cell-wide test map scales the rule against the 70-wide standard map;
        // four source frames become a two-frame countdown by Game::wonder_timer.
        sim.vic_match.constants.wonder_timer = 4;
        sim.wonder_world = Some(Box::new(TickWonderWorld {
            who: 0,
            o: 12,
            value: Arc::clone(&value),
            prerequisite_set: false,
        }));
        assert_eq!(sim.init_completed_wonder(0, 12), Ok(0));

        // A current value below the threshold neither arms nor wins.
        sim.do_frame();
        assert_eq!(sim.vic_leaders.slots[0].wonderwin_timer, 0);
        assert!(!sim.vic_leaders.slots[0].flag(victory_score::leader_flag::WON));

        // The next frame queries the live host again and arms at that frame.
        value.store(1, Ordering::SeqCst);
        sim.do_frame();
        assert_eq!(sim.vic_leaders.slots[0].wonderwin_timer, 1);
        assert_eq!(sim.vic_leaders.slots[0].wonderwin_stamp, 1);
        sim.do_frame();
        assert!(!sim.vic_leaders.slots[0].flag(victory_score::leader_flag::WON));
        sim.do_frame();
        assert!(sim.vic_leaders.slots[0].flag(victory_score::leader_flag::WON));
        assert!(sim.wonder_error.is_none());
    }

    #[test]
    fn live_world_government_prerequisite_bypasses_the_wonder_countdown() {
        let value = Arc::new(AtomicI32::new(1));
        let mut sim = Sim::new(11, 8);
        sim.activate(0);
        sim.activate(1);
        sim.vic_match.options.victory = victory_score::Victory::Standard as u8;
        sim.wonder_world = Some(Box::new(TickWonderWorld {
            who: 0,
            o: 12,
            value,
            prerequisite_set: false,
        }));
        assert_eq!(sim.init_completed_wonder(0, 12), Ok(0));

        let world_government = 600;
        let mut instant_timer_bonus = production::runtime::LiveProductionType::research(0x2b9, 1);
        instant_timer_bonus.prerequisites = vec![world_government];
        sim.production_runtime.install_type(instant_timer_bonus);
        sim.production_runtime.leaders[0]
            .tech
            .tech
            .set(world_government, true);
        let queued_type = 601;
        let row = sim.spawn_build(
            0,
            production::BuildData {
                flags: production::flag::VALID | production::flag::ACTIVE,
                build_masks: production::mask::REPEAT_QUEUE,
                queue: production::BuildQueue {
                    queued: 1,
                    entries: vec![production::BuildQueueEntry {
                        elapsed: 31,
                        type_index: queued_type as i16,
                        ..Default::default()
                    }],
                },
                ..Default::default()
            },
        );
        sim.production_runtime.leaders[0].queued_counts[queued_type as usize] = 1;

        sim.do_frame();

        assert!(sim.vic_leaders.slots[0].has_preq_2b9);
        assert!(sim.vic_leaders.slots[0].flag(victory_score::leader_flag::WON));
        assert_eq!(sim.vic_leaders.slots[0].wonderwin_timer, 0);
        assert_ne!(
            sim.vic_leaders.slots[0].leader_flags2 & victory_score::leader_flag2::INSTANT_VICTORY,
            0
        );
        assert_eq!(sim.builds[row].queue.queued, 0);
        assert_eq!(sim.builds[row].queue.entries[0].elapsed, 0);
        assert_eq!(
            sim.production_runtime.leaders[0].queued_counts[queued_type as usize],
            0
        );
    }

    #[test]
    fn step11_victory_flushes_concrete_build_queues_before_step12() {
        let mut sim = Sim::new(12, 8);
        sim.activate(0);
        sim.activate(1);
        sim.vic_match
            .set_sem(victory_score::game_sem::CHECK_VICTORY_MODE);
        sim.vic_leaders.slots[1].leader_flags &= !victory_score::leader_flag::ACTIVE;
        sim.vic_leaders.slots[1].leader_flags |= victory_score::leader_flag::DEFEATED;

        let queued_type = 600;
        let build = production::BuildData {
            flags: production::flag::VALID | production::flag::ACTIVE,
            build_masks: production::mask::REPEAT_QUEUE,
            queue: production::BuildQueue {
                queued: 1,
                entries: vec![production::BuildQueueEntry {
                    elapsed: 23,
                    type_index: queued_type as i16,
                    ..Default::default()
                }],
            },
            ..Default::default()
        };
        let row = sim.spawn_build(0, build);
        sim.production_runtime.leaders[0].queued_counts[queued_type as usize] = 1;

        sim.do_frame();

        assert!(sim.vic_leaders.slots[0].flag(victory_score::leader_flag::WON));
        assert_eq!(sim.builds[row].queue.queued, 0);
        assert_eq!(sim.builds[row].queue.entries[0].elapsed, 0);
        assert_eq!(
            sim.builds[row].build_masks & production::mask::REPEAT_QUEUE,
            0
        );
        assert_eq!(
            sim.production_runtime.leaders[0].queued_counts[queued_type as usize],
            0
        );
        assert_eq!(sim.vic_leaders.take_terminal_queue_cleanup(), 0);
    }

    #[test]
    fn defeated_owner_kills_planes_closes_ground_orders_and_clears_the_army_mask() {
        let mut sim = Sim::new(13, 8);
        sim.activate(0);
        sim.activate(1);
        let ground_type = 100;
        let plane_type = 101;
        sim.production_runtime.install_type(
            production::runtime::LiveProductionType::ordinary_unit(ground_type, 1, 1),
        );
        sim.production_runtime.install_type(
            production::runtime::LiveProductionType::hosted_air_unit(plane_type, 1, 1),
        );

        let ground = sim.spawn_unit(0, ground_type, 111, 222, 4).unwrap();
        let plane = sim.spawn_unit(0, plane_type, 333, 444, 4).unwrap();
        let survivor = sim.spawn_unit(1, ground_type, 555, 666, 4).unwrap();
        let ground_row = sim.world.row_of(ground).unwrap();
        let plane_row = sim.world.row_of(plane).unwrap();
        let survivor_row = sim.world.row_of(survivor).unwrap();
        let plane_o = sim.world.units.o()[plane_row] as i32;

        for row in [ground_row, plane_row, survivor_row] {
            sim.world
                .orders_mut(row)
                .replace(Order::move_to(900, 901, 48));
            sim.paths[row].push(movement::PathData::default());
            sim.world
                .units
                .set_unit_masks(row, defeat_cleanup::DEFEAT_UNIT_MASK | 0x0400_0000 | 0x20);
        }

        sim.vic_leaders.defeat(
            &mut sim.vic_match,
            0,
            victory_score::DefeatType::Resign,
            -1,
            0,
        );
        sim.flush_terminal_queue_cleanup();

        assert_eq!(sim.defeat_cleanup_error, None);
        assert_ne!(sim.world.units.get_flags(ground_row) & OBJ_FLAG_ACTIVE, 0);
        assert_eq!(sim.world.units.get_flags(plane_row) & OBJ_FLAG_ACTIVE, 0);
        assert_ne!(sim.world.units.get_flags(survivor_row) & OBJ_FLAG_ACTIVE, 0);
        assert!(sim.world.orders(ground_row).is_empty());
        assert!(sim.paths[ground_row].is_empty());
        assert_eq!(
            sim.world.units.get_unit_masks(ground_row)
                & (defeat_cleanup::DEFEAT_UNIT_MASK | 0x0400_0000),
            0
        );
        assert_eq!(
            sim.world.units.get_unit_masks(plane_row) & defeat_cleanup::DEFEAT_UNIT_MASK,
            0
        );
        assert_eq!(sim.world.orders(survivor_row).len(), 1);
        assert_eq!(sim.paths[survivor_row].len(), 1);
        assert_ne!(
            sim.world.units.get_unit_masks(survivor_row) & defeat_cleanup::DEFEAT_UNIT_MASK,
            0
        );
        assert!(sim
            .deaths
            .slots
            .iter()
            .any(|death| { death.valid != 0 && death.who == 0 && death.o == plane_o }));
        assert_eq!(sim.vic_leaders.take_defeat_unit_cleanup(), 0);
    }

    #[test]
    fn defeated_owner_stops_standing_army_groups_without_closing_the_army() {
        let mut sim = Sim::new(13, 8);
        let unit_type = 109;
        sim.production_runtime.install_type(
            production::runtime::LiveProductionType::ordinary_unit(unit_type, 1, 1),
        );
        let unit = sim.spawn_unit(0, unit_type, 321, 654, 4).unwrap();
        let row = sim.world.row_of(unit).unwrap();
        let object_id = sim.world.units.o()[row];
        sim.world
            .orders_mut(row)
            .replace(Order::move_to(900, 901, 48));
        sim.paths[row].push(movement::PathData::default());
        sim.world.units.set_unit_masks(
            row,
            defeat_cleanup::DEFEAT_UNIT_MASK | 0x0400_0000 | 0x0000_0100 | 0x20,
        );

        let group_id = groups_guys::Groups::index(0, 3);
        let mut group = groups_guys::GroupData {
            id: group_id as i32,
            form: 4,
            disband: 7,
            ..Default::default()
        };
        assert!(group.add(object_id, 0, false, 0, 0));
        sim.groups.list[group_id] = group;
        sim.armies.lists[0][5].valid = 1;
        sim.armies.lists[0][5].who = 0;
        sim.armies.lists[0][5].num_groups = 1;
        sim.armies.lists[0][5].list[0] = group_id as i32;
        let army_before = sim.armies.lists[0][5].clone();

        let runtime = std::mem::take(&mut sim.production_runtime);
        let receipt = sim.clean_defeated_unit_band(&runtime, 0).unwrap();
        sim.production_runtime = runtime;

        assert_eq!(receipt.armies_stopped, 1);
        assert_eq!(receipt.groups_stopped, 1);
        assert_eq!(receipt.army_members_halted, 1);
        assert_eq!(sim.armies.lists[0][5], army_before);
        assert_eq!(sim.groups.list[group_id].form, -1);
        assert_eq!(sim.groups.list[group_id].disband, 0);
        assert!(sim.world.orders(row).is_empty());
        assert!(sim.paths[row].is_empty());
        assert_eq!(
            sim.world.units.get_unit_masks(row)
                & (defeat_cleanup::DEFEAT_UNIT_MASK | 0x0400_0000 | 0x0000_0100),
            0
        );
    }

    #[test]
    fn missing_special_anim_payload_keeps_the_whole_defeat_transaction_unmutated() {
        let mut sim = Sim::new(14, 8);
        let unit_type = 119;
        sim.production_runtime.install_type(
            production::runtime::LiveProductionType::ordinary_unit(unit_type, 1, 1),
        );
        let unit = sim.spawn_unit(0, unit_type, 10, 20, 4).unwrap();
        let row = sim.world.row_of(unit).unwrap();
        let object_id = sim.world.units.o()[row];
        sim.world.orders_mut(row).replace(Order {
            kind: OrderIndex::SpecialAnim,
            ..Order::default()
        });
        sim.paths[row].push(movement::PathData::default());
        sim.world.units.set_unit_masks(
            row,
            defeat_cleanup::DEFEAT_UNIT_MASK | 0x0400_0000 | 0x0000_0100,
        );

        let group_id = groups_guys::Groups::index(0, 2);
        let mut group = groups_guys::GroupData {
            id: group_id as i32,
            form: 6,
            disband: 8,
            ..Default::default()
        };
        assert!(group.add(object_id, 0, false, 0, 0));
        sim.groups.list[group_id] = group.clone();
        sim.armies.lists[0][1].valid = 1;
        sim.armies.lists[0][1].num_groups = 1;
        sim.armies.lists[0][1].list[0] = group_id as i32;

        let runtime = std::mem::take(&mut sim.production_runtime);
        let error = sim.clean_defeated_unit_band(&runtime, 0).unwrap_err();
        sim.production_runtime = runtime;

        assert_eq!(
            error,
            defeat_cleanup::DefeatCleanupError::MissingSpecialAnimPayload {
                owner: 0,
                object_id: object_id as usize,
            }
        );
        assert_eq!(sim.groups.list[group_id], group);
        assert_eq!(sim.world.orders(row).order_type(), OrderIndex::SpecialAnim);
        assert_eq!(sim.paths[row].len(), 1);
        assert_eq!(
            sim.world.units.get_unit_masks(row)
                & (defeat_cleanup::DEFEAT_UNIT_MASK | 0x0400_0000 | 0x0000_0100),
            defeat_cleanup::DEFEAT_UNIT_MASK | 0x0400_0000 | 0x0000_0100
        );
    }

    #[test]
    fn standing_army_skips_enter_animation_but_halts_special_unit_animation() {
        let mut sim = Sim::new(15, 8);
        let unit_type = 121;
        sim.production_runtime.install_type(
            production::runtime::LiveProductionType::ordinary_unit(unit_type, 1, 1),
        );
        let entering = sim.spawn_unit(0, unit_type, 10, 20, 4).unwrap();
        let special_unit = sim.spawn_unit(0, unit_type, 30, 40, 4).unwrap();
        let entering_row = sim.world.row_of(entering).unwrap();
        let special_unit_row = sim.world.row_of(special_unit).unwrap();
        let entering_o = sim.world.units.o()[entering_row];
        let special_unit_o = sim.world.units.o()[special_unit_row];
        sim.world
            .orders_mut(entering_row)
            .replace(Order::special_anim(
                crate::order::SpecialAnimType::Enter,
                1,
                2,
            ));
        sim.world
            .orders_mut(special_unit_row)
            .replace(Order::special_anim(
                crate::order::SpecialAnimType::Unit,
                3,
                4,
            ));
        for row in [entering_row, special_unit_row] {
            sim.paths[row].push(movement::PathData::default());
            sim.world.units.set_unit_masks(
                row,
                defeat_cleanup::DEFEAT_UNIT_MASK | 0x0400_0000 | 0x0000_0100,
            );
        }

        let group_id = groups_guys::Groups::index(0, 4);
        let mut group = groups_guys::GroupData {
            id: group_id as i32,
            form: 5,
            disband: 6,
            ..Default::default()
        };
        assert!(group.add(entering_o, 0, false, 0, 0));
        assert!(group.add(special_unit_o, 0, false, 0, 0));
        sim.groups.list[group_id] = group;
        sim.armies.lists[0][2].valid = 1;
        sim.armies.lists[0][2].num_groups = 1;
        sim.armies.lists[0][2].list[0] = group_id as i32;

        let runtime = std::mem::take(&mut sim.production_runtime);
        let receipt = sim.clean_defeated_unit_band(&runtime, 0).unwrap();
        sim.production_runtime = runtime;

        assert_eq!(receipt.army_members_halted, 1);
        assert_eq!(sim.groups.list[group_id].form, -1);
        assert_eq!(sim.groups.list[group_id].disband, 0);
        assert_ne!(
            sim.world.units.get_unit_masks(entering_row) & 0x0000_0100,
            0,
            "SPECIAL_ENTER is skipped by Army::stop"
        );
        assert_eq!(
            sim.world.units.get_unit_masks(special_unit_row) & 0x0000_0100,
            0,
            "SPECIAL_UNIT is halted by Army::stop"
        );
        assert!(sim.world.orders(entering_row).is_empty());
        assert!(sim.world.orders(special_unit_row).is_empty());
    }

    #[test]
    fn defeated_owner_preflight_keeps_every_unit_untouched_on_an_unknown_type() {
        let mut sim = Sim::new(14, 8);
        sim.activate(0);
        sim.activate(1);
        let known_type = 110;
        let unknown_type = 111;
        sim.production_runtime.install_type(
            production::runtime::LiveProductionType::ordinary_unit(known_type, 1, 1),
        );
        let known = sim.spawn_unit(0, known_type, 10, 20, 4).unwrap();
        let unknown = sim.spawn_unit(0, unknown_type, 30, 40, 4).unwrap();
        let known_row = sim.world.row_of(known).unwrap();
        let unknown_row = sim.world.row_of(unknown).unwrap();
        for row in [known_row, unknown_row] {
            sim.world
                .orders_mut(row)
                .replace(Order::move_to(100, 200, 48));
            sim.paths[row].push(movement::PathData::default());
            sim.world
                .units
                .set_unit_masks(row, defeat_cleanup::DEFEAT_UNIT_MASK | 0x0400_0000);
        }

        sim.vic_leaders.defeat(
            &mut sim.vic_match,
            0,
            victory_score::DefeatType::Resign,
            -1,
            0,
        );
        sim.flush_terminal_queue_cleanup();

        assert_eq!(
            sim.defeat_cleanup_error,
            Some(defeat_cleanup::DefeatCleanupError::UnsupportedUnitType {
                owner: 0,
                object_id: 1,
                type_index: unknown_type,
            })
        );
        for row in [known_row, unknown_row] {
            assert_eq!(sim.world.orders(row).len(), 1);
            assert_eq!(sim.paths[row].len(), 1);
            assert_ne!(
                sim.world.units.get_unit_masks(row) & defeat_cleanup::DEFEAT_UNIT_MASK,
                0
            );
            assert_ne!(sim.world.units.get_flags(row) & OBJ_FLAG_ACTIVE, 0);
        }
        assert_eq!(sim.vic_leaders.take_defeat_unit_cleanup(), 1);
    }

    #[test]
    fn step14_research_completion_reaches_tech_race_and_cleans_before_return() {
        let mut sim = Sim::new(14, 8);
        sim.activate(0);
        sim.activate(1);
        sim.vic_match.options.victory = victory_score::Victory::TechRace as u8;
        sim.vic_match.options.ending_technology = 1;
        let age = crate::systems::tech_cities::ty::CLASSICAL_AGE;
        let producer_type = 430;
        let row = sim.spawn_build(
            0,
            production::BuildData {
                flags: production::flag::VALID | production::flag::ACTIVE,
                queue: production::BuildQueue {
                    queued: 1,
                    entries: vec![production::BuildQueueEntry {
                        elapsed: 1,
                        type_index: age as i16,
                        res: [-1; 3],
                        ..Default::default()
                    }],
                },
                ..Default::default()
            },
        );
        sim.production_runtime.register_build(row, producer_type);
        sim.production_runtime.install_type(
            production::runtime::LiveProductionType::in_place_building(producer_type, 1),
        );
        sim.production_runtime
            .install_type(production::runtime::LiveProductionType::research(age, 1));
        let opponent_plane_type = 120;
        sim.production_runtime.install_type(
            production::runtime::LiveProductionType::hosted_air_unit(opponent_plane_type, 1, 1),
        );
        let opponent_plane = sim.spawn_unit(1, opponent_plane_type, 700, 800, 4).unwrap();
        let opponent_plane_row = sim.world.row_of(opponent_plane).unwrap();
        sim.world
            .units
            .set_unit_masks(opponent_plane_row, defeat_cleanup::DEFEAT_UNIT_MASK | 0x80);

        sim.do_frame();

        assert!(sim.production_runtime.leaders[0].tech.tech.get(age));
        assert!(sim.vic_leaders.slots[0].flag(victory_score::leader_flag::WON));
        assert_eq!(
            sim.vic_leaders.slots[0].victory_type,
            victory_score::VictoryType::ByTechRace as i32
        );
        assert!(sim.vic_leaders.slots[1].flag(victory_score::leader_flag::DEFEATED));
        assert_eq!(sim.builds[row].queue.queued, 0);
        assert_eq!(sim.builds[row].queue.entries[0].elapsed, 0);
        assert_eq!(sim.vic_leaders.take_terminal_queue_cleanup(), 0);
        assert_eq!(sim.vic_leaders.take_defeat_unit_cleanup(), 0);
        assert_eq!(
            sim.world.units.get_flags(opponent_plane_row) & OBJ_FLAG_ACTIVE,
            0,
            "step-14 Tech Race must finish the defeated Unit sweep before returning"
        );
        assert_eq!(
            sim.world.units.get_unit_masks(opponent_plane_row) & defeat_cleanup::DEFEAT_UNIT_MASK,
            0
        );
    }

    #[test]
    fn active_wonder_without_world_blocks_the_victory_sweep_fail_closed() {
        let value = Arc::new(AtomicI32::new(1));
        let mut seed_world = TickWonderWorld {
            who: 0,
            o: 12,
            value,
            prerequisite_set: false,
        };
        let mut sim = Sim::new(12, 8);
        sim.activate(0);
        sim.activate(1);
        sim.vic_match.options.victory = victory_score::Victory::Wonder as u8;
        sim.wonders.init_wonder(&mut seed_world, 0, 12).unwrap();

        sim.do_frame();
        assert_eq!(sim.wonder_error, Some(wonders::WonderError::MissingWorld));
        assert_eq!(sim.cover.gaps[Gap::WonderValueWorld.index()], 1);
        assert!(!sim.vic_leaders.slots[0].flag(victory_score::leader_flag::WON));
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

    /// Even a receipt-complete detector Unit cannot authorize a Unit-only plane rebuild. The
    /// real `do_frame` call must fail before the GameDaemon shell and preserve sections 6/7.
    #[test]
    fn phase33_preflights_exact_units_but_preserves_planes_until_full_owner_lands() {
        use crate::systems::map_terrain::WorldSection;
        use crate::systems::step12_visibility_producer_frontier::OBJMASK_DETECT;
        use crate::systems::step12_visibility_runtime::{
            Step12ProducerResiduals, Step12VisibilityPreflightError, VisibilityConstants,
            VisibilityTypeProjection, UNIT_TYPE_BASE,
        };

        let mut sim = Sim::new(13, 16);
        sim.replace_step12_visibility_type_source(
            4,
            0x4455,
            VisibilityConstants {
                ptolemy_los_bonus: 2,
                the_ceo_unit_los: 2,
            },
            vec![VisibilityTypeProjection {
                type_index: UNIT_TYPE_BASE,
                object_masks: OBJMASK_DETECT,
                domain: 0,
                unit_flags2: 0,
                role: 0,
                is_siege: false,
            }],
        )
        .unwrap();
        sim.activate(0);
        let unit = sim.spawn_unit(0, UNIT_TYPE_BASE, 6000, 6000, 6).unwrap();
        sim.materialize_step12_object_init(unit).unwrap();
        sim.world.frame = 33;
        sim.game_daemon.busy = 7;
        sim.map.world.seen[3] = 0x91;
        sim.map.world.seen2[3] = 0x22;
        sim.map.world.seen3[3] = 0x84;
        sim.map.world.wcoord_seen[0] = 0x48;
        let before = sim.map.world.checksum_sections();

        let trace = sim.do_frame();

        assert_eq!(
            trace.steps[12],
            StepRun::Unimplemented(Gap::GameDaemonUpdateAllSeen)
        );
        assert_eq!(
            sim.game_daemon.busy, 7,
            "preflight must precede daemon mutation"
        );
        assert_eq!(
            sim.step12_visibility_error,
            Some(Step12VisibilityPreflightError::IncompleteProducer {
                prepared_unit_stamps: 1,
                residuals: Step12ProducerResiduals::MISSING,
            })
        );
        let after = sim.map.world.checksum_sections();
        assert_eq!(
            after.section(WorldSection::TDataAndFog),
            before.section(WorldSection::TDataAndFog)
        );
        assert_eq!(
            after.section(WorldSection::WCoordSeen),
            before.section(WorldSection::WCoordSeen)
        );
        assert_eq!(
            (sim.map.world.seen[3], sim.map.world.seen3[3]),
            (0x91, 0x84)
        );
    }

    #[test]
    fn fog_option_three_scheduled_do_frame_reads_no_authority_and_mutates_no_fog_plane() {
        use crate::systems::map_terrain::WorldSection;

        let mut sim = Sim::new(17, 8);
        sim.world.frame = 33;
        sim.map.fog.option = borders_fog::FogOption(3);
        sim.map.world.seen[2] = 0x12;
        sim.map.world.seen2[2] = 0x34;
        sim.map.world.seen3[2] = 0x56;
        sim.map.world.wcoord_seen[0] = 0x78;
        let before = sim.map.world.checksum_sections();

        let trace = sim.do_frame();

        assert!(matches!(trace.steps[12], StepRun::Executed));
        assert_eq!(sim.game_daemon.busy, 0);
        assert_eq!(sim.step12_visibility_error, None);
        let after = sim.map.world.checksum_sections();
        assert_eq!(
            after.section(WorldSection::TDataAndFog),
            before.section(WorldSection::TDataAndFog)
        );
        assert_eq!(
            after.section(WorldSection::WCoordSeen),
            before.section(WorldSection::WCoordSeen)
        );
    }

    #[test]
    fn signed_negative_nonphase_do_frame_never_enters_visibility_authority() {
        use crate::systems::map_terrain::WorldSection;

        let mut sim = Sim::new(19, 8);
        sim.world.frame = -67;
        sim.map.world.seen[1] = 0xa5;
        sim.map.world.seen3[1] = 0x5a;
        let before = sim.map.world.checksum_sections();

        let trace = sim.do_frame();

        assert!(matches!(trace.steps[12], StepRun::Executed));
        assert_eq!(sim.step12_visibility_error, None);
        assert_eq!(
            sim.map
                .world
                .checksum_sections()
                .section(WorldSection::TDataAndFog),
            before.section(WorldSection::TDataAndFog)
        );
    }

    #[test]
    fn step12_authority_digest_changes_without_impersonating_checksum_channel_12() {
        use crate::systems::step12_visibility_runtime::VisibilityConstants;

        let mut sim = Sim::new(23, 8);
        let channel_before = sim.channel_digest();
        let authority_before = sim.step12_visibility_authority_digest();

        sim.replace_step12_visibility_type_source(
            9,
            0x12_12_12_12,
            VisibilityConstants {
                ptolemy_los_bonus: 2,
                the_ceo_unit_los: 2,
            },
            Vec::new(),
        )
        .unwrap();

        assert_ne!(sim.step12_visibility_authority_digest(), authority_before);
        assert_eq!(sim.channel_digest(), channel_before);
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
