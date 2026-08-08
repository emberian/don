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
//! Everything else — gathering, construction, tech, pathing, fog — is **scaffolding**, and
//! each scaffolded verb increments a counter in [`EnvWorld::unimplemented`] so a training
//! run can print exactly which parts of the action space currently have no dynamics behind
//! them. That counter is the point: an RL surface whose gaps are silent is worse than no
//! surface at all.

use crate::generated as g;
use crate::typecaps::{TypeCap, TypeCaps, F_ATTACK, F_BUILDING, F_MOVE};
use don_sim::world::SUBTILE;
use don_sim::{Handle, World};
use std::sync::Arc;

/// Static tables shared by every world in a batch; never mutated after construction.
pub struct Rules {
    pub caps: TypeCaps,
    /// `Balance::final_balance_table` at `0x00C12BF4`, `short[493][493]` [measured].
    /// `None` when `schema/live/balance-real.bin` is absent; the damage chain then sees a
    /// flat 100 % balance term and [`Rules::balance_is_real`] is false.
    pub balance: Option<Vec<i16>>,
    pub building_types: Vec<u8>,
}

pub const BALANCE_DIM: usize = 493;

impl Rules {
    pub fn load(typecaps: Option<&std::path::Path>, balance: Option<&std::path::Path>) -> (Arc<Rules>, bool, bool) {
        let (caps, caps_real) = TypeCaps::load_or_permissive(typecaps);
        let p = balance
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| {
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
        (Arc::new(Rules { caps, balance, building_types }), caps_real, bal_real)
    }

    pub fn balance_is_real(&self) -> bool {
        self.balance.is_some()
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
        "score_explored", "score_territory", "score_units", "score_units_2",
        "score_buildings", "score_economy", "score_pop", "score_unit_upgrades",
        "score_research", "score_wonders", "score_combat",
    ];
    /// Terms this build actually computes. The rest are structurally present and always
    /// zero; see `docs/tracks/rl-env.md`.
    pub const LIVE: [&'static str; 5] =
        ["score_units", "score_buildings", "score_economy", "score_pop", "score_combat"];

    pub fn as_array(&self) -> [i32; 11] {
        [
            self.score_explored, self.score_territory, self.score_units, self.score_units_2,
            self.score_buildings, self.score_economy, self.score_pop,
            self.score_unit_upgrades, self.score_research, self.score_wonders,
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
    /// `LeaderData::defeat_type` (0 = not defeated) and `victory_type`.
    pub defeat_type: i32,
    pub victory_type: i32,
    /// `LeaderData::econ[6]`: FOOD TIMBER WEALTH KNOWLEDGE METAL OIL.
    pub econ: [i32; g::NUM_COMMON],
    /// `LeaderData::base_rate[6]`.
    pub base_rate: [i32; g::NUM_COMMON],
    /// `LeaderData::collected[6]`, lifetime.
    pub collected: [i32; g::NUM_COMMON],
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
        PlayerState {
            who,
            team: who,
            alive: true,
            defeat_type: 0,
            victory_type: 0,
            // Placeholder starting stock. The real start economy comes from
            // `Constants::init` and is not wired; see the provenance report.
            econ: [200, 200, 200, 0, 0, 0],
            base_rate: [0; g::NUM_COMMON],
            collected: [0; g::NUM_COMMON],
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

/// One environment instance.
pub struct EnvWorld {
    pub sim: World,
    pub rules: Arc<Rules>,
    pub players: Vec<PlayerState>,
    // ---- entity columns, parallel to `sim` rows -------------------------------------
    pub type_index: Vec<u16>,
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

const NO_HANDLE: Handle = Handle { id: u32::MAX, generation: 0 };

impl EnvWorld {
    pub fn new(rules: Arc<Rules>, capacity: usize, seed: u64, grid_w: usize, grid_h: usize) -> EnvWorld {
        let cap = capacity.min(don_sim::MAX_UNITS);
        EnvWorld {
            sim: World::with_capacity(cap, seed),
            rules,
            players: (0..g::NUM_PLAYERS).map(|i| PlayerState::new(i as u8)).collect(),
            type_index: vec![0; cap],
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
        self.order[row] = g::OrderIndex::None as u8;
        self.target[row] = NO_HANDLE;
        self.dest_x[row] = x;
        self.dest_y[row] = y;
        self.stance[row] = 0;
        self.form[row] = 0;
        self.attack[row] = c.attack;
        self.armor[row] = c.armor;
        self.max_hits[row] = c.hits.max(1);
        self.speed[row] = c.move_rate;
        self.range_max[row] = c.range_max;
        self.handle_gen[row] = h.generation;
        self.sim.set_pos(row, x.rem_euclid(self.subtile_w), y.rem_euclid(self.subtile_h));
        self.sim.hits_mut()[row] = c.hits.max(1);
        self.sim.cooldown_mut()[row] = 0;
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
    pub fn despawn(&mut self, h: Handle) -> bool {
        let Some(row) = self.sim.row_of(h) else { return false };
        let last = self.sim.live_count() as usize - 1;
        let t = self.type_index[row];
        let owner = self.sim.owner()[row] as usize;
        if row != last {
            self.type_index[row] = self.type_index[last];
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
            self.handle_gen[row] = self.handle_gen[last];
        }
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
        ok
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
        (self.sim.pos_x()[row] / SUBTILE, self.sim.pos_y()[row] / SUBTILE)
    }

    #[inline]
    pub fn relation(&self, a: u8, b: u8) -> u8 {
        if a == b {
            return 0;
        }
        let (a, b) = (a as usize, b as usize);
        if a >= g::NUM_PLAYERS || b >= g::NUM_PLAYERS {
            return 3;
        }
        match self.players[a].diplos[b] {
            2 => 1, // ALLY
            0 => 2, // WAR
            _ => 3, // PEACE / neutral
        }
    }

    // ---- per-frame systems -----------------------------------------------------------

    /// Advance one simulation frame.
    ///
    /// Owner slots are visited in the engine's rotated order: `Objects::process_all`
    /// starts at `frame % 10` and walks `(frame + i) % 10` [measured]. A fixed-order
    /// scheduler diverges inside one tick, so the rotation is reproduced here even though
    /// the per-object work below is scaffolding.
    pub fn frame(&mut self) {
        let f = self.sim.frame as usize;
        for i in 0..g::NUM_OWNER_SLOTS {
            let slot = ((f + i) % g::NUM_OWNER_SLOTS) as u8;
            self.process_slot(slot);
        }
        don_sim::simd::tick_down(self.sim.cooldown_mut());
        self.sim.frame += 1;
        self.reap();
        self.recompute_scores();
    }

    fn process_slot(&mut self, slot: u8) {
        let n = self.sim.live_count() as usize;
        for row in 0..n {
            if self.sim.owner()[row] != slot {
                continue;
            }
            match self.order[row] {
                x if x == g::OrderIndex::MoveTo as u8 || x == g::OrderIndex::AttackTo as u8 => {
                    self.advance_move(row);
                }
                x if x == g::OrderIndex::Attack as u8 => self.advance_attack(row),
                _ => {}
            }
        }
    }

    /// Straight-line integer approach to the destination at the type's `MOVES` rate.
    ///
    /// **Not** the engine's mover: `Unit::move_step` uses `sin_table`/`cosx`/`find_angle`
    /// and the real path comes from `PathFinder::astar_path` `0x00683770`, which is
    /// unread. This exists so a MOVE action changes state.
    fn advance_move(&mut self, row: usize) {
        let (px, py) = (self.sim.pos_x()[row], self.sim.pos_y()[row]);
        let (dx, dy) = (self.dest_x[row] - px, self.dest_y[row] - py);
        let step = self.speed[row].max(1) as i32;
        let dist2 = (dx as i64) * (dx as i64) + (dy as i64) * (dy as i64);
        if dist2 <= (step as i64) * (step as i64) {
            self.sim.set_pos(row, self.dest_x[row], self.dest_y[row]);
            self.order[row] = g::OrderIndex::None as u8;
            return;
        }
        // Integer normalisation via the Chebyshev/octagonal approximation; no float, no
        // trig table. Deliberately a different approximation from the engine's, and
        // labelled as such rather than dressed up as the engine's.
        let (ax, ay) = (dx.abs(), dy.abs());
        let denom = (ax.max(ay) * 1007 + ax.min(ay) * 441) >> 10;
        let denom = denom.max(1);
        let nx = px + dx * step / denom;
        let ny = py + dy * step / denom;
        self.sim.set_pos(row, nx.rem_euclid(self.subtile_w), ny.rem_euclid(self.subtile_h));
    }

    fn advance_attack(&mut self, row: usize) {
        let Some(trow) = self.sim.row_of(self.target[row]) else {
            self.order[row] = g::OrderIndex::None as u8;
            self.target[row] = NO_HANDLE;
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
            self.advance_move(row);
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
        Handle { id: self.sim.handles()[row], generation: self.handle_gen[row] }
    }

    fn reap(&mut self) {
        let mut row = 0usize;
        while row < self.sim.live_count() as usize {
            if self.sim.hits()[row] <= 0 {
                let h = self.handle_at(row);
                self.despawn(h);
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
            self.despawn(h);
        }
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
                self.spawn(a as u8, t, (cx + ox).rem_euclid(self.subtile_w), (cy + oy).rem_euclid(self.subtile_h));
            }
        }
        self.recompute_scores();
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
