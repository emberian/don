//! One environment's mutable state: the `don-sim` world plus the per-player leader state
//! the RL surface needs.
//!
//! # What is real and what is scaffolding
//!
//! Storage, identity and the tick kernels are `don-sim`'s: dense SoA rows, generational
//! handles, the vector `tick_down`. Damage resolution is `don_sim::mechanics::damage`, the
//! derived integer chain from `ObjectData::get_damage` `0x00644130`, driven with the real
//! 493×493 balance table when `schema/live/balance-real.bin` is present.
//!
//! Construction, tech, pathing and fog remain **scaffolding**. Ordinary VecEnv gathering is
//! masked, while an explicit Farm-only [`GatherHost`] seam owns the recovered order,
//! occupancy, payout and retirement transaction without guessing its still-external inputs.
//! Every other scaffolded verb increments [`EnvWorld::unimplemented`] so a training run can
//! print exactly which parts of the action space have no dynamics behind them. That counter
//! is the point: an RL surface whose gaps are silent is worse than no surface at all.

use crate::generated as g;
use crate::typecaps::{
    load_formation_caps, FormationCaps, TypeCap, TypeCaps, F_ATTACK, F_BUILDING, F_MOVE,
};
use don_sim::command::{Fleet, QueuePos};
use don_sim::objects::Band;
use don_sim::order::{OrderIndex, ORDER_PATHED};
use don_sim::systems::collision::{UnitRow, UnitTable, DOMAIN_LAND};
use don_sim::systems::containment::NearbyUnitType;
use don_sim::systems::economy::{
    self, CapGates, DoGatherContext, EconRules, GatherInputs, LeaderEcon,
};
use don_sim::systems::gather_lifecycle::{
    attached_ordinary_order_state, ensure_ordinary_attachment, farm_first_gather_tick,
    FarmFirstTickDisposition, OrdinaryGatherKind, OrdinaryGatherTarget,
};
use don_sim::systems::gathering::{
    check_gatherers, num_gatherers, retire_gather_order, site_gross, AttachResult,
    GatherAssignment, GatherCount, GatherOrderWalk, GatherSite, GatherWorker, NonFlatGatherState,
    NO_OBJECT,
};
use don_sim::systems::groups_guys::FormationMember;
use don_sim::systems::movement::vector_dist;
use don_sim::systems::order_dispatch::{
    install_air_patrol, install_group_patrol, AirPatrolSearch, OrderQueue, OrderRec, PatrolInstall,
    PatrolPayload, UnitWork,
};
use don_sim::systems::patrol::{
    self, AirPatrolAction, AirPatrolAfterPhysics, AirPatrolOrder, AirPatrolTarget,
    GroundPatrolAction,
};
use don_sim::world::SUBTILE;
use don_sim::{Handle, World};
use std::sync::Arc;

/// Static tables shared by every world in a batch; never mutated after construction.
pub struct Rules {
    pub caps: TypeCaps,
    /// Captured postload runtime facts used by `Form::categorize`. Empty/`None` when the
    /// ignored live table is unavailable; formation then remains masked at the host seam.
    pub formation_caps: FormationCaps,
    /// `Balance::final_balance_table` at `0x00C12BF4`, `short[493][493]` [measured].
    /// `None` when `schema/live/balance-real.bin` is absent; the damage chain then sees a
    /// flat 100 % balance term and [`Rules::balance_is_real`] is false.
    pub balance: Option<Vec<i16>>,
    pub building_types: Vec<u8>,
}

pub const BALANCE_DIM: usize = 493;

impl Rules {
    pub fn load(
        typecaps: Option<&std::path::Path>,
        balance: Option<&std::path::Path>,
    ) -> (Arc<Rules>, bool, bool) {
        let (caps, caps_real) = TypeCaps::load_or_permissive(typecaps);
        let formation_caps = load_formation_caps(None).unwrap_or_else(|_| vec![None; g::NUM_TYPES]);
        let p = balance.map(|p| p.to_path_buf()).unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../schema/live/balance-real.bin")
        });
        let balance = std::fs::read(&p).ok().and_then(|b| {
            if b.len() < BALANCE_DIM * BALANCE_DIM * 2 {
                return None;
            }
            Some(
                b.chunks_exact(2)
                    .take(BALANCE_DIM * BALANCE_DIM)
                    .map(|c| i16::from_le_bytes([c[0], c[1]]))
                    .collect::<Vec<i16>>(),
            )
        });
        let bal_real = balance.is_some();
        let building_types = caps.building_bitset();
        (
            Arc::new(Rules {
                caps,
                formation_caps,
                balance,
                building_types,
            }),
            caps_real,
            bal_real,
        )
    }

    pub fn balance_is_real(&self) -> bool {
        self.balance.is_some()
    }

    #[inline]
    pub fn formation_cap(&self, type_id: u16) -> Option<crate::typecaps::FormationTypeCap> {
        self.formation_caps.get(type_id as usize).copied().flatten()
    }

    /// `(i32)(i16) balance[atk * 493 + def]`, the operand `get_damage` reads at
    /// `0x0064418E`. Types outside the table's 0..493 domain fall back to 100 %.
    #[inline]
    pub fn balance_pct(&self, atk: u16, def: u16) -> i32 {
        match &self.balance {
            Some(b) if (atk as usize) < BALANCE_DIM && (def as usize) < BALANCE_DIM => {
                b[don_sim::balance_index(atk as i32, def as i32) as usize] as i32
            }
            _ => 100,
        }
    }
}

/// The eleven `LeaderData` score fields, offsets 24..68 [measured,
/// `schema/state-schema.json`]. Field names are the engine's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScoreTerms {
    pub score_explored: i32,
    pub score_territory: i32,
    pub score_units: i32,
    pub score_units_2: i32,
    pub score_buildings: i32,
    pub score_economy: i32,
    pub score_pop: i32,
    pub score_unit_upgrades: i32,
    pub score_research: i32,
    pub score_wonders: i32,
    pub score_combat: i32,
}

impl ScoreTerms {
    pub const NAMES: [&'static str; 11] = [
        "score_explored",
        "score_territory",
        "score_units",
        "score_units_2",
        "score_buildings",
        "score_economy",
        "score_pop",
        "score_unit_upgrades",
        "score_research",
        "score_wonders",
        "score_combat",
    ];
    /// Terms this build actually computes. The rest are structurally present and always
    /// zero; see `docs/tracks/rl-env.md`.
    pub const LIVE: [&'static str; 5] = [
        "score_units",
        "score_buildings",
        "score_economy",
        "score_pop",
        "score_combat",
    ];

    pub fn as_array(&self) -> [i32; 11] {
        [
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
        ]
    }
    /// Our aggregation, **not** the engine's. `Leader::compute_score` `0x006EC560` is
    /// unread, so its weighting is underived; this is an unweighted sum and is labelled as
    /// such everywhere it surfaces.
    pub fn total(&self) -> i32 {
        self.as_array().iter().sum()
    }
}

/// Per-player state. Field names track `LeaderData` (`schema/state-schema.json`).
#[derive(Clone, Debug)]
pub struct PlayerState {
    pub who: u8,
    pub team: u8,
    pub alive: bool,
    /// `LeaderData::leader_flags` at `+0x00`; formation category tests bit 2.
    pub leader_flags: u32,
    /// Exact host fact for `Leader::has_tech(0x12)`, the first branch of
    /// `UnitData::is_modern_infantry`. Research remains outside the compact env, so this
    /// starts false and changes only when an explicit host supplies that tech state.
    pub has_modern_infantry_tech: bool,
    /// `LeaderData::defeat_type` (0 = not defeated) and `victory_type`.
    pub defeat_type: i32,
    pub victory_type: i32,
    /// `LeaderData::econ[6]`: FOOD TIMBER WEALTH KNOWLEDGE METAL OIL.
    pub econ: [i32; g::NUM_COMMON],
    /// `LeaderData::base_rate[6]`.
    pub base_rate: [i32; g::NUM_COMMON],
    /// `LeaderData::collected[6]`, lifetime.
    pub collected: [i32; g::NUM_COMMON],
    /// The recovered checksum-bearing economy block used by the explicit gathering path.
    /// `econ` remains the public compact stockpile mirror and is synchronized at each payout.
    pub leader_econ: LeaderEcon,
    pub gather_last_calc_frame: i32,
    pub gather_dirty: bool,
    pub pop: i32,
    pub pop_cap: i32,
    pub city_num: i32,
    pub units_built: i32,
    pub units_killed: i32,
    pub units_lost: i32,
    pub buildings_built: i32,
    pub buildings_lost: i32,
    pub territory: i32,
    pub explored: i32,
    /// `LeaderData::diplos[8]`, values from the WAR/PEACE/ALLY enum.
    pub diplos: [u8; g::NUM_PLAYERS],
    pub score: ScoreTerms,
    /// `LeaderData::num_buildings[129]`, indexed by `type - BUILD_TYPE_BASE`.
    pub num_buildings: Vec<u16>,
    /// `LeaderData::num_units[352]`, indexed by `type - UNIT_TYPE_BASE`.
    pub num_units: Vec<u16>,
}

impl PlayerState {
    fn new(who: u8) -> PlayerState {
        let mut diplos = [1u8; g::NUM_PLAYERS]; // PEACE
        diplos[who as usize] = 2; // ALLY with self
        let starting_econ = [200, 200, 200, 0, 0, 0];
        let mut leader_econ = LeaderEcon::new();
        leader_econ.stockpile = starting_econ;
        PlayerState {
            who,
            team: who,
            alive: true,
            leader_flags: 0,
            has_modern_infantry_tech: false,
            defeat_type: 0,
            victory_type: 0,
            // Placeholder starting stock. The real start economy comes from
            // `Constants::init` and is not wired; see the provenance report.
            econ: starting_econ,
            base_rate: [0; g::NUM_COMMON],
            collected: [0; g::NUM_COMMON],
            leader_econ,
            gather_last_calc_frame: -1,
            gather_dirty: true,
            pop: 0,
            pop_cap: 50,
            city_num: 0,
            units_built: 0,
            units_killed: 0,
            units_lost: 0,
            buildings_built: 0,
            buildings_lost: 0,
            territory: 0,
            explored: 0,
            diplos,
            score: ScoreTerms::default(),
            num_buildings: vec![0; g::NUM_BUILDTYPES],
            num_units: vec![0; g::NUM_UNITTYPES],
        }
    }

    #[inline]
    pub fn can_afford(&self, cost: &[i32; g::NUM_COMMON]) -> bool {
        (0..g::NUM_COMMON).all(|k| self.econ[k] >= cost[k])
    }
    #[inline]
    fn pay(&mut self, cost: &[i32; g::NUM_COMMON]) {
        for k in 0..g::NUM_COMMON {
            self.econ[k] -= cost[k];
        }
    }
}

/// Which verbs were accepted but have no dynamics behind them yet, by unit-verb index.
#[derive(Clone, Debug, Default)]
pub struct Unimplemented {
    pub unit: Vec<u64>,
    pub player: Vec<u64>,
}

/// One adjacent retail boundary required before an AIR_PATROL executor can advance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirPatrolHostBoundary {
    AirPhysics,
    UnitTargetSearch,
    BuildingTargetSearch,
    AnimalThinkBird,
    TypeIdentity,
}

/// Fail-closed error from an Arena-independent RL air-patrol host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirPatrolHostError {
    /// The host has not recovered this retail transaction.
    Unavailable(AirPatrolHostBoundary),
    /// A supposedly available host rejected incoherent runtime state.
    InvalidState(&'static str),
}

