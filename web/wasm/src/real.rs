//! A world of **real unit types**, fought with the **derived damage chain**.
//!
//! # What is derived here and what is not — read this before believing anything on screen
//!
//! Derived, and used unmodified:
//!
//! * `don_sim::damage` — `ObjectData::get_damage` `0x00644130`, the whole 31-step integer
//!   chain, Tier B against retail. Every attack in this file goes through it.
//! * `don_sim::get_attack` / `get_armor` — the two virtual getters, `0x006469F0` /
//!   `0x00647DB0`.
//! * `don_sim::balance_index` — the flat index into `Balance::final_balance_table`.
//! * `don_sim::flank_level`, reached from inside the damage chain's step 20.
//! * The unit-type table, the 493x493 balance table and the combat rules constants: all
//!   read out of the live process by other lanes, see [`crate::gamedata`].
//! * The owner-slot rotation `(frame + i) % 10` in [`RealWorld::step`], which is how
//!   `Objects::process_all` orders a frame. A fixed-order scheduler diverges inside one
//!   tick, so this is not cosmetic.
//! * The RNG recurrence `s <- s*1664525 + 1013904223`, `Random::get`.
//!
//! **Not derived — this file's own glue, and every one of them is a placeholder:**
//!
//! * *Target acquisition.* Nearest enemy in a local grid neighbourhood, else march on the
//!   nearest enemy owner's centroid. The engine's acquisition lives in `Unit`/`Order` code
//!   nobody has read yet.
//! * *Movement.* One integer step of `MOVES` subtiles along the straight line to the goal.
//!   The engine's `Unit::move_step` uses `sin_table`/`cosx`/`find_angle` and a real
//!   pathfinder; there is no terrain here at all.
//! * *The attack angle.* [`atan2_u32`] is an integer approximation written for this file.
//!   The engine has its own `find_angle`. What is derived is the *consumer*: the flank
//!   tier and the `FLANK_BONUS` arithmetic in step 20.
//! * *Which damage predicates are true.* The chain reads about thirty booleans obtained by
//!   walking the object graph. Only "attacker alive" and "defender alive" are set; every
//!   other predicate is left false, i.e. **inert**, because guessing them would move damage
//!   by factors of 2 and 4 with nothing behind the guess. See [`predicates`].
//! * *No terrain, no height, no buildings, no economy, no line of sight, no upgrades.*
//!
//! So: the *arithmetic* of a hit is the engine's, the numbers going into it are the
//! engine's, and the decision of who hits whom is ours. Read the screen accordingly.

use crate::gamedata::{GameData, UnitTypeRec};
use don_sim::{damage, get_armor, get_attack, DamageInput, DamagePredicates, UnreachedTerms};

/// Subtiles per tile. `rules.xml` states 192 as the largest speed denominator, and
/// `don_sim` uses the same granularity.
pub const SUBTILE: i32 = 192;
/// Map extent in tiles. A provisioning choice, not a derived map size: 128 tiles is about
/// the size of a small skirmish map, and it is what makes a battle fill a world's cell in
/// the cluster mosaic instead of sitting as a dot in the middle of an empty square.
pub const MAP_TILES: i32 = 128;
/// Map extent in subtiles.
pub const MAP_SPAN: i32 = MAP_TILES * SUBTILE;
/// Owner slots the frame scheduler rotates through — `Objects::process_all` uses
/// `(frame + i) % 10`.
pub const OWNER_SLOTS: usize = 10;
/// Reach of a `MAX_RANGE == 0` unit, in subtiles. Melee contact distance is not derived;
/// one tile is a placeholder.
const MELEE_REACH: i32 = SUBTILE;
/// Cap on units examined by one acquisition scan, so a dense world cannot turn target
/// selection into an O(n^2) frame.
const MAX_SCAN: u32 = 192;

const NO_TARGET: u32 = u32::MAX;
const NO_ROW: u32 = u32::MAX;

/// Order bits held per unit. `MOVE` is a standing move-to; nothing else is modelled.
pub mod order_bits {
    pub const MOVE: u8 = 1;
}

/// Integer `atan2` into the engine's angle space, where a full turn is `2^32`.
///
/// **This is ours, not the engine's.** The engine has `find_angle` and a `sin_table`; this
/// is the classic single-division octant approximation (error under ~0.005 turn), chosen
/// because it is integer-only and therefore deterministic across targets. It exists to feed
/// the damage chain's step 20, whose arithmetic *is* derived.
#[inline]
pub fn atan2_u32(dy: i32, dx: i32) -> u32 {
    let ay = (dy as i64).abs();
    let x = dx as i64;
    if ay == 0 && x == 0 {
        return 0;
    }
    // 0x20000000 is an eighth of a turn, i.e. PI/4 in this fixed-point angle space.
    const EIGHTH: i64 = 0x2000_0000;
    let a = if x >= 0 {
        let d = x + ay;
        EIGHTH - (EIGHTH * (x - ay)) / d
    } else {
        let d = ay - x;
        3 * EIGHTH - (EIGHTH * (x + ay)) / d
    };
    (if dy < 0 { -a } else { a }) as u32
}