/// Exact host seam around `Unit::do_air_patrol`'s recovered order transition.
///
/// There are intentionally no default methods. In particular, a host cannot inherit
/// straight-line physics or an always-empty target search and call the executor complete.
/// The ordinary [`EnvWorld::frame`] supplies no host and leaves AIR_PATROL stationary;
/// [`EnvWorld::frame_with_air_patrol_host`] is the only advancing entrypoint.
pub trait AirPatrolHost {
    /// Validate all host capabilities before the owner-slot scheduler mutates the frame.
    fn preflight(&mut self, world: &EnvWorld) -> Result<(), AirPatrolHostError>;

    /// Animal virtual override at the head of `Unit::do_air_patrol`.
    fn think_bird(
        &mut self,
        world: &mut EnvWorld,
        row: usize,
        order: &mut AirPatrolOrder,
    ) -> Result<(), AirPatrolHostError>;

    /// Complete `Unit::do_air_physics` transaction. The host mutates every live airframe
    /// field it owns in `world`; `false` stops the patrol executor for this frame.
    fn do_air_physics(
        &mut self,
        world: &mut EnvWorld,
        row: usize,
        order: &mut AirPatrolOrder,
        target_x: i32,
        target_y: i32,
    ) -> Result<bool, AirPatrolHostError>;

    /// Retail virtual type query, including upgrade-line membership.
    fn actor_is_type(
        &mut self,
        world: &EnvWorld,
        row: usize,
        type_id: i32,
        strict: bool,
    ) -> Result<bool, AirPatrolHostError>;

    /// Complete mod-16 air/bomber primary search and option-controlled fallback.
    fn find_unit_target(
        &mut self,
        world: &EnvWorld,
        row: usize,
        order: &AirPatrolOrder,
        search_x: i32,
        search_y: i32,
        search: AirPatrolSearch,
    ) -> Result<Option<AirPatrolTarget>, AirPatrolHostError>;

    /// Complete mod-32 building spatial scan and owner-target-bit query.
    fn find_building_target(
        &mut self,
        world: &EnvWorld,
        row: usize,
        order: &AirPatrolOrder,
        search_x: i32,
        search_y: i32,
    ) -> Result<Option<AirPatrolTarget>, AirPatrolHostError>;
}

/// One ordinary Farm order retained outside the compact observation mirror.
///
/// The phase is the exact checksum-visible twenty-byte `GatherOrder` suffix recovered in
/// `don-sim::systems::gathering`. The surrounding order queue remains [`EnvWorld::orders`];
/// this record supplies the Farm-only state that `OrderRec` deliberately does not duplicate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvFarmGatherOrder {
    pub worker_owner: u8,
    pub worker_o: i16,
    pub target_owner: u8,
    pub target_o: i16,
    pub target_uid: u16,
    pub phase: NonFlatGatherState,
    pub first_tick_complete: bool,
}

impl EnvFarmGatherOrder {
    /// The scalar byte image retail's `GatherOrder::walk_data` contributes. Order-list
    /// allocation/capacity metadata belongs to the queue walker and is outside this image.
    pub fn walk(&self) -> GatherOrderWalk {
        GatherOrderWalk {
            // The recovered base byte is independent of the OrderIndex and starts clear in
            // the admitted constructor state. No unproven flag meaning is assigned here.
            order_base_byte: 0,
            ox: i32::from(self.target_o),
            whom: i32::from(self.target_owner),
            uid: self.target_uid,
            tx: self.phase.tx,
            ty: self.phase.ty,
            build_type: self.phase.build_type,
            wait: self.phase.wait,
            goto_build: self.phase.goto_build,
            non_flat_gather: self.phase.non_flat_gather,
            dist_mod: self.phase.dist_mod,
            been_there: self.phase.been_there,
        }
    }
}

/// Persistent owner-local gathering state for one EnvWorld.
///
/// These are identity-keyed rather than row-parallel because retail links sites and workers
/// by `(who,o)`, and EnvWorld compacts rows after a despawn. The vectors retain deterministic
/// insertion order; the intrusive `gather_down` chain remains authoritative for occupancy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvGatherState {
    pub sites: Vec<GatherSite>,
    pub workers: Vec<GatherWorker>,
    pub farm_orders: Vec<EnvFarmGatherOrder>,
}

impl Default for EnvGatherState {
    fn default() -> Self {
        Self {
            sites: Vec::new(),
            workers: Vec::new(),
            farm_orders: Vec::new(),
        }
    }
}

/// Exact adjacent boundaries still required to admit Farm gathering into an EnvWorld.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatherHostBoundary {
    FarmTarget,
    FarmUpdate,
    FarmPerWorkerGross,
    LeaderGatherInputs,
    GatherMove,
}

/// Fail-closed error from the explicit Farm provider path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatherHostError {
    Unavailable(GatherHostBoundary),
    InvalidState(&'static str),
}

#[derive(Debug)]
enum EnvFrameHostError {
    Air(AirPatrolHostError),
    Gather(GatherHostError),
}

/// Host-owned leader inputs for the recovered `Leader::gather` payout pipeline.
///
/// `inputs.object_income` contains every authoritative contribution *except* the Farm sites
/// stored in [`EnvGatherState`]. EnvWorld adds those from active occupancy and the host's
/// fully evaluated per-worker results before calling `economy::leader_gather`.
#[derive(Clone, Debug)]
pub struct GatherLeaderFrame {
    pub inputs: GatherInputs,
    pub cap_gates: CapGates,
    pub payout: DoGatherContext,
}

/// Mandatory authoritative provider for the admitted Farm-only environment seam.
///
/// No method has a default. In particular, the target footprint/update result, per-worker
/// six-resource evaluator and leader-wide tech/game gates cannot be inferred from TypeCap or
/// from a distance to the Farm. The ordinary [`EnvWorld::frame`] supplies no provider and
/// therefore preserves Gather orders without advancing or paying them.
pub trait GatherHost {
    fn preflight(&mut self, world: &EnvWorld) -> Result<(), GatherHostError>;

    fn farm_target(
        &mut self,
        world: &EnvWorld,
        worker_row: usize,
        target_row: usize,
    ) -> Result<OrdinaryGatherTarget, GatherHostError>;

    /// Exact return from `FarmData::update` plus the low-byte game gate read by the admitted
    /// `Unit::do_gather` first-tick branch.
    fn farm_first_tick(
        &mut self,
        world: &EnvWorld,
        order: &EnvFarmGatherOrder,
    ) -> Result<(i32, i32), GatherHostError>;

    /// Fully evaluated six-slot contribution for one active worker at this Farm, before
    /// leader-wide composition/caps. This is the output of the real type/player evaluator,
    /// not permission to substitute a resource-kind heuristic.
    fn farm_per_worker_gross(
        &mut self,
        world: &EnvWorld,
        site: &GatherSite,
    ) -> Result<[i32; g::NUM_COMMON], GatherHostError>;

    fn leader_frame(
        &mut self,
        world: &EnvWorld,
        who: u8,
    ) -> Result<GatherLeaderFrame, GatherHostError>;
}

/// One environment instance.
#[derive(Clone)]
pub struct EnvWorld {
    pub sim: World,
    pub rules: Arc<Rules>,
    pub players: Vec<PlayerState>,
    pub gather: EnvGatherState,
    // ---- entity columns, parallel to `sim` rows -------------------------------------
    pub type_index: Vec<u16>,
    /// Executable `UnitData::orderlist` state for the environment lane. Unlike the
    /// historical `order` byte mirror below, this retains every queued node and the
    /// dynamic waypoint arrays owned by patrol orders.
    pub orders: Vec<OrderQueue>,
    pub order: Vec<u8>,
    pub target: Vec<Handle>,
    pub dest_x: Vec<i32>,
    pub dest_y: Vec<i32>,
    pub stance: Vec<u8>,
    pub form: Vec<u8>,
    pub attack: Vec<i16>,
    pub armor: Vec<i16>,
    pub max_hits: Vec<i32>,
    pub speed: Vec<i16>,
    pub range_max: Vec<i32>,
    /// `UnitData::spell_time`; AIR_PATROL's animal override primes this from 0 to 1.
    pub spell_time: Vec<i16>,
    /// Generation of the handle owning each row. `don-sim` exposes ids but not
    /// generations, and reconstructing one by probing would be O(generation); mirroring
    /// it through the same swap-remove keeps `handle_at` O(1).
    handle_gen: Vec<u32>,
    // ---- episode -------------------------------------------------------------------
    /// Per-agent handle lists captured when the last observation was written. Actions are
    /// resolved against these, never against raw row indices: within one step another
    /// agent's DISBAND or QUEUE_UP compacts the SoA rows, so a row index the policy saw
    /// can name a different entity by the time the action is applied. Handles are exactly
    /// the abstraction `don-sim` provides for that, and using them makes the multi-agent
    /// simultaneous-action semantics well defined instead of order-dependent.
    pub ctrl: Vec<Vec<Handle>>,
    /// Same, for the observed entity list the `TargetEntity` head indexes into.
    pub obs_ents: Vec<Vec<Handle>>,
    pub step_index: u32,
    pub done: bool,
    pub truncated: bool,
    pub unimplemented: Unimplemented,
    rng: u64,
    subtile_w: i32,
    subtile_h: i32,
}

const NO_HANDLE: Handle = Handle {
    id: u32::MAX,
    generation: 0,
};

impl EnvWorld {
    pub fn new(
        rules: Arc<Rules>,
        capacity: usize,
        seed: u64,
        grid_w: usize,
        grid_h: usize,
    ) -> EnvWorld {
        let cap = capacity.min(don_sim::MAX_UNITS);
        EnvWorld {
            sim: World::with_capacity(cap, seed),
            rules,
            players: (0..g::NUM_PLAYERS)
                .map(|i| PlayerState::new(i as u8))
                .collect(),
            gather: EnvGatherState::default(),
            type_index: vec![0; cap],
            orders: vec![OrderQueue::new(); cap],
            order: vec![0; cap],
            target: vec![NO_HANDLE; cap],
            dest_x: vec![0; cap],
            dest_y: vec![0; cap],
            stance: vec![0; cap],
            form: vec![0; cap],
            attack: vec![0; cap],
            armor: vec![0; cap],
            max_hits: vec![1; cap],
            speed: vec![0; cap],
            range_max: vec![0; cap],
            spell_time: vec![0; cap],
            handle_gen: vec![0; cap],
            ctrl: vec![Vec::new(); g::NUM_PLAYERS],
            obs_ents: vec![Vec::new(); g::NUM_PLAYERS],
            step_index: 0,
            done: false,
            truncated: false,
            unimplemented: Unimplemented {
                unit: vec![0; g::N_UNIT_VERBS],
                player: vec![0; g::N_PLAYER_VERBS],
            },
            rng: seed | 1,
            subtile_w: grid_w as i32 * SUBTILE,
            subtile_h: grid_h as i32 * SUBTILE,
        }
    }

    /// Heap payload reserved by this mutable world, excluding the shared [`Rules`] tables
    /// and allocator metadata. The benchmark reports this separately from output buffers
    /// so changing observation shape cannot be mistaken for changing simulation state.
    pub fn bytes_reserved(&self) -> usize {
        fn vec_bytes<T>(v: &Vec<T>) -> usize {
            v.capacity() * std::mem::size_of::<T>()
        }

        let mut bytes = self.sim.bytes_reserved()
            + vec_bytes(&self.players)
            + vec_bytes(&self.gather.sites)
            + vec_bytes(&self.gather.workers)
            + vec_bytes(&self.gather.farm_orders)
            + vec_bytes(&self.type_index)
            + vec_bytes(&self.orders)
            + vec_bytes(&self.order)
            + vec_bytes(&self.target)
            + vec_bytes(&self.dest_x)
            + vec_bytes(&self.dest_y)
            + vec_bytes(&self.stance)
            + vec_bytes(&self.form)
            + vec_bytes(&self.attack)
            + vec_bytes(&self.armor)
            + vec_bytes(&self.max_hits)
            + vec_bytes(&self.speed)
            + vec_bytes(&self.range_max)
            + vec_bytes(&self.spell_time)
            + vec_bytes(&self.handle_gen)
            + vec_bytes(&self.ctrl)
            + vec_bytes(&self.obs_ents)
            + vec_bytes(&self.unimplemented.unit)
            + vec_bytes(&self.unimplemented.player);
        for p in &self.players {
            bytes += vec_bytes(&p.num_buildings) + vec_bytes(&p.num_units);
        }
        for v in self.ctrl.iter().chain(self.obs_ents.iter()) {
            bytes += vec_bytes(v);
        }
        bytes += self
            .orders
            .iter()
            .map(OrderQueue::bytes_reserved)
            .sum::<usize>();
        bytes
    }