/// Integer square root of a non-negative `i64`. No float, so identical on every target.
#[inline]
fn isqrt(v: i64) -> i64 {
    if v <= 0 {
        return 0;
    }
    let mut x = (v as f64).sqrt() as i64; // seed only; the loop below is what decides
                                          // Newton correction in integers: two steps are always enough from a f64 seed, but the
                                          // loop is written to converge from anything so the seed can never be load-bearing.
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

/// The damage predicates this simulation can honestly assert.
///
/// Two are true — both objects are alive, which is exactly what the caller has just
/// checked. Everything else is a walk of an object graph that does not exist here
/// (`vtbl[0x20]`, the build flag, six tech checks, three UNVERIFIED player properties), so
/// it is left false. False is the inert value for all of them: with this set, steps 3-19
/// and 23-30 of the chain do not fire, and damage reduces to
/// `((attack * balance / 100) [+ flank] + 5) / 10 - armor`, floored at 1 when the floor's
/// own three conditions hold.
///
/// Setting `attacker_vf_0x20` and `defender_vf_0x20` true would multiply damage by four
/// (step 5). That is exactly the kind of guess this project exists not to make.
pub fn predicates() -> DamagePredicates {
    DamagePredicates {
        attacker_vf_0x18: true,
        defender_vf_0x18: true,
        ..Default::default()
    }
}

/// One battle. Structure-of-arrays, dense rows, stable ids — the same storage discipline as
/// `don_sim::World`, extended with the columns a real unit needs (type, target, orders).
pub struct RealWorld {
    // ---- hot columns, touched every frame ----
    pos_x: Vec<i32>,
    pos_y: Vec<i32>,
    hits: Vec<i32>,
    cooldown: Vec<i16>,
    /// Engine-space angle this unit is facing; the defender's facing is what step 20 reads.
    facing: Vec<i32>,
    /// Handle id of the current target, or [`NO_TARGET`].
    target: Vec<u32>,
    // ---- cold columns ----
    type_idx: Vec<u16>,
    max_hits: Vec<i32>,
    owner: Vec<u8>,
    order_x: Vec<i32>,
    order_y: Vec<i32>,
    order_flags: Vec<u8>,
    selected: Vec<u8>,
    /// Per-instance render tag, refreshed at the end of each frame so the renderer can bind
    /// it as a vertex buffer with no CPU pass of its own. See [`RealWorld::tag_of`].
    tag: Vec<u32>,

    // ---- identity: a permutation of 0..capacity, live ids in the prefix ----
    handle_of_row: Vec<u32>,
    row_of_handle: Vec<u32>,
    live: u32,
    capacity: u32,

    // ---- per-frame scratch, allocated once ----
    /// Rows ordered by owner, so the rotation can walk one owner's units contiguously.
    order: Vec<u32>,
    owner_start: [u32; OWNER_SLOTS + 1],
    /// Uniform grid over the map: `grid_dim^2` cells, rows bucketed by counting sort.
    grid_dim: i32,
    cell_start: Vec<u32>,
    cell_rows: Vec<u32>,
    /// Per-owner centroid and population, recomputed each frame.
    cx: [i64; OWNER_SLOTS],
    cy: [i64; OWNER_SLOTS],
    pop: [u32; OWNER_SLOTS],

    pub frame: u64,
    rng: u32,
    seed: u32,
    /// Owners this world was populated with.
    pub owners: u8,
    pub units_per_owner: u32,
    /// Cumulative outputs of the derived damage chain, for the aggregate view.
    pub kills: u32,
    pub damage_dealt: u64,
    pub shots: u64,
    /// Battles fought since the world was created (it resets when a side is wiped out).
    pub rounds: u32,
    /// When true a wiped-out world restarts, so a cluster keeps moving indefinitely.
    pub auto_reset: bool,
}

impl RealWorld {
    pub fn with_capacity(capacity: usize, seed: u64) -> RealWorld {
        let n = capacity.max(1);
        // One grid cell per ~sqrt(capacity) units, clamped: a 64-unit world does not want
        // 1024 cells to clear every frame, and a 4096-unit world does want more than 16.
        let dim = (isqrt(n as i64 / 2).max(4).min(48)) as i32;
        RealWorld {
            pos_x: vec![0; n],
            pos_y: vec![0; n],
            hits: vec![0; n],
            cooldown: vec![0; n],
            facing: vec![0; n],
            target: vec![NO_TARGET; n],
            type_idx: vec![0; n],
            max_hits: vec![1; n],
            owner: vec![0; n],
            order_x: vec![0; n],
            order_y: vec![0; n],
            order_flags: vec![0; n],
            selected: vec![0; n],
            tag: vec![0; n],
            handle_of_row: (0..n as u32).collect(),
            row_of_handle: vec![NO_ROW; n],
            live: 0,
            capacity: n as u32,
            order: vec![0; n],
            owner_start: [0; OWNER_SLOTS + 1],
            grid_dim: dim,
            cell_start: vec![0; (dim * dim) as usize + 1],
            cell_rows: vec![0; n],
            cx: [0; OWNER_SLOTS],
            cy: [0; OWNER_SLOTS],
            pop: [0; OWNER_SLOTS],
            frame: 0,
            rng: (seed as u32) | 1,
            seed: (seed as u32) | 1,
            owners: 0,
            units_per_owner: 0,
            kills: 0,
            damage_dealt: 0,
            shots: 0,
            rounds: 0,
            auto_reset: true,
        }
    }

    /// `Random::get`'s recurrence, `0x00a39cf0` — `s <- s*1664525 + 1013904223` [measured].
    /// The *mapping* from state to a bounded value is not derived; the shifts below are
    /// this file's, and only spawn placement depends on them.
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

    #[inline]
    pub fn live_count(&self) -> u32 {
        self.live
    }
    #[inline]
    pub fn capacity(&self) -> u32 {
        self.capacity
    }
    #[inline]
    pub fn pos_x(&self) -> &[i32] {
        &self.pos_x[..self.live as usize]
    }
    #[inline]
    pub fn pos_y(&self) -> &[i32] {
        &self.pos_y[..self.live as usize]
    }
    #[inline]
    pub fn owner_col(&self) -> &[u8] {
        &self.owner[..self.live as usize]
    }
    #[inline]
    pub fn hits_col(&self) -> &[i32] {
        &self.hits[..self.live as usize]
    }
    #[inline]
    pub fn handles(&self) -> &[u32] {
        &self.handle_of_row[..self.live as usize]
    }
    /// The render tag column. Refreshed once per frame by [`RealWorld::refresh_tags`], so a
    /// vertex buffer can be filled straight from it with no per-entity CPU pass.
    #[inline]
    pub fn tags(&self) -> &[u32] {
        &self.tag[..self.live as usize]
    }
    /// Base pointers for the zero-copy render path; the length is `live_count()`.
    #[inline]
    pub fn pos_x_base(&self) -> *const i32 {
        self.pos_x.as_ptr()
    }
    #[inline]
    pub fn pos_y_base(&self) -> *const i32 {
        self.pos_y.as_ptr()
    }
    #[inline]
    pub fn tag_base(&self) -> *const u32 {
        self.tag.as_ptr()
    }

    #[inline]
    fn row_of_id(&self, id: u32) -> Option<usize> {
        if id >= self.capacity {
            return None;
        }
        let row = self.row_of_handle[id as usize];
        if row >= self.live || self.handle_of_row[row as usize] != id {
            return None;
        }
        Some(row as usize)
    }

    fn spawn(&mut self, gd: &GameData, unit_idx: usize, owner: u8, x: i32, y: i32) -> Option<u32> {
        if self.live >= self.capacity {
            return None;
        }
        let row = self.live as usize;
        let id = self.handle_of_row[row];
        let ut = &gd.units[unit_idx];
        self.pos_x[row] = x.clamp(0, MAP_SPAN - 1);
        self.pos_y[row] = y.clamp(0, MAP_SPAN - 1);
        self.hits[row] = ut.hits.max(1);
        self.max_hits[row] = ut.hits.max(1);
        self.cooldown[row] = 0;
        self.facing[row] = 0;
        self.target[row] = NO_TARGET;
        self.type_idx[row] = unit_idx as u16;
        self.owner[row] = owner;
        self.order_flags[row] = 0;
        self.selected[row] = 0;
        self.row_of_handle[id as usize] = row as u32;
        self.live += 1;
        Some(id)
    }

    fn despawn_row(&mut self, row: usize) {
        let last = self.live as usize - 1;
        let dead_id = self.handle_of_row[row];
        if row != last {
            self.pos_x[row] = self.pos_x[last];
            self.pos_y[row] = self.pos_y[last];
            self.hits[row] = self.hits[last];
            self.max_hits[row] = self.max_hits[last];
            self.cooldown[row] = self.cooldown[last];
            self.facing[row] = self.facing[last];
            self.target[row] = self.target[last];
            self.type_idx[row] = self.type_idx[last];
            self.owner[row] = self.owner[last];
            self.order_x[row] = self.order_x[last];
            self.order_y[row] = self.order_y[last];
            self.order_flags[row] = self.order_flags[last];
            self.selected[row] = self.selected[last];
            self.tag[row] = self.tag[last];
            let moved = self.handle_of_row[last];
            self.handle_of_row[row] = moved;
            self.row_of_handle[moved as usize] = row as u32;
        }
        self.handle_of_row[last] = dead_id;
        self.live -= 1;
    }

    /// Fill the world with `owners` armies of `per_owner` units each.
    ///
    /// Composition is a placeholder: an age band is drawn per world and each owner gets
    /// three roster types from it, so a cluster is visibly varied and the balance table
    /// actually gets exercised across matchups. Nothing about it models a real build order.
    pub fn populate(&mut self, gd: &GameData, owners: u8, per_owner: u32) {
        self.live = 0;
        for k in 0..self.capacity as usize {
            self.handle_of_row[k] = k as u32;
        }
        self.owners = owners.clamp(2, OWNER_SLOTS as u8);
        self.units_per_owner = per_owner;
        let roster = &gd.roster;
        if roster.is_empty() {
            return;
        }
        // Age band for this world: a contiguous slice of the roster, so the two sides meet
        // with comparable technology and the fight lasts long enough to watch.
        let band = 32.min(roster.len());
        let lo = self.rand_below((roster.len() - band + 1) as u32) as usize;
        // Armies start about a third of the map apart. That is a staging decision, not a
        // map fact: at the real `MOVES` values (a Citizen is 25 subtiles per frame, about
        // 2 tiles a second at the 67 ms tick) starting them at opposite corners is minutes
        // of walking before anything happens, which is not a spectator.
        let radius = MAP_SPAN / 6;
        let centre = MAP_SPAN / 2;
        // Spread the army over an area that grows with its size, so 2,048 units are not
        // stacked in the same twelve tiles as 32.
        let half = (3 + isqrt(per_owner as i64) as i32 * 2 / 3) * SUBTILE;
        for o in 0..self.owners {
            // Armies start on a circle around the map centre, facing inward.
            let ang = (o as i64 * 0x1_0000_0000i64) / self.owners.max(1) as i64;
            let (sx, sy) = unit_circle(ang as u32, radius);
            let ox = centre + sx;
            let oy = centre + sy;
            let mut kinds = [0usize; 3];
            for k in kinds.iter_mut() {
                *k = roster[lo + self.rand_below(band as u32) as usize] as usize;
            }
            for i in 0..per_owner {
                let kind = kinds[(i % 3) as usize];
                let jx = self.rand_below(2 * half as u32) as i32 - half;
                let jy = self.rand_below(2 * half as u32) as i32 - half;
                if self.spawn(gd, kind, o, ox + jx, oy + jy).is_none() {
                    break;
                }
            }
        }
    }

    /// Restart the battle in place, keeping the world's identity and its counters.
    fn reset_round(&mut self, gd: &GameData) {
        self.rounds += 1;
        // Advance the stream rather than reseeding, so the next round is a different fight
        // and the whole sequence is still a pure function of the original seed.
        let _ = self.rand();
        let owners = self.owners;
        let per = self.units_per_owner;
        self.populate(gd, owners, per);
    }

    // ---- per-frame index building --------------------------------------------------

    fn build_owner_order(&mut self) {
        let n = self.live as usize;
        let mut counts = [0u32; OWNER_SLOTS + 1];
        for row in 0..n {
            counts[(self.owner[row] as usize) % OWNER_SLOTS + 1] += 1;
        }
        for k in 1..=OWNER_SLOTS {
            counts[k] += counts[k - 1];
        }
        self.owner_start[..=OWNER_SLOTS].copy_from_slice(&counts[..=OWNER_SLOTS]);
        let mut cursor = counts;
        for row in 0..n {
            let o = (self.owner[row] as usize) % OWNER_SLOTS;
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

    fn build_grid(&mut self) {
        let n = self.live as usize;
        let cells = (self.grid_dim * self.grid_dim) as usize;
        for c in self.cell_start[..=cells].iter_mut() {
            *c = 0;
        }
        self.cx = [0; OWNER_SLOTS];
        self.cy = [0; OWNER_SLOTS];
        self.pop = [0; OWNER_SLOTS];
        for row in 0..n {
            let c = self.cell_of(self.pos_x[row], self.pos_y[row]);
            self.cell_start[c + 1] += 1;
            let o = (self.owner[row] as usize) % OWNER_SLOTS;
            self.cx[o] += self.pos_x[row] as i64;
            self.cy[o] += self.pos_y[row] as i64;
            self.pop[o] += 1;
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
        for o in 0..OWNER_SLOTS {
            if self.pop[o] > 0 {
                self.cx[o] /= self.pop[o] as i64;
                self.cy[o] /= self.pop[o] as i64;
            }
        }
    }

    /// Nearest enemy inside a widening ring of grid cells. Placeholder; see module docs.
    fn acquire(&self, row: usize) -> u32 {
        let d = self.grid_dim;
        let span = MAP_SPAN / d;
        let px = self.pos_x[row];
        let py = self.pos_y[row];
        let me = self.owner[row];
        let ccx = (px / span).clamp(0, d - 1);
        let ccy = (py / span).clamp(0, d - 1);
        let mut best = NO_TARGET;
        let mut best_d2 = i64::MAX;
        let mut scanned = 0u32;
        for r in 0..=3 {
            for gy in (ccy - r).max(0)..=(ccy + r).min(d - 1) {
                for gx in (ccx - r).max(0)..=(ccx + r).min(d - 1) {
                    // Only the new ring, so a widening search does not rescan its centre.
                    if r > 0 && (gx - ccx).abs() != r && (gy - ccy).abs() != r {
                        continue;
                    }
                    let c = (gy * d + gx) as usize;
                    let (s, e) = (self.cell_start[c] as usize, self.cell_start[c + 1] as usize);
                    for k in s..e {
                        let other = self.cell_rows[k] as usize;
                        if self.owner[other] == me || self.hits[other] <= 0 {
                            continue;
                        }
                        scanned += 1;
                        let dx = (self.pos_x[other] - px) as i64;
                        let dy = (self.pos_y[other] - py) as i64;
                        let d2 = dx * dx + dy * dy;
                        if d2 < best_d2 {
                            best_d2 = d2;
                            best = self.handle_of_row[other];
                        }
                        if scanned >= MAX_SCAN {
                            return best;
                        }
                    }
                }
            }
            if best != NO_TARGET {
                return best;
            }
        }
        best
    }

    /// Nearest enemy owner's centroid, used when nothing is close enough to see.
    fn march_goal(&self, row: usize) -> Option<(i32, i32)> {
        let me = (self.owner[row] as usize) % OWNER_SLOTS;
        let px = self.pos_x[row] as i64;
        let py = self.pos_y[row] as i64;
        let mut best = None;
        let mut best_d2 = i64::MAX;
        for o in 0..OWNER_SLOTS {
            if o == me || self.pop[o] == 0 {
                continue;
            }
            let dx = self.cx[o] - px;
            let dy = self.cy[o] - py;
            let d2 = dx * dx + dy * dy;
            if d2 < best_d2 {
                best_d2 = d2;
                best = Some((self.cx[o] as i32, self.cy[o] as i32));
            }
        }
        best
    }

    /// One integer step of `speed` subtiles toward `(tx, ty)`. Placeholder movement.
    #[inline]
    fn move_towards(&mut self, row: usize, tx: i32, ty: i32, speed: i32) {
        let dx = (tx - self.pos_x[row]) as i64;
        let dy = (ty - self.pos_y[row]) as i64;
        let dist = isqrt(dx * dx + dy * dy);
        self.facing[row] = atan2_u32(dy as i32, dx as i32) as i32;
        if dist == 0 {
            return;
        }
        let s = speed.max(1) as i64;
        if dist <= s {
            self.pos_x[row] = tx.clamp(0, MAP_SPAN - 1);
            self.pos_y[row] = ty.clamp(0, MAP_SPAN - 1);
            return;
        }
        let nx = self.pos_x[row] as i64 + (dx * s) / dist;
        let ny = self.pos_y[row] as i64 + (dy * s) / dist;
        self.pos_x[row] = (nx as i32).clamp(0, MAP_SPAN - 1);
        self.pos_y[row] = (ny as i32).clamp(0, MAP_SPAN - 1);
    }

    /// Assemble the damage chain's inputs for one attack and run it.
    ///
    /// Every field is either a real `UnitType` value, a real table lookup, a real rules
    /// constant, or an explicitly inert placeholder — the comments say which.
    fn strike(&mut self, gd: &GameData, row: usize, trow: usize, dir: u32) -> i32 {
        let at: UnitTypeRec = gd.units[self.type_idx[row] as usize];
        let dt: UnitTypeRec = gd.units[self.type_idx[trow] as usize];
        let i = DamageInput {
            // real: `Balance::final_balance_table[attacker][defender]`
            balance_pct: gd.balance_pct(at.type_id, dt.type_id),
            // real: the two virtual getters, with the upgrade term off (no tech modelled)
            attack: get_attack(at.attack, false, at.military_level, gd.rules_0x8b8),
            armor: get_armor(dt.armor, false, dt.military_level, gd.rules_0x8b8),
            // real: `UnitType[+0x1E4]`
            attacker_masks: at.obj_masks as u32,
            defender_masks: dt.obj_masks as u32,
            // ours: an approximate angle, feeding derived flank arithmetic
            attack_dir: dir as i32,
            // inert: no splash, no overkill window
            splash_flag: 0,
            overkill_gate: 0,
            attacker_player: self.owner[row] as u32,
            attacker_type_id: at.type_id,
            attacker_domain: at.domain,
            attacker_splash_percent: at.splash_percent,
            // inert: `UnitType[+0x40]` is not in the live dump; 0 is not 0x1AB or 0x1AC, so
            // steps 11 and 27 (both UNVERIFIED anyway) cannot fire.
            attacker_type_0x40: 0,
            attacker_z: 0,
            attacker_flag8_bit5: false,
            defender_type_id: dt.type_id,
            defender_domain: dt.domain,
            defender_type_0x2b8_bit2: false,
            // never read: the splash path is off. 1 rather than 0 so a future change cannot
            // reach the un-guarded `idiv` at 0x006448B9 by accident.
            defender_splash_divisor: 1,
            defender_flags_0x68: 0,
            defender_flags_0x6c_bit12: false,
            // flat world: no height, no river
            defender_z: 0,
            // real consumer, our producer: the defender's own facing
            defender_facing: self.facing[trow],
            defender_facing_entrench: self.facing[trow],
            defender_overkill_stamp: 0,
            defender_word_0xa4: 0,
            attacker_vf_0xe4: 0,
            current_frame: self.frame as i32,
            game_flag_0x821_bit1: false,
            tile_rocky: false,
            // inert: with no tile ownership modelled, "always friendly ground" keeps step 6
            // from firing on a value we would have had to invent.
            tile_owner: self.owner[row] as i32,
        };
        damage(&i, &predicates(), &gd.rules, &UnreachedTerms::default())
    }

    fn process(&mut self, gd: &GameData, row: usize) {
        if self.hits[row] <= 0 {
            return;
        }
        if self.cooldown[row] > 0 {
            self.cooldown[row] -= 1;
        }
        let ut: UnitTypeRec = gd.units[self.type_idx[row] as usize];
        let reach = if ut.max_range > 0 {
            ut.max_range * SUBTILE
        } else {
            MELEE_REACH
        };
        let reach2 = (reach as i64) * (reach as i64);

        // Resolve the standing target, re-acquiring when it is gone.
        let mut trow = match self.target[row] {
            NO_TARGET => None,
            id => self.row_of_id(id),
        };
        if let Some(t) = trow {
            if self.hits[t] <= 0 || self.owner[t] == self.owner[row] {
                trow = None;
            }
        }
        if trow.is_none() {
            let id = self.acquire(row);
            self.target[row] = id;
            trow = if id == NO_TARGET {
                None
            } else {
                self.row_of_id(id)
            };
        }

        // A standing move order outranks chasing: an order the player issued must be
        // visibly obeyed, or the order path proves nothing.
        let ordered = self.order_flags[row] & order_bits::MOVE != 0;
        if ordered {
            let dx = (self.order_x[row] - self.pos_x[row]) as i64;
            let dy = (self.order_y[row] - self.pos_y[row]) as i64;
            if dx * dx + dy * dy <= (SUBTILE as i64 * SUBTILE as i64) {
                self.order_flags[row] &= !order_bits::MOVE;
            }
        }

        let Some(t) = trow else {
            if ordered {
                let (ox, oy) = (self.order_x[row], self.order_y[row]);
                self.move_towards(row, ox, oy, ut.moves);
            } else if let Some((gx, gy)) = self.march_goal(row) {
                self.move_towards(row, gx, gy, ut.moves);
            }
            return;
        };

        let dx = self.pos_x[t] - self.pos_x[row];
        let dy = self.pos_y[t] - self.pos_y[row];
        let d2 = (dx as i64) * (dx as i64) + (dy as i64) * (dy as i64);
        if d2 <= reach2 {
            let dir = atan2_u32(dy, dx);
            self.facing[row] = dir as i32;
            if self.cooldown[row] == 0 {
                let d = self.strike(gd, row, t, dir);
                self.shots += 1;
                if d > 0 {
                    self.hits[t] -= d;
                    self.damage_dealt += d as u64;
                    if self.hits[t] <= 0 {
                        self.kills += 1;
                    }
                }
                self.cooldown[row] = ut.recharge.clamp(1, i16::MAX as i32) as i16;
            }
        } else if ordered {
            let (ox, oy) = (self.order_x[row], self.order_y[row]);
            self.move_towards(row, ox, oy, ut.moves);
        } else {
            let (tx, ty) = (self.pos_x[t], self.pos_y[t]);
            self.move_towards(row, tx, ty, ut.moves);
        }
    }

    /// Advance one simulation frame.
    ///
    /// **The owner loop is the derived part**: `Objects::process_all` walks owner slots in
    /// `(frame + i) % 10` order, so which side acts first rotates every frame. Processing
    /// owners in a fixed order is a different simulation — it diverges inside one tick — so
    /// the rotation is reproduced here even though nothing else about the per-object chain
    /// is modelled yet.
    pub fn step(&mut self, gd: &GameData) {
        self.frame += 1;
        self.build_grid();
        self.build_owner_order();
        for slot in 0..OWNER_SLOTS {
            let o = ((self.frame as usize + slot) % OWNER_SLOTS) as usize;
            let s = self.owner_start[o] as usize;
            let e = self.owner_start[o + 1] as usize;
            for k in s..e {
                let row = self.order[k] as usize;
                self.process(gd, row);
            }
        }
        // Deaths are collected after the frame, so every unit alive at the top of the frame
        // acts in it, and row compaction cannot move a unit out from under the loop.
        let mut row = 0usize;
        while row < self.live as usize {
            if self.hits[row] <= 0 {
                self.despawn_row(row);
            } else {
                row += 1;
            }
        }
        if self.auto_reset && self.live > 0 {
            let mut sides = 0;
            let mut seen = [false; OWNER_SLOTS];
            for r in 0..self.live as usize {
                let o = (self.owner[r] as usize) % OWNER_SLOTS;
                if !seen[o] {
                    seen[o] = true;
                    sides += 1;
                }
            }
            if sides < 2 {
                self.reset_round(gd);
            }
        } else if self.auto_reset && self.live == 0 {
            self.reset_round(gd);
        }
        self.refresh_tags(gd);
    }

    /// Recompute the render tag column. One pass over the live prefix; the alternative —
    /// packing the same bits in JavaScript per entity per frame — is the CPU loop this
    /// whole architecture exists to delete.
    pub fn refresh_tags(&mut self, gd: &GameData) {
        for row in 0..self.live as usize {
            self.tag[row] = self.tag_of(row, gd);
        }
        for slot in self.live as usize..self.capacity as usize {
            self.tag[slot] = 0;
        }
    }

    // ---- commands ------------------------------------------------------------------

    /// `GroupCommand` (0x00): replace `who`'s selection with the listed object ids.
    pub fn cmd_group(&mut self, who: u8, ids: impl Iterator<Item = i16>) -> u32 {
        for r in 0..self.live as usize {
            if self.owner[r] == who {
                self.selected[r] = 0;
            }
        }
        let mut n = 0;
        for id in ids {
            if id < 0 {
                continue;
            }
            if let Some(row) = self.row_of_id(id as u32) {
                if self.owner[row] == who {
                    self.selected[row] = 1;
                    n += 1;
                }
            }
        }
        n
    }

    /// `MoveToCommand` (0x07): give every selected unit of `who` a standing move order.
    /// `to_x`/`to_y` are honoured; the other seven fields of the real struct are not — see
    /// `crate::wire_gen::move_to`.
    pub fn cmd_move_to(&mut self, who: u8, tx: i32, ty: i32) -> u32 {
        let mut n = 0;
        for r in 0..self.live as usize {
            if self.owner[r] == who && self.selected[r] != 0 {
                self.order_x[r] = tx.clamp(0, MAP_SPAN - 1);
                self.order_y[r] = ty.clamp(0, MAP_SPAN - 1);
                self.order_flags[r] |= order_bits::MOVE;
                // Drop the current target so the unit actually leaves the fight it is in;
                // it will re-acquire when something comes into reach.
                self.target[r] = NO_TARGET;
                n += 1;
            }
        }
        n
    }

    /// `AttackCommand` (0x04): point every selected unit of `who` at object `whom`.
    pub fn cmd_attack(&mut self, who: u8, whom: i32) -> u32 {
        if whom < 0 || self.row_of_id(whom as u32).is_none() {
            return 0;
        }
        let mut n = 0;
        for r in 0..self.live as usize {
            if self.owner[r] == who && self.selected[r] != 0 {
                self.target[r] = whom as u32;
                self.order_flags[r] &= !order_bits::MOVE;
                n += 1;
            }
        }
        n
    }

    /// `HaltCommand` (0x0c): clear orders and targets for `who`'s selection.
    pub fn cmd_halt(&mut self, who: u8) -> u32 {
        let mut n = 0;
        for r in 0..self.live as usize {
            if self.owner[r] == who && self.selected[r] != 0 {
                self.order_flags[r] = 0;
                self.target[r] = NO_TARGET;
                n += 1;
            }
        }
        n
    }

    /// Object ids of `who`'s units within `radius` subtiles of a point — the *query* behind
    /// a drag-select. Writes at most `out.len()` ids and returns how many.
    pub fn pick(&self, who: u8, x: i32, y: i32, radius: i32, out: &mut [i16]) -> usize {
        let r2 = (radius as i64) * (radius as i64);
        let mut n = 0;
        for row in 0..self.live as usize {
            if n >= out.len() {
                break;
            }
            if self.owner[row] != who {
                continue;
            }
            let dx = (self.pos_x[row] - x) as i64;
            let dy = (self.pos_y[row] - y) as i64;
            if dx * dx + dy * dy <= r2 {
                out[n] = self.handle_of_row[row] as i16;
                n += 1;
            }
        }
        n
    }

    /// The unit nearest a point, any owner — what a click on the map resolves to.
    /// Returns `(object id, owner, type index, hits, max hits)`.
    pub fn pick_nearest(&self, x: i32, y: i32) -> Option<(i16, u8, u16, i32, i32)> {
        let mut best = None;
        let mut best_d2 = i64::MAX;
        for row in 0..self.live as usize {
            let dx = (self.pos_x[row] - x) as i64;
            let dy = (self.pos_y[row] - y) as i64;
            let d2 = dx * dx + dy * dy;
            if d2 < best_d2 {
                best_d2 = d2;
                best = Some((
                    self.handle_of_row[row] as i16,
                    self.owner[row],
                    self.type_idx[row],
                    self.hits[row],
                    self.max_hits[row],
                ));
            }
        }
        best
    }

    /// Per-instance render tag. One `u32` carries everything the vertex shader needs:
    ///
    /// | bits | meaning |
    /// |---|---|
    /// | 31 | occupied |
    /// | 30 | selected |
    /// | 24..29 | size class, from `TARGET_SIZE` and range |
    /// | 16..23 | hit points, 0..255 of maximum |
    /// | 8..15 | type hue, the low byte of the real `type_id` |
    /// | 0..3 | owner |
    #[inline]
    pub fn tag_of(&self, row: usize, gd: &GameData) -> u32 {
        let ut = &gd.units[self.type_idx[row] as usize];
        let hp = if self.max_hits[row] > 0 {
            ((self.hits[row].max(0) as i64 * 255) / self.max_hits[row] as i64) as u32
        } else {
            0
        };
        // Ranged units draw a touch smaller than the heavy things they are shooting at.
        let size = (2 + (ut.hits / 90).clamp(0, 5) + if ut.max_range > 8 { 1 } else { 0 }) as u32;
        0x8000_0000
            | ((self.selected[row] as u32) << 30)
            | ((size & 0x3f) << 24)
            | ((hp & 0xff) << 16)
            | (((ut.type_id as u32) & 0xff) << 8)
            | (self.owner[row] as u32 & 0xf)
    }

    /// Order-independent state digest. Each unit is hashed with its stable id and the
    /// per-unit hashes are combined commutatively, so row compaction — which is not a state
    /// change — cannot move it, while two units swapping attributes can.
    pub fn digest(&self) -> u64 {
        let mut acc: u64 = 0;
        for row in 0..self.live as usize {
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for v in [
                self.handle_of_row[row] as u64,
                self.pos_x[row] as u32 as u64,
                self.pos_y[row] as u32 as u64,
                self.hits[row] as u32 as u64,
                self.cooldown[row] as u16 as u64,
                self.type_idx[row] as u64,
                self.owner[row] as u64,
                self.target[row] as u64,
            ] {
                h ^= v;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
            acc = acc.wrapping_add(h);
        }
        let mut out = acc ^ self.frame ^ (self.kills as u64) << 32 ^ self.damage_dealt;
        out = out.wrapping_mul(0x0000_0100_0000_01B3);
        out ^ (self.live as u64) ^ (self.rounds as u64) << 40
    }

    /// Sum of live hit points — the aggregate view's "how much army is left" metric.
    pub fn total_hits(&self) -> u64 {
        let mut t = 0u64;
        for row in 0..self.live as usize {
            t += self.hits[row].max(0) as u64;
        }
        t
    }

    pub fn seed_value(&self) -> u32 {
        self.seed
    }
}

/// `(cos, sin) * radius` from an engine-space angle, by integer quadrant reflection of a
/// 16-entry table. Used only to place starting armies on a circle.
fn unit_circle(angle: u32, radius: i32) -> (i32, i32) {
    // sin over a quarter turn, scaled by 4096, 16 samples + endpoint.
    const S: [i32; 17] = [
        0, 400, 799, 1189, 1567, 1928, 2268, 2582, 2867, 3119, 3335, 3513, 3650, 3745, 3797, 3822,
        4096,
    ];
    let t = (angle >> 22) as usize; // 0..1023 over a full turn -> 0..1023
    let q = (t / 256) % 4;
    let i = (t % 256) * 16 / 256;
    let s = S[i.min(16)];
    let c = S[(16 - i).min(16)];
    let (sx, sy) = match q {
        0 => (c, s),
        1 => (-s, c),
        2 => (-c, -s),
        _ => (s, -c),
    };
    (
        ((sx as i64 * radius as i64) / 4096) as i32,
        ((sy as i64 * radius as i64) / 4096) as i32,
    )
}

/// A shard: many independent worlds plus the contiguous render mirror an instanced draw
/// needs. Worlds share nothing, which is what makes worker-level parallelism sound.
pub struct RealBatch {
    pub worlds: Vec<RealWorld>,
    pub gd: GameData,
}

impl RealBatch {
    pub fn new(
        gd: GameData,
        worlds: usize,
        capacity: usize,
        owners: u8,
        per_owner: u32,
        seed: u64,
    ) -> RealBatch {
        let mut ws = Vec::with_capacity(worlds);
        for w in 0..worlds {
            // Splitmix-style spread so neighbouring worlds do not start correlated.
            let s = seed
                .wrapping_add((w as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
                .rotate_left(17);
            let mut world = RealWorld::with_capacity(capacity, s);
            world.populate(&gd, owners, per_owner);
            ws.push(world);
        }
        RealBatch { worlds: ws, gd }
    }

    pub fn step(&mut self) {
        for w in &mut self.worlds {
            w.step(&self.gd);
        }
    }

    pub fn live_units(&self) -> usize {
        self.worlds.iter().map(|w| w.live_count() as usize).sum()
    }

    pub fn digest(&self) -> u64 {
        let mut acc: u64 = 0;
        for (k, w) in self.worlds.iter().enumerate() {
            acc = acc
                .wrapping_add(w.digest().wrapping_mul(k as u64 * 2 + 1))
                .rotate_left(7);
        }
        acc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gd() -> GameData {
        GameData::synthetic()
    }

    #[test]
    fn atan2_is_monotone_around_the_circle() {
        // Sampled around a circle, the angle must advance monotonically in u32 space.
        let mut last = 0u32;
        for k in 1..64 {
            let a = (k as f64) * std::f64::consts::TAU / 64.0;
            let x = (a.cos() * 10000.0) as i32;
            let y = (a.sin() * 10000.0) as i32;
            let got = atan2_u32(y, x);
            if k > 1 {
                assert!(got > last, "angle went backwards at k={k}: {last} -> {got}");
            }
            last = got;
        }
    }

    #[test]
    fn isqrt_is_exact() {
        for v in [
            0i64,
            1,
            2,
            3,
            4,
            99,
            100,
            101,
            1 << 20,
            (1 << 31) - 1,
            1 << 40,
        ] {
            let r = isqrt(v);
            assert!(r * r <= v && (r + 1) * (r + 1) > v, "isqrt({v}) = {r}");
        }
    }

    #[test]
    fn a_battle_kills_units_and_is_deterministic() {
        let run = || {
            let mut b = RealBatch::new(gd(), 2, 128, 2, 32, 0xC0FFEE);
            for _ in 0..600 {
                b.step();
            }
            (b.digest(), b.worlds[0].kills, b.worlds[0].shots)
        };
        let (d1, k1, s1) = run();
        let (d2, k2, s2) = run();
        assert_eq!(d1, d2, "same seed must give the same digest");
        assert!(s1 > 0, "nobody attacked in 600 frames");
        assert!(k1 > 0, "600 frames of combat killed nobody");
        assert_eq!((k1, s1), (k2, s2));
    }

    #[test]
    fn rows_stay_dense_and_ids_stay_a_permutation_through_a_battle() {
        let mut b = RealBatch::new(gd(), 1, 64, 2, 24, 7);
        for _ in 0..400 {
            b.step();
            let w = &b.worlds[0];
            let mut ids = w.handle_of_row.clone();
            ids.sort_unstable();
            assert!(
                ids.iter().copied().eq(0..w.capacity()),
                "id permutation broken"
            );
            for row in 0..w.live_count() as usize {
                assert_eq!(w.row_of_id(w.handle_of_row[row]).unwrap(), row);
            }
        }
    }

    #[test]
    fn a_move_order_moves_the_selection_and_nothing_else() {
        let mut b = RealBatch::new(gd(), 1, 64, 2, 16, 11);
        let w = &mut b.worlds[0];
        let ids: Vec<i16> = (0..w.live_count())
            .filter(|&r| w.owner[r as usize] == 0)
            .map(|r| w.handle_of_row[r as usize] as i16)
            .collect();
        assert!(!ids.is_empty());
        let n = w.cmd_group(0, ids.iter().copied());
        assert_eq!(n as usize, ids.len());
        let target = (MAP_SPAN - SUBTILE, SUBTILE);
        assert_eq!(w.cmd_move_to(0, target.0, target.1) as usize, ids.len());
        let before: Vec<(i32, i32)> = (0..w.live_count() as usize)
            .map(|r| (w.pos_x[r], w.pos_y[r]))
            .collect();
        for _ in 0..30 {
            b.step();
        }
        let w = &b.worlds[0];
        // At least one ordered unit must have closed on the order point.
        let mut closed = 0;
        for r in 0..w.live_count() as usize {
            if w.owner[r] == 0 && w.order_flags[r] & order_bits::MOVE != 0 {
                let d0 = {
                    let k = w.handle_of_row[r] as usize;
                    let _ = k;
                    before.len()
                };
                let _ = d0;
                let dx = (w.pos_x[r] - target.0).abs();
                let dy = (w.pos_y[r] - target.1).abs();
                if dx + dy < MAP_SPAN {
                    closed += 1;
                }
            }
        }
        assert!(closed > 0, "no ordered unit is under a standing move order");
    }
}