    #[inline]
    fn next_rand(&mut self) -> u64 {
        // Deterministic scaffold RNG. The engine's stream is the LCG in `Random::get`
        // (`s <- s*1664525 + 1013904223`); this env does not claim to reproduce it.
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    #[inline]
    pub fn cap(&self, t: u16) -> &TypeCap {
        self.rules.caps.get(t)
    }

    pub fn spawn(&mut self, owner: u8, t: u16, x: i32, y: i32) -> Option<Handle> {
        let h = self.sim.spawn(owner)?;
        let row = self.sim.row_of(h).expect("just spawned");
        let c = *self.rules.caps.get(t);
        self.type_index[row] = t;
        self.orders[row].clear();
        self.order[row] = g::OrderIndex::None as u8;
        self.target[row] = NO_HANDLE;
        self.dest_x[row] = x;
        self.dest_y[row] = y;
        self.stance[row] = 0;
        let initial_form = if matches!(t, 50..=53) { 9 } else { 0 };
        self.form[row] = initial_form;
        self.attack[row] = c.attack;
        self.armor[row] = c.armor;
        self.max_hits[row] = c.hits.max(1);
        self.speed[row] = c.move_rate;
        self.range_max[row] = c.range_max;
        self.spell_time[row] = 0;
        self.handle_gen[row] = h.generation;
        self.sim.set_pos(
            row,
            x.rem_euclid(self.subtile_w),
            y.rem_euclid(self.subtile_h),
        );
        self.sim.hits_mut()[row] = c.hits.max(1);
        self.sim.cooldown_mut()[row] = 0;
        // Unit::init `0x00612100`: the four Citizen type ids start in Mob, every other
        // type in Line; form_mod is the signed -1 non-contributor sentinel, and a newly
        // spawned object belongs to no command group.
        self.sim.units.group_mut()[row] = -1;
        self.sim.units.form_mut()[row] = initial_form as i8;
        self.sim.units.form_mod_mut()[row] = -1;
        self.sim.units.stance_mut()[row] = 0;
        self.sim.units.set_unit_masks(row, 0);
        let p = owner as usize;
        if p < g::NUM_PLAYERS {
            if c.has(F_BUILDING) {
                let i = (t as usize).wrapping_sub(g::BUILD_TYPE_BASE);
                if i < g::NUM_BUILDTYPES {
                    self.players[p].num_buildings[i] += 1;
                }
            } else {
                let i = (t as usize).wrapping_sub(g::UNIT_TYPE_BASE);
                if i < g::NUM_UNITTYPES {
                    self.players[p].num_units[i] += 1;
                }
                self.players[p].pop += c.pop.max(1) as i32;
            }
        }
        Some(h)
    }

    /// Remove an entity, mirroring `don-sim`'s swap-remove into the env columns.
    ///
    /// A worker Gather order or Farm site is retired before the object table compacts.
    /// The retirement runs against a checkpoint: an invalid intrusive chain refuses the
    /// despawn without leaving the order, occupancy, or economy-dirty state half changed.
    pub fn try_despawn(&mut self, h: Handle) -> Result<bool, GatherHostError> {
        let Some(row) = self.sim.row_of(h) else {
            return Ok(false);
        };
        let owner = u8::try_from(self.sim.owner()[row]).ok();
        let object = self.sim.units.o()[row];
        let gather_related = owner.is_some_and(|owner| {
            self.gather.farm_orders.iter().any(|order| {
                (order.worker_owner == owner && order.worker_o == object)
                    || (order.target_owner == owner && order.target_o == object)
            }) || self
                .gather
                .workers
                .iter()
                .any(|worker| worker.owner == owner && worker.unit_o == object)
                || self
                    .gather
                    .sites
                    .iter()
                    .any(|site| site.owner == owner && site.build_o == object)
        });
        let checkpoint = gather_related.then(|| self.clone());
        if gather_related {
            if let Err(error) = self.retire_gather_for_despawn(row) {
                *self = checkpoint.expect("gather-related despawn has a checkpoint");
                return Err(error);
            }
        }

        let last = self.sim.live_count() as usize - 1;
        let t = self.type_index[row];
        let owner = self.sim.owner()[row] as usize;
        if row != last {
            self.type_index[row] = self.type_index[last];
            self.orders.swap(row, last);
            self.order[row] = self.order[last];
            self.target[row] = self.target[last];
            self.dest_x[row] = self.dest_x[last];
            self.dest_y[row] = self.dest_y[last];
            self.stance[row] = self.stance[last];
            self.form[row] = self.form[last];
            self.attack[row] = self.attack[last];
            self.armor[row] = self.armor[last];
            self.max_hits[row] = self.max_hits[last];
            self.speed[row] = self.speed[last];
            self.range_max[row] = self.range_max[last];
            self.spell_time[row] = self.spell_time[last];
            self.handle_gen[row] = self.handle_gen[last];
        }
        self.orders[last].clear();
        let ok = self.sim.despawn(h);
        if ok && owner < g::NUM_PLAYERS {
            let c = *self.rules.caps.get(t);
            let p = &mut self.players[owner];
            if c.has(F_BUILDING) {
                let i = (t as usize).wrapping_sub(g::BUILD_TYPE_BASE);
                if i < g::NUM_BUILDTYPES {
                    p.num_buildings[i] = p.num_buildings[i].saturating_sub(1);
                }
                p.buildings_lost += 1;
            } else {
                let i = (t as usize).wrapping_sub(g::UNIT_TYPE_BASE);
                if i < g::NUM_UNITTYPES {
                    p.num_units[i] = p.num_units[i].saturating_sub(1);
                }
                p.pop = (p.pop - c.pop.max(1) as i32).max(0);
                p.units_lost += 1;
            }
        }
        Ok(ok)
    }

    /// Compatibility form for callers that cannot surface an invariant error. A failed
    /// gather retirement is observable as a refused despawn, with the world restored.
    pub fn despawn(&mut self, h: Handle) -> bool {
        self.try_despawn(h).unwrap_or(false)
    }

    /// `ObjectData::get_damage` `0x00644130` driven from env state.
    ///
    /// The predicate inputs the chain reads by walking the object graph are **not**
    /// derivable from this state and are left at `Default` (all false). That means the
    /// spine of the chain runs — balance term, ×10 attack, mid-chain armor subtraction,
    /// the conditional floor of 1 — and the branch-guarded terms (overkill, flank,
    /// entrenchment, height, river, recapture) do not. Damage here is therefore the
    /// derived formula on a reduced predicate set, not retail-equal.
    pub fn resolve_damage(&self, atk_row: usize, def_row: usize) -> i32 {
        let at = self.type_index[atk_row];
        let dt = self.type_index[def_row];
        let ac = self.rules.caps.get(at);
        let dc = self.rules.caps.get(dt);
        let input = don_sim::DamageInput {
            balance_pct: self.rules.balance_pct(at, dt),
            attack: (self.attack[atk_row] as i32) * 10,
            armor: self.armor[def_row] as i32,
            attacker_type_id: at as i32,
            defender_type_id: dt as i32,
            attacker_domain: ac.domain as i32,
            defender_domain: dc.domain as i32,
            attacker_splash_percent: 100,
            attacker_player: self.sim.owner()[atk_row] as u32,
            current_frame: self.step_index as i32,
            ..Default::default()
        };
        don_sim::damage(
            &input,
            &don_sim::DamagePredicates::default(),
            &don_sim::CombatRules::default(),
            &don_sim::UnreachedTerms::default(),
        )
    }

    #[inline]
    pub fn tile_of(&self, row: usize) -> (i32, i32) {
        (
            self.sim.pos_x()[row] / SUBTILE,
            self.sim.pos_y()[row] / SUBTILE,
        )
    }

    #[inline]
    pub fn relation(&self, a: u8, b: i8) -> u8 {
        if b >= 0 && a == b as u8 {
            return 0;
        }
        let (a, b) = (a as usize, usize::try_from(b).unwrap_or(usize::MAX));
        if a >= g::NUM_PLAYERS || b >= g::NUM_PLAYERS {
            return 3;
        }
        match self.players[a].diplos[b] {
            2 => 1, // ALLY
            0 => 2, // WAR
            _ => 3, // PEACE / neutral
        }
    }

    /// Make the executable queue authoritative for a row which predates queue-aware
    /// command installation. This compatibility import is intentionally one-way: after
    /// the first queued command, [`Self::sync_order_from_queue`] owns the byte mirror.
    fn import_legacy_order(&mut self, row: usize) {
        if !self.orders[row].is_empty() || self.order[row] == OrderIndex::None as u8 {
            return;
        }
        let Some(kind) = OrderIndex::from_index(self.order[row] as usize) else {
            return;
        };
        let mut rec = match kind {
            OrderIndex::MoveTo | OrderIndex::AttackTo => {
                OrderRec::move_to(self.dest_x[row], self.dest_y[row], 0)
            }
            _ => OrderRec::of_kind(kind),
        };
        rec.kind = kind;
        self.orders[row].push_back(rec);
    }

    /// Retire the persistent Gather state for one owner-local worker identity.
    ///
    /// `kill_current_order`'s Gather epilogue deliberately ignores the captured target UID
    /// after resolving the raw `(whom,ox)` target. When that object has vanished, retail
    /// leaves a stale chain entry for `Build::check_gatherers`; this compact host runs that
    /// exact pruning primitive immediately so a later object-slot reuse cannot cross-link
    /// the worker into two sites.
    fn retire_farm_gather_inner(
        &mut self,
        worker_owner: u8,
        worker_o: i16,
    ) -> Result<bool, GatherHostError> {
        let Some(order_pos) = self
            .gather
            .farm_orders
            .iter()
            .position(|order| order.worker_owner == worker_owner && order.worker_o == worker_o)
        else {
            return Ok(false);
        };
        let order = self.gather.farm_orders[order_pos];
        let target_live = (0..self.sim.live_count() as usize).any(|row| {
            self.sim.owner()[row] == order.target_owner as i8
                && self.sim.units.o()[row] == order.target_o
        });
        let site_pos =
            self.gather.sites.iter().position(|site| {
                site.owner == order.target_owner && site.build_o == order.target_o
            });
        if target_live && site_pos.is_none() {
            return Err(GatherHostError::InvalidState(
                "live Farm Gather target has no persistent site",
            ));
        }

        let retirement = if target_live {
            let site = &mut self.gather.sites[site_pos.expect("checked above")];
            retire_gather_order(
                Some(site),
                &mut self.gather.workers,
                worker_owner,
                worker_o,
                None,
            )
        } else {
            retire_gather_order(None, &mut self.gather.workers, worker_owner, worker_o, None)
        }
        .map_err(|_| GatherHostError::InvalidState("Farm Gather retirement chain is invalid"))?;

        if !target_live {
            if let Some(site_pos) = site_pos {
                check_gatherers(&mut self.gather.sites[site_pos], &mut self.gather.workers)
                    .map_err(|_| {
                        GatherHostError::InvalidState("stale Farm Gather chain cannot be pruned")
                    })?;
            }
        }
        self.gather.farm_orders.remove(order_pos);
        if retirement.leader_economy_dirty {
            self.players[worker_owner as usize].gather_dirty = true;
        }
        Ok(true)
    }

    /// Checkpointed form used by HALT and unshifted order replacement.
    fn retire_farm_gather_for_row(&mut self, row: usize) -> Result<bool, GatherHostError> {
        let worker_owner = u8::try_from(self.sim.owner()[row])
            .map_err(|_| GatherHostError::InvalidState("Gather worker lost its owner"))?;
        let worker_o = self.sim.units.o()[row];
        if !self
            .gather
            .farm_orders
            .iter()
            .any(|order| order.worker_owner == worker_owner && order.worker_o == worker_o)
        {
            return Ok(false);
        }
        let gather_checkpoint = self.gather.clone();
        let dirty_checkpoint = self.players[worker_owner as usize].gather_dirty;
        match self.retire_farm_gather_inner(worker_owner, worker_o) {
            Ok(retired) => Ok(retired),
            Err(error) => {
                self.gather = gather_checkpoint;
                self.players[worker_owner as usize].gather_dirty = dirty_checkpoint;
                Err(error)
            }
        }
    }

    fn install_order_without_retirement(&mut self, row: usize, rec: OrderRec, queue: QueuePos) {
        self.import_legacy_order(row);
        match queue {
            QueuePos::New => self.orders[row].replace(rec),
            QueuePos::Last => self.orders[row].push_back(rec),
            QueuePos::First => self.orders[row].push_front(rec),
        }
        self.sync_order_from_queue(row);
    }

    /// Install an ordinary order with the three queue positions carried on the wire.
    /// Patrol uses its two exceptional installers below instead.
    ///
    /// An unshifted command first executes the recovered Gather retirement epilogue. A
    /// FRONT/BACK insertion leaves the Gather node in the queue and is therefore refused
    /// while this Farm-only provider owns it: the env has not derived retail's suspended
    /// Gather occupancy behavior and must not keep paying a preempted worker by guess.
    pub fn install_order(
        &mut self,
        row: usize,
        rec: OrderRec,
        queue: QueuePos,
    ) -> Result<(), GatherHostError> {
        let has_gather = self.retire_farm_gather_for_row_if_new(row, queue)?;
        if has_gather && queue != QueuePos::New {
            return Err(GatherHostError::InvalidState(
                "queued order insertion around a live Farm Gather is not admitted",
            ));
        }
        self.install_order_without_retirement(row, rec, queue);
        Ok(())
    }

    fn retire_farm_gather_for_row_if_new(
        &mut self,
        row: usize,
        queue: QueuePos,
    ) -> Result<bool, GatherHostError> {
        let owner = u8::try_from(self.sim.owner()[row])
            .map_err(|_| GatherHostError::InvalidState("order actor lost its owner"))?;
        let object = self.sim.units.o()[row];
        let has_gather = self
            .gather
            .farm_orders
            .iter()
            .any(|order| order.worker_owner == owner && order.worker_o == object);
        if has_gather && queue == QueuePos::New {
            self.retire_farm_gather_for_row(row)?;
        }
        Ok(has_gather)
    }

    fn gather_collision_row(&self, row: usize) -> UnitRow {
        UnitRow {
            who: i32::from(self.sim.owner()[row]),
            o: i32::from(self.sim.units.o()[row]),
            x: self.sim.pos_x()[row],
            y: self.sim.pos_y()[row],
            domain: i32::from(self.cap(self.type_index[row]).domain),
            // Every EnvWorld entity is currently on-map. No containment transition exists
            // in this environment, so this is state, not a fallback assumption.
            on_map: true,
            active: self.sim.hits()[row] > 0,
            ..UnitRow::default()
        }
    }

    /// Admit one exact ordinary Farm Gather constructor transaction.
    ///
    /// Capacity 1 is the recovered flat Farm arm of `BuildTypeData::calc_gather`; no terrain
    /// or type-table estimate is used. This function installs only QUEUE_NEW because queued
    /// attachment/retirement interaction has not yet been integrated into EnvWorld.
    pub fn install_farm_gather(
        &mut self,
        worker_row: usize,
        target_row: usize,
        target: OrdinaryGatherTarget,
        queue: QueuePos,
    ) -> Result<AttachResult, GatherHostError> {
        let checkpoint = self.clone();
        match self.install_farm_gather_inner(worker_row, target_row, target, queue) {
            Ok(status) => Ok(status),
            Err(error) => {
                *self = checkpoint;
                Err(error)
            }
        }
    }

    fn install_farm_gather_inner(
        &mut self,
        worker_row: usize,
        target_row: usize,
        target: OrdinaryGatherTarget,
        queue: QueuePos,
    ) -> Result<AttachResult, GatherHostError> {
        if queue != QueuePos::New {
            return Err(GatherHostError::InvalidState(
                "Farm Gather provider currently admits QUEUE_NEW only",
            ));
        }
        if target.kind != OrdinaryGatherKind::Farm {
            return Err(GatherHostError::InvalidState(
                "ordinary environment provider currently admits Farm only",
            ));
        }
        if self.type_index[target_row] as i32 != don_sim::systems::tech_cities::ty::FARM {
            return Err(GatherHostError::InvalidState(
                "authoritative Farm target does not name TypeIndex 417",
            ));
        }
        let worker_type = self.type_index[worker_row] as i32;
        if !matches!(worker_type, 0x32 | 0x33) {
            return Err(GatherHostError::InvalidState(
                "ordinary Farm gatherer must be Citizen TypeIndex 0x32 or 0x33",
            ));
        }
        let worker_owner = u8::try_from(self.sim.owner()[worker_row])
            .map_err(|_| GatherHostError::InvalidState("Farm gatherer has no player owner"))?;
        let target_owner = u8::try_from(self.sim.owner()[target_row])
            .map_err(|_| GatherHostError::InvalidState("Farm target has no player owner"))?;
        if worker_owner != target_owner {
            return Err(GatherHostError::InvalidState(
                "ordinary Farm worker and site must share the owner-local object table",
            ));
        }
        if worker_owner as usize >= g::NUM_PLAYERS {
            return Err(GatherHostError::InvalidState(
                "ordinary Farm Gather owner is outside the leader table",
            ));
        }
        if i32::from(self.cap(self.type_index[worker_row]).domain) != DOMAIN_LAND {
            return Err(GatherHostError::InvalidState(
                "ordinary Farm gatherer is not land-domain",
            ));
        }

        let worker_o = self.sim.units.o()[worker_row];
        let target_o = self.sim.units.o()[target_row];
        let target_uid = self.sim.units.uid()[target_row] as u16;
        // QUEUE_NEW kills the current order before constructing its replacement. This is
        // the recovered Gather retirement epilogue, not an occupancy shortcut.
        self.retire_farm_gather_for_row(worker_row)?;
        let site_pos = if let Some(pos) = self
            .gather
            .sites
            .iter()
            .position(|site| site.owner == target_owner && site.build_o == target_o)
        {
            pos
        } else {
            let mut site = GatherSite::new(target_owner, target_o);
            site.uid = target_uid;
            // Farm's exact flat evaluator returns one; see docs/mechanics/gathering.md.
            site.set_authoritative_capacity(1);
            self.gather.sites.push(site);
            self.gather.sites.len() - 1
        };
        if self.gather.sites[site_pos].uid != target_uid {
            return Err(GatherHostError::InvalidState(
                "Farm object slot was reused after the site state was created",
            ));
        }

        let worker_pos = if let Some(pos) = self
            .gather
            .workers
            .iter()
            .position(|worker| worker.owner == worker_owner && worker.unit_o == worker_o)
        {
            pos
        } else {
            self.gather
                .workers
                .push(GatherWorker::new(worker_owner, worker_o, worker_type));
            self.gather.workers.len() - 1
        };
        self.gather.workers[worker_pos].type_index = worker_type;
        self.gather.workers[worker_pos].valid_unit = true;
        self.gather.workers[worker_pos].assignment = Some(GatherAssignment {
            target_owner: i32::from(target_owner),
            target_build: i32::from(target_o),
            target_uid,
            been_there: false,
            inside_target: None,
        });

        let mut collision = UnitTable::default();
        collision.rows.push(self.gather_collision_row(worker_row));
        let unit_type = NearbyUnitType {
            type_index: worker_type,
            domain: DOMAIN_LAND,
            // The Farm attachment-only branch reads neither radius nor unit flags.
            big_radius: 0,
            block_radius: 0,
            unit_flags: 0,
        };
        let attachment = {
            let gather = &mut self.gather;
            ensure_ordinary_attachment(
                &collision,
                &mut gather.sites[site_pos],
                &mut gather.workers,
                worker_o,
                target,
                unit_type,
            )
            .map_err(|_| {
                GatherHostError::InvalidState("Farm attachment lifecycle rejected provider state")
            })?
        };
        if attachment.status == AttachResult::Full {
            self.gather.workers[worker_pos].assignment = None;
            return Err(GatherHostError::InvalidState(
                "Farm's single authoritative gather slot is occupied",
            ));
        }

        self.install_order_without_retirement(
            worker_row,
            OrderRec::gather(i32::from(target_owner), i32::from(target_o), target_uid),
            QueuePos::New,
        );
        self.gather
            .farm_orders
            .retain(|order| order.worker_owner != worker_owner || order.worker_o != worker_o);
        self.gather.farm_orders.push(EnvFarmGatherOrder {
            worker_owner,
            worker_o,
            target_owner,
            target_o,
            target_uid,
            phase: attached_ordinary_order_state(OrdinaryGatherKind::Farm),
            first_tick_complete: false,
        });
        self.players[worker_owner as usize].gather_dirty = true;
        Ok(attachment.status)
    }

    pub fn farm_gather_order(
        &self,
        worker_owner: u8,
        worker_o: i16,
    ) -> Option<&EnvFarmGatherOrder> {
        self.gather
            .farm_orders
            .iter()
            .find(|order| order.worker_owner == worker_owner && order.worker_o == worker_o)
    }

    /// Clear `UnitData::orderlist`, including `kill_current_order`'s Gather epilogue.
    pub fn clear_orders(&mut self, row: usize) -> Result<(), GatherHostError> {
        self.retire_farm_gather_for_row(row)?;
        self.orders[row].clear();
        self.sync_order_from_queue(row);
        Ok(())
    }

    /// `Group::action_patrol` + `Unit::add_patrol_order` for one environment actor.
    ///
    /// Env actions are per actor rather than persistent `Group` objects, so the unit is
    /// deliberately executed ungrouped. The recovered ground executor does not read
    /// `id`/`form_id` in that arm; `(whose, oxx)` still carries the real object address.
    pub fn install_group_patrol_order(
        &mut self,
        row: usize,
        target_x: i32,
        target_y: i32,
        queue: QueuePos,
    ) -> Result<PatrolInstall, GatherHostError> {
        let has_gather = self.retire_farm_gather_for_row_if_new(row, queue)?;
        if has_gather && queue != QueuePos::New {
            return Err(GatherHostError::InvalidState(
                "queued patrol insertion around a live Farm Gather is not admitted",
            ));
        }
        self.import_legacy_order(row);
        let who = self.sim.owner()[row] as u8;
        let o = self.sim.units.o()[row];
        let (x, y) = (self.sim.pos_x()[row], self.sim.pos_y()[row]);
        let mut unit = UnitWork::at(who, o, x, y);
        unit.orders = std::mem::take(&mut self.orders[row]);
        let result = install_group_patrol(
            &mut unit,
            x,
            y,
            target_x,
            target_y,
            0,
            self.form[row] as i32,
            o as i32,
            who as i32,
            queue,
        );
        self.orders[row] = unit.orders;
        self.dest_x[row] = target_x;
        self.dest_y[row] = target_y;
        self.sync_order_from_queue(row);
        Ok(result)
    }

    /// `Group::action_air_patrol` + the true-plane replacement/extension installer.
    pub fn install_air_patrol_order(
        &mut self,
        row: usize,
        target_x: i32,
        target_y: i32,
        queue: QueuePos,
    ) -> Result<PatrolInstall, GatherHostError> {
        let has_gather = self.retire_farm_gather_for_row_if_new(row, queue)?;
        if has_gather && queue != QueuePos::New {
            return Err(GatherHostError::InvalidState(
                "queued patrol insertion around a live Farm Gather is not admitted",
            ));
        }
        self.import_legacy_order(row);
        let who = self.sim.owner()[row] as u8;
        let o = self.sim.units.o()[row];
        let (x, y) = (self.sim.pos_x()[row], self.sim.pos_y()[row]);
        let mut unit = UnitWork::at(who, o, x, y);
        unit.orders = std::mem::take(&mut self.orders[row]);
        // EnvWorld currently has no launch-home/garrison column. This is the exact
        // no-live-home branch of add_air_patrol_order, not an invented origin.
        let result = install_air_patrol(&mut unit, target_x, target_y, -1, -1, None, true, queue);
        self.orders[row] = unit.orders;
        self.dest_x[row] = target_x;
        self.dest_y[row] = target_y;
        self.sync_order_from_queue(row);
        Ok(result)
    }

    /// Refresh the compact observation/action mirror from the executable list head.
    fn sync_order_from_queue(&mut self, row: usize) {
        self.orders[row].reset();
        let Some(front) = self.orders[row].front() else {
            self.order[row] = OrderIndex::None as u8;
            return;
        };
        let (kind, x, y, target_who, target_o, target_uid) = (
            front.kind,
            front.x,
            front.y,
            front.target_who,
            front.target_o,
            front.target_uid,
        );
        self.order[row] = kind as u8;
        if matches!(
            kind,
            OrderIndex::MoveTo
                | OrderIndex::AttackTo
                | OrderIndex::GroupMove
                | OrderIndex::GroupAttackTo
        ) {
            self.dest_x[row] = x;
            self.dest_y[row] = y;
        } else if matches!(kind, OrderIndex::Attack | OrderIndex::GroupAttack) {
            let target_row = (0..self.sim.live_count() as usize).find(|&candidate| {
                self.sim.owner()[candidate] as i32 == target_who
                    && self.sim.units.o()[candidate] as i32 == target_o
                    && self.sim.units.uid()[candidate] as u16 == target_uid
            });
            self.target[row] = target_row.map_or(NO_HANDLE, |target| self.handle_at(target));
        }
    }

    fn retire_front_order(&mut self, row: usize) {
        self.orders[row].reset();
        self.orders[row].remove_current();
        self.orders[row].reset();
        self.sync_order_from_queue(row);
    }

    fn remove_farm_order_node(&mut self, row: usize, order: EnvFarmGatherOrder) {
        let retained: Vec<OrderRec> = self.orders[row]
            .iter()
            .filter(|record| {
                !(record.kind == OrderIndex::Gather
                    && record.target_who == i32::from(order.target_owner)
                    && record.target_o == i32::from(order.target_o)
                    && record.target_uid == order.target_uid)
            })
            .cloned()
            .collect();
        self.orders[row].clear();
        for record in retained {
            self.orders[row].push_back(record);
        }
        self.sync_order_from_queue(row);
    }

    /// Retire every gather relationship owned by an entity while its owner-local object
    /// identity is still resolvable. This runs before `World::despawn` compacts the rows.
    fn retire_gather_for_despawn(&mut self, row: usize) -> Result<(), GatherHostError> {
        let owner = u8::try_from(self.sim.owner()[row])
            .map_err(|_| GatherHostError::InvalidState("despawned gather object lost owner"))?;
        let object = self.sim.units.o()[row];

        if let Some(order) = self
            .gather
            .farm_orders
            .iter()
            .copied()
            .find(|order| order.worker_owner == owner && order.worker_o == object)
        {
            self.retire_farm_gather_inner(owner, object)?;
            self.remove_farm_order_node(row, order);
        }

        let targeting: Vec<EnvFarmGatherOrder> = self
            .gather
            .farm_orders
            .iter()
            .copied()
            .filter(|order| order.target_owner == owner && order.target_o == object)
            .collect();
        for order in targeting {
            self.retire_farm_gather_inner(order.worker_owner, order.worker_o)?;
            if let Some(worker_row) = (0..self.sim.live_count() as usize).find(|&candidate| {
                self.sim.owner()[candidate] == order.worker_owner as i8
                    && self.sim.units.o()[candidate] == order.worker_o
            }) {
                self.remove_farm_order_node(worker_row, order);
            }
        }

        if let Some(site_pos) = self
            .gather
            .sites
            .iter()
            .position(|site| site.owner == owner && site.build_o == object)
        {
            check_gatherers(&mut self.gather.sites[site_pos], &mut self.gather.workers)
                .map_err(|_| GatherHostError::InvalidState("despawned Farm chain is invalid"))?;
            if self.gather.sites[site_pos].gather_down != NO_OBJECT {
                return Err(GatherHostError::InvalidState(
                    "despawned Farm still has an unretired gatherer",
                ));
            }
            self.gather.sites.remove(site_pos);
        }

        if let Some(worker_pos) = self
            .gather
            .workers
            .iter()
            .position(|worker| worker.owner == owner && worker.unit_o == object)
        {
            self.gather.workers[worker_pos].valid_unit = false;
            self.gather.workers[worker_pos].assignment = None;
            for site in &mut self.gather.sites {
                check_gatherers(site, &mut self.gather.workers).map_err(|_| {
                    GatherHostError::InvalidState("despawned gather worker cannot be unlinked")
                })?;
            }
            if self.gather.workers[worker_pos].gather_down != NO_OBJECT {
                return Err(GatherHostError::InvalidState(
                    "despawned gather worker retains an intrusive successor",
                ));
            }
            self.gather.workers.remove(worker_pos);
        }
        Ok(())
    }

    // ---- per-frame systems -----------------------------------------------------------

    /// Advance one simulation frame.
    ///
    /// Owner slots are visited in the engine's rotated order: `Objects::process_all`
    /// starts at `frame % 10` and walks `(frame + i) % 10` [measured]. A fixed-order
    /// scheduler diverges inside one tick, so the rotation is reproduced here even though
    /// the per-object work below is scaffolding.
    pub fn frame(&mut self) {
        self.frame_inner(None, None)
            .expect("a frame without an explicit host cannot call one");
    }

    /// Advance one frame with every AIR_PATROL host transaction explicit. Preflight runs
    /// before the scheduler, so a known missing boundary cannot partially mutate a frame.
    pub fn frame_with_air_patrol_host(
        &mut self,
        host: &mut dyn AirPatrolHost,
    ) -> Result<(), AirPatrolHostError> {
        host.preflight(self)?;
        let checkpoint = self.clone();
        match self.frame_inner(Some(host), None) {
            Ok(()) => Ok(()),
            Err(error) => {
                *self = checkpoint;
                Err(match error {
                    EnvFrameHostError::Air(error) => error,
                    EnvFrameHostError::Gather(_) => {
                        unreachable!("no Gather host was supplied to the air-only frame")
                    }
                })
            }
        }
    }

    /// Advance one frame with the Farm gathering boundaries explicit. Preflight runs before
    /// owner processing, so a known missing target/evaluator/payout provider cannot partially
    /// mutate the admitted transaction.
    pub fn frame_with_gather_host(
        &mut self,
        host: &mut dyn GatherHost,
    ) -> Result<(), GatherHostError> {
        host.preflight(self)?;
        let checkpoint = self.clone();
        match self.frame_inner(None, Some(host)) {
            Ok(()) => Ok(()),
            Err(error) => {
                *self = checkpoint;
                Err(match error {
                    EnvFrameHostError::Gather(error) => error,
                    EnvFrameHostError::Air(_) => {
                        unreachable!("no air host was supplied to the Gather-only frame")
                    }
                })
            }
        }
    }

    fn frame_inner(
        &mut self,
        mut air_host: Option<&mut dyn AirPatrolHost>,
        mut gather_host: Option<&mut dyn GatherHost>,
    ) -> Result<(), EnvFrameHostError> {
        let f = self.sim.frame as usize;
        for i in 0..g::NUM_OWNER_SLOTS {
            let slot = ((f + i) % g::NUM_OWNER_SLOTS) as u8;
            self.process_slot(slot, &mut air_host, &mut gather_host)?;
        }
        if let Some(host) = gather_host.as_deref_mut() {
            self.advance_gather_payout(host)
                .map_err(EnvFrameHostError::Gather)?;
        }
        don_sim::simd::tick_down(self.sim.cooldown_mut());
        self.sim.frame += 1;
        self.reap();
        self.recompute_scores();
        Ok(())
    }

    fn process_slot(
        &mut self,
        slot: u8,
        air_host: &mut Option<&mut dyn AirPatrolHost>,
        gather_host: &mut Option<&mut dyn GatherHost>,
    ) -> Result<(), EnvFrameHostError> {
        let n = self.sim.live_count() as usize;
        for row in 0..n {
            if self.sim.owner()[row] != slot as i8 {
                continue;
            }
            match self.order[row] {
                x if x == g::OrderIndex::MoveTo as u8 || x == g::OrderIndex::AttackTo as u8 => {
                    self.advance_move(row, true);
                }
                x if x == g::OrderIndex::GroupMove as u8 => self.advance_group_move(row),
                x if x == g::OrderIndex::GroupAttack as u8 => self.advance_group_attack(row),
                x if x == g::OrderIndex::GroupAttackTo as u8 => self.advance_group_attack_to(row),
                x if x == g::OrderIndex::Attack as u8 => self.advance_attack(row),
                x if x == g::OrderIndex::Gather as u8 => {
                    if let Some(host) = gather_host.as_deref_mut() {
                        self.advance_farm_gather(row, host)
                            .map_err(EnvFrameHostError::Gather)?;
                    } else {
                        self.unimplemented.unit[g::uv::GATHER] += 1;
                    }
                }
                x if x == g::OrderIndex::GroupPatrol as u8 => self.advance_group_patrol(row),
                x if x == g::OrderIndex::AirPatrol as u8 => {
                    if let Some(host) = air_host.as_deref_mut() {
                        self.advance_air_patrol(row, host)
                            .map_err(EnvFrameHostError::Air)?;
                    } else {
                        // Fail closed: retain the exact order body without crossing the
                        // environment's explicitly approximate straight-line mover.
                        self.unimplemented.unit[g::uv::PATROL] += 1;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn advance_farm_gather(
        &mut self,
        row: usize,
        host: &mut dyn GatherHost,
    ) -> Result<(), GatherHostError> {
        let worker_owner = u8::try_from(self.sim.owner()[row])
            .map_err(|_| GatherHostError::InvalidState("Farm gatherer lost its owner"))?;
        let worker_o = self.sim.units.o()[row];
        let Some(order_pos) = self
            .gather
            .farm_orders
            .iter()
            .position(|order| order.worker_owner == worker_owner && order.worker_o == worker_o)
        else {
            return Err(GatherHostError::InvalidState(
                "Gather order queue has no persistent Farm phase",
            ));
        };
        let order = self.gather.farm_orders[order_pos];
        if order.first_tick_complete {
            return Ok(());
        }
        let target_row = (0..self.sim.live_count() as usize).find(|&candidate| {
            self.sim.owner()[candidate] == order.target_owner as i8
                && self.sim.units.o()[candidate] == order.target_o
                && self.sim.units.uid()[candidate] as u16 == order.target_uid
        });
        let Some(target_row) = target_row else {
            return Err(GatherHostError::InvalidState(
                "Farm Gather target failed its generational identity check",
            ));
        };
        let target = host.farm_target(self, row, target_row)?;
        let (farm_update_result, game_gate_value) = host.farm_first_tick(self, &order)?;
        let site_pos = self
            .gather
            .sites
            .iter()
            .position(|site| {
                site.owner == order.target_owner
                    && site.build_o == order.target_o
                    && site.uid == order.target_uid
            })
            .ok_or(GatherHostError::InvalidState(
                "Farm Gather order has no persistent site",
            ))?;

        let mut collision = UnitTable::default();
        collision.rows.push(self.gather_collision_row(row));
        let unit_type = NearbyUnitType {
            type_index: self.type_index[row] as i32,
            domain: DOMAIN_LAND,
            big_radius: 0,
            block_radius: 0,
            unit_flags: 0,
        };
        let mut phase = order.phase;
        let outcome = {
            let gather = &mut self.gather;
            farm_first_gather_tick::<_, std::convert::Infallible, std::convert::Infallible>(
                &collision,
                &mut gather.sites[site_pos],
                &mut gather.workers,
                worker_o,
                &mut phase,
                target,
                unit_type,
                farm_update_result,
                game_gate_value,
                &mut self.sim.random,
            )
            .map_err(|_| {
                GatherHostError::InvalidState("Farm first-tick lifecycle rejected provider state")
            })?
        };
        self.gather.farm_orders[order_pos].phase = phase;
        match outcome.disposition {
            FarmFirstTickDisposition::Active {
                move_order: None, ..
            } => {
                self.gather.farm_orders[order_pos].first_tick_complete = true;
            }
            FarmFirstTickDisposition::Active {
                move_order: Some(_),
                ..
            } => {
                // The exact plan and RNG/been-there mutations above are retained, but the
                // ordinary EnvWorld mover is an approximation. Do not route a Gather move
                // through it and pretend the provider boundary was complete.
                return Err(GatherHostError::Unavailable(GatherHostBoundary::GatherMove));
            }
            FarmFirstTickDisposition::RetiredAtCapacity(_) => {
                return Err(GatherHostError::InvalidState(
                    "Farm became full after its constructor attachment",
                ));
            }
        }
        if outcome.leader_economy_dirty {
            self.players[worker_owner as usize].gather_dirty = true;
        }
        Ok(())
    }

    fn advance_gather_payout(&mut self, host: &mut dyn GatherHost) -> Result<(), GatherHostError> {
        let mut farm_income = [[0i32; g::NUM_COMMON]; g::NUM_PLAYERS];
        let mut owners = [false; g::NUM_PLAYERS];
        for site in self.gather.sites.clone() {
            let active = num_gatherers(&site, &self.gather.workers, GatherCount::Active, 0)
                .map_err(|_| GatherHostError::InvalidState("Farm gather chain is invalid"))?;
            if active <= 0 {
                continue;
            }
            let per_worker = host.farm_per_worker_gross(self, &site)?;
            let gross = site_gross(per_worker, active, site.gather_max);
            let owner = site.owner as usize;
            if owner >= g::NUM_PLAYERS {
                return Err(GatherHostError::InvalidState(
                    "Farm site owner is outside the leader table",
                ));
            }
            owners[owner] = true;
            for resource in 0..g::NUM_COMMON {
                farm_income[owner][resource] =
                    farm_income[owner][resource].wrapping_add(gross[resource]);
            }
        }

        for who in 0..g::NUM_PLAYERS {
            if !owners[who] {
                continue;
            }
            let mut frame = host.leader_frame(self, who as u8)?;
            for resource in 0..g::NUM_COMMON {
                frame.inputs.object_income[resource] =
                    frame.inputs.object_income[resource].wrapping_add(farm_income[who][resource]);
            }
            let player = &mut self.players[who];
            // Compact public stockpile actions (tribute/build/queue) remain authoritative
            // for EnvWorld; synchronize them into the recovered block before payout.
            player.leader_econ.stockpile = player.econ;
            let payout = economy::leader_gather(
                &EconRules::shipped(),
                &mut player.leader_econ,
                self.sim.frame,
                who as i32,
                &mut player.gather_last_calc_frame,
                &mut player.gather_dirty,
                &frame.inputs,
                &frame.cap_gates,
                &frame.payout,
            );
            player.econ = player.leader_econ.stockpile;
            player.base_rate = player.leader_econ.displayed;
            for resource in 0..g::NUM_COMMON {
                if payout[resource].whole > 0 {
                    player.collected[resource] =
                        player.collected[resource].wrapping_add(payout[resource].whole);
                }
            }
        }
        Ok(())
    }

    /// `Unit::do_patrol` for the ungrouped environment actor. The executor advances the
    /// route cursor before reading it and inserts an exact ATTACK_TO node at the head;
    /// the patrol node remains behind it and resumes when that movement leg retires.
    fn advance_group_patrol(&mut self, row: usize) {
        let who = self.sim.owner()[row] as u8;
        let o = self.sim.units.o()[row];
        let (x, y) = (self.sim.pos_x()[row], self.sim.pos_y()[row]);
        let step = {
            self.orders[row].reset();
            let Some(front) = self.orders[row].front_mut() else {
                self.order[row] = OrderIndex::None as u8;
                return;
            };
            let PatrolPayload::Group(order) = &mut front.patrol_payload else {
                // A kind without its concrete class body cannot be executed faithfully.
                self.unimplemented.unit[g::uv::PATROL] += 1;
                return;
            };
            patrol::step_group_patrol(order, x, y, o, who, -1, -1, false)
        };

        if let GroundPatrolAction::InsertAttackTo(m) = step.action {
            self.orders[row].push_front(OrderRec {
                kind: OrderIndex::AttackTo,
                flags: 0,
                x: m.x,
                y: m.y,
                angle: m.angle,
                dest: m.dest,
                tolerance: m.tolerance,
                pause: m.pause,
                retry: m.retry,
                attempts: m.attempts,
                timer: m.timer,
                facing: m.facing,
                dest_x: m.dest_x,
                dest_y: m.dest_y,
                last_x: m.last_x,
                last_y: m.last_y,
                off_x: m.off_x,
                off_y: m.off_y,
                ..OrderRec::default()
            });
        }
        self.sync_order_from_queue(row);
    }

    /// Execute the derived AIR_PATROL transition only after a mandatory exact host has
    /// supplied the adjacent physics, type and target-search transactions.
    fn advance_air_patrol(
        &mut self,
        row: usize,
        host: &mut dyn AirPatrolHost,
    ) -> Result<(), AirPatrolHostError> {
        self.orders[row].reset();
        let Some(front) = self.orders[row].front() else {
            self.order[row] = OrderIndex::None as u8;
            return Ok(());
        };
        let PatrolPayload::Air(mut air) = front.patrol_payload.clone() else {
            self.unimplemented.unit[g::uv::PATROL] += 1;
            return Ok(());
        };

        host.think_bird(self, row, &mut air)?;
        let list_len = self.orders[row].len();
        let is_animal = (don_sim::balance_path::ANIMAL_FIRST..=don_sim::balance_path::ANIMAL_LAST)
            .contains(&(self.type_index[row] as i32));
        let home = if air.air.oxx >= 0 && air.air.whose >= 0 {
            (0..self.sim.live_count() as usize)
                .find(|&candidate| {
                    self.sim.owner()[candidate] as i32 == air.air.whose
                        && self.sim.units.o()[candidate] as i32 == air.air.oxx
                })
                .map(|candidate| (self.sim.pos_x()[candidate], self.sim.pos_y()[candidate]))
        } else {
            None
        };
        let target =
            patrol::air_patrol_target(&mut air, is_animal, home, self.subtile_w, self.subtile_h);
        if !host.do_air_physics(self, row, &mut air, target.0, target.1)? {
            if let Some(front) = self.orders[row].front_mut() {
                front.patrol_payload = PatrolPayload::Air(air);
            }
            self.sync_order_from_queue(row);
            return Ok(());
        }

        let phase = (self.sim.units.o()[row] as i32).wrapping_add(self.sim.frame);
        let fighter_bomber = host.actor_is_type(self, row, 0x134, false)?;
        let relative_scan_point = |point: (i32, i32)| {
            if fighter_bomber {
                if let Some((hx, hy)) = home {
                    return (
                        point
                            .0
                            .wrapping_add(hx)
                            .clamp(0, self.subtile_w.saturating_sub(1)),
                        point
                            .1
                            .wrapping_add(hy)
                            .clamp(0, self.subtile_h.saturating_sub(1)),
                    );
                }
            }
            point
        };
        let mut unit_target = None;
        if !is_animal && air.air.returning == 0 && phase % 16 == 0 {
            let n = air.points.len();
            let (sx, sy) = relative_scan_point((air.points.x[n - 1], air.points.y[n - 1]));
            let search = if host.actor_is_type(self, row, 0x130, false)? {
                AirPatrolSearch::BomberFirst
            } else {
                AirPatrolSearch::AirFirst
            };
            unit_target = host.find_unit_target(self, row, &air, sx, sy, search)?;
        }

        let mut building_target = None;
        if !is_animal && phase % 32 == 0 {
            let cursor = air.points.clamp_air_cursor();
            let (sx, sy) = relative_scan_point((air.points.x[cursor], air.points.y[cursor]));
            building_target = host.find_building_target(self, row, &air, sx, sy)?;
        }

        let input = AirPatrolAfterPhysics {
            actor_x: self.sim.pos_x()[row],
            actor_y: self.sim.pos_y()[row],
            actor_o: self.sim.units.o()[row],
            frame: self.sim.frame,
            is_animal,
            spell_time: self.spell_time[row],
            order_list_len: list_len,
            unit_target,
            building_target,
        };
        let action = patrol::step_air_patrol_after_physics(&mut air, target, &input);
        if let Some(front) = self.orders[row].front_mut() {
            front.patrol_payload = PatrolPayload::Air(air.clone());
        }
        match action {
            AirPatrolAction::KillCurrent => self.retire_front_order(row),
            AirPatrolAction::InsertStrafe { target, mandatory } => {
                let order =
                    patrol::patrol_strafe_order(target, air.air.oxx, air.air.whose, mandatory);
                self.orders[row].push_front(OrderRec::strafe(order));
                self.sync_order_from_queue(row);
            }
            AirPatrolAction::PrimeAnimalSpellTime => {
                self.spell_time[row] = 1;
                self.sync_order_from_queue(row)
            }
            AirPatrolAction::Continue => self.sync_order_from_queue(row),
        }
        Ok(())
    }

    /// Straight-line integer approach to the destination at the type's `MOVES` rate.
    ///
    /// **Not** the engine's mover: `Unit::move_step` uses `sin_table`/`cosx`/`find_angle`
    /// and the real path comes from `PathFinder::astar_path` `0x00683770`, which is
    /// unread. This exists so a MOVE action changes state.
    fn advance_move(&mut self, row: usize, retire_on_arrival: bool) {
        let (x, y) = (self.dest_x[row], self.dest_y[row]);
        if self.advance_towards(row, x, y) && retire_on_arrival {
            if !self.orders[row].is_empty() {
                self.retire_front_order(row);
            } else {
                self.order[row] = g::OrderIndex::None as u8;
            }
        }
    }

    /// Execute a FORM-installed `GROUP_MOVE` only while the compact world can prove the
    /// recovered leader/group/order relationship from its own live rows.
    ///
    /// `Group::action_form` has already written each member's authoritative final `x/y` into
    /// its order node. EnvWorld does not own the retail `Groups` pool's changing
    /// `curr_x/curr_y` path offsets, so it must not invent a refresh after the leader/order
    /// relationship is lost. While that relationship is present, the existing reduced-world
    /// mover may advance toward the installed member destination; otherwise the node remains
    /// intact and the missing execution fact is counted.
    fn advance_group_move(&mut self, row: usize) {
        let Some(order) = self.orders[row].front().cloned() else {
            self.order[row] = OrderIndex::None as u8;
            return;
        };
        if order.kind != OrderIndex::GroupMove
            || order.group_id < 0
            || order.group_oxx < 0
            || order.group_whose < 0
        {
            self.unimplemented.unit[g::uv::FORM] += 1;
            return;
        }
        if self.sim.units.group()[row] < 0 {
            self.convert_group_order_to_ordinary(row, &order);
            return;
        }
        let Ok(leader_who) = u8::try_from(order.group_whose) else {
            self.unimplemented.unit[g::uv::FORM] += 1;
            return;
        };
        let Ok(leader_o) = i16::try_from(order.group_oxx) else {
            self.unimplemented.unit[g::uv::FORM] += 1;
            return;
        };
        let Some(leader_row) = self.fleet_row(leader_who, leader_o) else {
            self.unimplemented.unit[g::uv::FORM] += 1;
            return;
        };
        let same_group = self.sim.units.group()[leader_row] == self.sim.units.group()[row];
        let leader_has_order = self.orders[leader_row].front().is_some_and(|leader_order| {
            leader_order.kind == OrderIndex::GroupMove
                && leader_order.group_id == order.group_id
                && leader_order.group_oxx == order.group_oxx
                && leader_order.group_whose == order.group_whose
        });
        if !same_group || !leader_has_order {
            let dx = order.x - self.sim.pos_x()[row];
            let dy = order.y - self.sim.pos_y()[row];
            if vector_dist(dx, dy) <= 0x5ff {
                // Exact local arm at 0x005E7EE2/0x005E8654: a near follower whose leader
                // relationship disappeared converts this node, returns, and executes the
                // ordinary move on the next frame.
                self.convert_group_order_to_ordinary(row, &order);
            } else {
                // The far branch is Group::refresh_group_order, whose mutable Groups-pool
                // state EnvWorld does not own.
                self.unimplemented.unit[g::uv::FORM] += 1;
            }
            return;
        }
        self.dest_x[row] = order.x;
        self.dest_y[row] = order.y;
        self.advance_move(row, true);
    }

    /// The locally exact, allocation-free part of `Unit::ungroup_move_order`: replace the
    /// group node with its ordinary movement counterpart and preserve the MoveOrder fields.
    fn convert_group_order_to_ordinary(&mut self, row: usize, original: &OrderRec) {
        let actor_who = i32::from(self.sim.owner()[row]);
        let actor_o = i32::from(self.sim.units.o()[row]);
        let is_leader = original.group_whose == actor_who && original.group_oxx == actor_o;
        let current = self.orders[row]
            .front_mut()
            .expect("GROUP_MOVE conversion requires the front node cloned by its caller");
        current.kind = match original.kind {
            OrderIndex::GroupMove => OrderIndex::MoveTo,
            OrderIndex::GroupAttackTo => OrderIndex::AttackTo,
            _ => return,
        };
        if !is_leader {
            current.flags &= !ORDER_PATHED;
        }
        current.dest = 0;
        current.orig_x = current.x;
        current.orig_y = current.y;
        current.group_oxx = -1;
        current.group_whose = -1;
        current.group_id = -1;
        current.group_form_id = 0;
        current.group_angle = 0;
        current.in_group = 0;
        self.sync_order_from_queue(row);
    }

    /// Product boundary for `GROUP_ATTACK_TO`.
    ///
    /// EnvWorld can authoritatively perform the same local ungroup conversions as retail,
    /// but it does not own the virtual target predicate, `Unit::fight`, or
    /// `Unit::do_attack_to_pause`. A live grouped node therefore remains intact: advancing its
    /// movement before learning that combat is unavailable would violate the wrapper's
    /// preflight transaction. Ungrouped actors and near followers whose leader relation was
    /// lost convert to ordinary `ATTACK_TO`, which is the exact reachable local branch.
    fn advance_group_attack_to(&mut self, row: usize) {
        let Some(order) = self.orders[row].front().cloned() else {
            self.order[row] = OrderIndex::None as u8;
            return;
        };
        if order.kind != OrderIndex::GroupAttackTo
            || order.group_id < 0
            || order.group_oxx < 0
            || order.group_whose < 0
        {
            self.unimplemented.unit[g::uv::MOVE_TO] += 1;
            return;
        }
        if self.sim.units.group()[row] < 0 {
            self.convert_group_order_to_ordinary(row, &order);
            return;
        }
        let leader = u8::try_from(order.group_whose)
            .ok()
            .zip(i16::try_from(order.group_oxx).ok())
            .and_then(|(who, object)| self.fleet_row(who, object));
        let valid_leader = leader.is_some_and(|leader_row| {
            self.sim.units.group()[leader_row] == self.sim.units.group()[row]
                && self.orders[leader_row].front().is_some_and(|leader_order| {
                    leader_order.kind == OrderIndex::GroupAttackTo
                        && leader_order.group_id == order.group_id
                        && leader_order.group_oxx == order.group_oxx
                        && leader_order.group_whose == order.group_whose
                })
        });
        if !valid_leader {
            let dx = order.x - self.sim.pos_x()[row];
            let dy = order.y - self.sim.pos_y()[row];
            if vector_dist(dx, dy) <= 0x5ff {
                self.convert_group_order_to_ordinary(row, &order);
            } else {
                self.unimplemented.unit[g::uv::MOVE_TO] += 1;
            }
            return;
        }
        self.unimplemented.unit[g::uv::MOVE_TO] += 1;
    }

    /// Product boundary for `GROUP_ATTACK`.
    ///
    /// The ungrouped retail branch is a local AttackOrder copy and is authoritative here.
    /// Every grouped branch depends on target liveness/range/nearby search and mutable Groups
    /// operations (`fight`, kill/distribute/refresh), none of which EnvWorld currently owns as
    /// one transactional snapshot. Such nodes remain byte-for-byte intact and are counted.
    fn advance_group_attack(&mut self, row: usize) {
        let Some(order) = self.orders[row].front().cloned() else {
            self.order[row] = OrderIndex::None as u8;
            return;
        };
        if order.kind != OrderIndex::GroupAttack {
            self.unimplemented.unit[g::uv::ATTACK] += 1;
            return;
        }
        if self.sim.units.group()[row] >= 0 {
            self.unimplemented.unit[g::uv::ATTACK] += 1;
            return;
        }

        // Unit::set_angle(group_angle, ?, 0): publish the represented UnitData angle and its
        // large-turn mask toggle before replacing the order. EnvWorld does not store Guys.
        let old_angle = self.sim.units.angle()[row];
        let angle_delta = (order.group_angle as u32).wrapping_sub(old_angle as u32);
        if angle_delta > 0x3fff_ffff && angle_delta < 0xc000_0001 {
            self.sim.units.unit_masks_mut()[row] ^= 2;
        }
        self.sim.units.angle_mut()[row] = order.group_angle;

        // AttackOrder::operator= copies exactly the UnitOrder flag, TargetOrder identity,
        // and AttackOrder fields into a freshly allocated ordinary ATTACK.
        let ordinary = OrderRec {
            kind: OrderIndex::Attack,
            flags: order.flags,
            target_o: order.target_o,
            target_who: order.target_who,
            target_uid: order.target_uid,
            attack_def_x: order.attack_def_x,
            attack_def_y: order.attack_def_y,
            attack_mandatory: order.attack_mandatory,
            attack_defensive: order.attack_defensive,
            attack_in_range: order.attack_in_range,
            attack_ever_in_range: order.attack_ever_in_range,
            attack_new_ord: order.attack_new_ord,
            ..OrderRec::default()
        };
        *self.orders[row]
            .front_mut()
            .expect("GROUP_ATTACK conversion cloned a live front node") = ordinary;
        self.sync_order_from_queue(row);
    }

    /// Integrate one frame toward an explicit target using the environment's existing
    /// movement host. Returns true when the target was reached this frame.
    fn advance_towards(&mut self, row: usize, target_x: i32, target_y: i32) -> bool {
        let (px, py) = (self.sim.pos_x()[row], self.sim.pos_y()[row]);
        let (dx, dy) = (target_x - px, target_y - py);
        let step = self.speed[row].max(1) as i32;
        let dist2 = (dx as i64) * (dx as i64) + (dy as i64) * (dy as i64);
        if dist2 <= (step as i64) * (step as i64) {
            self.sim.set_pos(row, target_x, target_y);
            return true;
        }
        // Integer normalisation via the Chebyshev/octagonal approximation; no float, no
        // trig table. Deliberately a different approximation from the engine's, and
        // labelled as such rather than dressed up as the engine's.
        let (ax, ay) = (dx.abs(), dy.abs());
        let denom = (ax.max(ay) * 1007 + ax.min(ay) * 441) >> 10;
        let denom = denom.max(1);
        let nx = px + dx * step / denom;
        let ny = py + dy * step / denom;
        self.sim.set_pos(
            row,
            nx.rem_euclid(self.subtile_w),
            ny.rem_euclid(self.subtile_h),
        );
        false
    }

    fn advance_attack(&mut self, row: usize) {
        let Some(trow) = self.sim.row_of(self.target[row]) else {
            self.target[row] = NO_HANDLE;
            if self.orders[row].is_empty() {
                self.order[row] = g::OrderIndex::None as u8;
            } else {
                self.retire_front_order(row);
            }
            return;
        };
        let (px, py) = (self.sim.pos_x()[row], self.sim.pos_y()[row]);
        let (tx, ty) = (self.sim.pos_x()[trow], self.sim.pos_y()[trow]);
        // RANGE is in TCoords; 1 WCoord = 4 TCoords [unitrules.xml comment], and one
        // WCoord is one tile here, so a TCoord is SUBTILE/4.
        let reach = (self.range_max[row].max(1) as i64) * (SUBTILE as i64 / 4) + SUBTILE as i64;
        let d2 = ((tx - px) as i64).pow(2) + ((ty - py) as i64).pow(2);
        if d2 > reach * reach {
            self.dest_x[row] = tx;
            self.dest_y[row] = ty;
            self.advance_move(row, false);
            return;
        }
        if self.sim.cooldown()[row] > 0 {
            return;
        }
        let dmg = self.resolve_damage(row, trow);
        self.sim.hits_mut()[trow] -= dmg;
        let rc = self.cap(self.type_index[row]).recharge.max(1);
        self.sim.cooldown_mut()[row] = rc;
        let killer = self.sim.owner()[row] as usize;
        if self.sim.hits()[trow] <= 0 && killer < g::NUM_PLAYERS {
            self.players[killer].units_killed += 1;
        }
    }

    /// Stable handle for a live row.
    #[inline]
    pub fn handle_at(&self, row: usize) -> Handle {
        Handle {
            id: self.sim.handles()[row],
            generation: self.handle_gen[row],
        }
    }

    fn reap(&mut self) {
        let mut row = 0usize;
        while row < self.sim.live_count() as usize {
            if self.sim.hits()[row] <= 0 {
                let h = self.handle_at(row);
                self.try_despawn(h)
                    .expect("reap must retire a valid gather lifecycle");
            } else {
                row += 1;
            }
        }
    }

    fn recompute_scores(&mut self) {
        let n = self.sim.live_count() as usize;
        let mut units = [0i32; g::NUM_PLAYERS];
        let mut builds = [0i32; g::NUM_PLAYERS];
        for row in 0..n {
            let o = self.sim.owner()[row] as usize;
            if o >= g::NUM_PLAYERS {
                continue;
            }
            if self.rules.caps.get(self.type_index[row]).has(F_BUILDING) {
                builds[o] += 1;
            } else {
                units[o] += 1;
            }
        }
        for p in 0..g::NUM_PLAYERS {
            let ps = &mut self.players[p];
            ps.score.score_units = units[p];
            ps.score.score_buildings = builds[p];
            ps.score.score_pop = ps.pop;
            ps.score.score_economy = ps.collected.iter().sum::<i32>() / 10;
            ps.score.score_combat = ps.units_killed * 2 - ps.units_lost;
            if ps.alive && units[p] == 0 && builds[p] == 0 {
                ps.alive = false;
                ps.defeat_type = 1; // eliminated
            }
        }
    }

    /// Reset to a fresh episode. Placeholder scenario: `start_units` peasants per agent,
    /// on a ring. Real start positions come from the map generator, which is underived.
    pub fn reset(&mut self, num_agents: usize, start_units: usize, seed: u64) {
        while self.sim.live_count() > 0 {
            let h = self.handle_at(0);
            self.try_despawn(h)
                .expect("reset must retire a valid gather lifecycle");
        }
        self.gather = EnvGatherState::default();
        for v in self.ctrl.iter_mut().chain(self.obs_ents.iter_mut()) {
            v.clear();
        }
        self.rng = seed | 1;
        self.sim.frame = 0;
        self.step_index = 0;
        self.done = false;
        self.truncated = false;
        for p in 0..g::NUM_PLAYERS {
            self.players[p] = PlayerState::new(p as u8);
            self.players[p].alive = p < num_agents;
        }
        // Peasants (TypeIndex 50) and one Small City (414) each: the two types whose
        // positional identity in the shipped XML is cross-validated in typecaps.rs.
        for a in 0..num_agents {
            let ang = a as i32;
            let cx = self.subtile_w / 2 + (self.subtile_w / 3) * (1 - 2 * (ang & 1)) / 2;
            let cy = self.subtile_h / 2 + (self.subtile_h / 3) * (1 - 2 * ((ang >> 1) & 1)) / 2;
            self.spawn(a as u8, g::BUILD_TYPE_BASE as u16, cx, cy);
            for k in 0..start_units {
                let r = self.next_rand();
                let ox = ((r % 9) as i32 - 4) * SUBTILE;
                let oy = (((r >> 8) % 9) as i32 - 4) * SUBTILE;
                let t = if k % 4 == 0 {
                    // A unit with a real attack, so the ATTACK verb has something to do.
                    first_attacker(&self.rules.caps)
                } else {
                    g::UNIT_TYPE_BASE as u16
                };
                self.spawn(
                    a as u8,
                    t,
                    (cx + ox).rem_euclid(self.subtile_w),
                    (cy + oy).rem_euclid(self.subtile_h),
                );
            }
        }
        self.recompute_scores();
    }
}

// The command bridge's product host. Every lookup resolves the engine's `(who,o)`
// identity through ObjectRegistry; dense EnvWorld row indices never leak across this
// boundary.
impl EnvWorld {
    fn fleet_row(&self, who: u8, o: i16) -> Option<usize> {
        // ObjectRegistry::slot indexes the fixed retail owner table directly.
        // Reject an invalid wire owner before reaching that indexing operation.
        if usize::from(who) >= crate::generated::NUM_PLAYERS {
            return None;
        }
        let index = usize::try_from(o).ok()?;
        let row = *self
            .sim
            .objects
            .slot(who as usize)
            .band(Band::Unit)
            .get(index)?;
        let row = row as usize;
        (row < self.sim.live_count() as usize).then_some(row)
    }

    fn fleet_row_has_gather(&self, row: usize) -> bool {
        let owner = self.sim.owner()[row];
        let object = self.sim.units.o()[row];
        u8::try_from(owner).is_ok_and(|owner| {
            self.gather
                .farm_orders
                .iter()
                .any(|order| order.worker_owner == owner && order.worker_o == object)
        })
    }
}

impl Fleet for EnvWorld {
    fn alive(&self, who: u8, o: i16) -> bool {
        self.fleet_row(who, o)
            .is_some_and(|row| self.sim.hits()[row] > 0)
    }

    fn is_unit(&self, who: u8, o: i16) -> bool {
        self.fleet_row(who, o).is_some_and(|row| {
            let cap = self.cap(self.type_index[row]);
            cap.has(crate::typecaps::F_UNIT) && !cap.has(F_BUILDING)
        })
    }

    fn is_building(&self, who: u8, o: i16) -> bool {
        self.fleet_row(who, o)
            .is_some_and(|row| self.cap(self.type_index[row]).has(F_BUILDING))
    }

    fn is_on_map(&self, who: u8, o: i16) -> bool {
        // EnvWorld currently has no containment transition: every live row is on-map.
        self.alive(who, o)
    }

    fn is_captain(&self, who: u8, o: i16) -> bool {
        // EnvWorld spawns only root Unit rows; subordinate Guy rows are not entities.
        self.is_unit(who, o)
    }

    fn form_category(&self, who: u8, o: i16) -> i32 {
        let Some(row) = self.fleet_row(who, o) else {
            return 18;
        };
        let Some(cap) = self.rules.formation_cap(self.type_index[row]) else {
            return 18;
        };
        let leader_flags = self
            .players
            .get(who as usize)
            .map_or(0, |player| player.leader_flags);
        cap.category(leader_flags)
    }

    fn formation_member(
        &self,
        who: u8,
        o: i16,
        water_destination: bool,
    ) -> Option<FormationMember> {
        // The compact EnvWorld map has no water/transport effective-type substitution.
        if water_destination {
            return None;
        }
        let row = self.fleet_row(who, o)?;
        let cap = self.rules.formation_cap(self.type_index[row])?;
        let player = self.players.get(who as usize)?;
        Some(FormationMember {
            category: cap.category(player.leader_flags),
            x_spacing: cap.x_spacing,
            y_spacing: cap.y_spacing,
            formation_size: cap.uber_size,
            guy_spacing: cap.guy_spacing,
            modern_infantry: cap.modern_infantry(player.has_modern_infantry_tech),
            width: i32::from(self.sim.units.form_mod()[row]),
            angle: self.sim.units.angle()[row],
        })
    }

    fn formation_water_destination(&self, _x: i32, _y: i32) -> bool {
        // EnvWorld's current grid has no terrain-domain column; all admitted positions
        // are land. This is an authoritative fact of this reduced world, not a retail-map
        // guess.
        false
    }

    fn form(&self, who: u8, o: i16) -> i8 {
        self.fleet_row(who, o)
            .map_or(-1, |row| self.sim.units.form()[row])
    }

    fn set_form(&mut self, who: u8, o: i16, form: i8) {
        if let Some(row) = self.fleet_row(who, o) {
            self.sim.units.form_mut()[row] = form;
            self.form[row] = form as u8;
        }
    }

    fn angle(&self, who: u8, o: i16) -> i32 {
        self.fleet_row(who, o)
            .map_or(0, |row| self.sim.units.angle()[row])
    }

    fn role(&self, who: u8, o: i16) -> i32 {
        self.fleet_row(who, o)
            .and_then(|row| self.rules.formation_cap(self.type_index[row]))
            .map_or(0, |cap| cap.role)
    }

    fn domain(&self, who: u8, o: i16) -> i32 {
        self.fleet_row(who, o)
            .and_then(|row| self.rules.formation_cap(self.type_index[row]))
            .map_or(-1, |cap| cap.domain)
    }

    fn unit_masks(&self, who: u8, o: i16) -> u32 {
        self.fleet_row(who, o)
            .map_or(0, |row| self.sim.units.get_unit_masks(row))
    }

    fn set_unit_masks(&mut self, who: u8, o: i16, masks: u32) {
        if let Some(row) = self.fleet_row(who, o) {
            self.sim.units.set_unit_masks(row, masks);
        }
    }

    fn can_move(&self, who: u8, o: i16) -> bool {
        self.fleet_row(who, o).is_some_and(|row| {
            self.sim.hits()[row] > 0 && self.cap(self.type_index[row]).has(F_MOVE)
        })
    }

    fn is_plane(&self, who: u8, o: i16) -> bool {
        self.fleet_row(who, o)
            .is_some_and(|row| self.cap(self.type_index[row]).is_plane)
    }

    fn group_of(&self, who: u8, o: i16) -> i16 {
        self.fleet_row(who, o)
            .map_or(-1, |row| self.sim.units.group()[row])
    }

    fn set_group_of(&mut self, who: u8, o: i16, slot: i16) {
        if let Some(row) = self.fleet_row(who, o) {
            self.sim.units.group_mut()[row] = slot;
        }
    }

    fn uid(&self, who: u8, o: i16) -> u16 {
        self.fleet_row(who, o)
            .map_or(u16::MAX, |row| self.sim.units.get_uid(row))
    }

    fn pos(&self, who: u8, o: i16) -> (i32, i32) {
        self.fleet_row(who, o)
            .map_or((0, 0), |row| (self.sim.pos_x()[row], self.sim.pos_y()[row]))
    }

    fn valid_pos(&self, x: i32, y: i32) -> bool {
        (0..self.subtile_w).contains(&x) && (0..self.subtile_h).contains(&y)
    }

    fn orders(&self, who: u8, o: i16) -> Option<&OrderQueue> {
        let row = self.fleet_row(who, o)?;
        self.orders.get(row)
    }

    fn orders_mut(&mut self, who: u8, o: i16) -> Option<&mut OrderQueue> {
        let row = self.fleet_row(who, o)?;
        self.orders.get_mut(row)
    }

    fn can_install_order(&self, who: u8, o: i16, _queue: QueuePos) -> bool {
        self.fleet_row(who, o)
            .is_some_and(|row| !self.fleet_row_has_gather(row))
    }

    fn install_order_rec(&mut self, who: u8, o: i16, order: OrderRec, queue: QueuePos) -> bool {
        let Some(row) = self.fleet_row(who, o) else {
            return false;
        };
        EnvWorld::install_order(self, row, order, queue).is_ok()
    }

    fn set_stance(&mut self, who: u8, o: i16, stance: i8) {
        if let Some(row) = self.fleet_row(who, o) {
            self.sim.units.stance_mut()[row] = stance;
            self.stance[row] = stance as u8;
        }
    }

    fn disband(&mut self, who: u8, o: i16) {
        if let Some(row) = self.fleet_row(who, o) {
            let handle = self.handle_at(row);
            let _ = self.try_despawn(handle);
        }
    }
}

/// The lowest unit TypeIndex that both moves and attacks. Derived from the table, not
/// named, so it stays correct if the table changes.
fn first_attacker(caps: &TypeCaps) -> u16 {
    for t in g::UNIT_TYPE_BASE..g::GAIA_TYPE_BASE {
        let c = caps.get(t as u16);
        if c.has(F_ATTACK) && c.has(F_MOVE) {
            return t as u16;
        }
    }
    g::UNIT_TYPE_BASE as u16
}

impl PlayerState {
    pub fn pay_public(&mut self, cost: &[i32; g::NUM_COMMON]) {
        self.pay(cost)
    }
}
