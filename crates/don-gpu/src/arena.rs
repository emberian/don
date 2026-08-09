//! A flat multi-world entity arena, and four ways to run the same tick over it.
//!
//! # The claim under test
//!
//! "Order execution has to stay CPU-bound because it is branchy." That is false as stated:
//! **branchiness is a property of the layout, not of the problem.** The engine's per-unit
//! step is a virtual call through the `Order` hierarchy — 28 archetypes (see
//! [`crate::orders`]) — and if you port that shape directly you get one indirect call and one
//! mispredict per entity per tick, forever. Grouped by order *first*, the identical
//! computation becomes 28 dense homogeneous kernels with no dynamic dispatch inside any of
//! them, and the dispatch cost collapses to one counting-sort pass over the entity list.
//!
//! This module implements both, plus the batch-wide and per-world variants of the grouped
//! form, and asserts all of them produce **the same bits**. Which one is faster is a
//! measurement, in `docs/tracks/gpu-batch.md`, not an assumption.
//!
//! # Layout
//!
//! One flat column per attribute, spanning **every world in the batch**: slot
//! `world * capacity + row`. Worlds are contiguous and enumerated world-major, which is what
//! makes a single stable counting-sort pass on the order digit produce `(order, world, row)`
//! ordering — see [`crate::partition`]. This is the Madrona/JUX column-store shape, and it is
//! also what an observation tensor wants, so there is no gather on export.
//!
//! It is deliberately *not* [`crate::World`]: that type is the authoritative per-world store
//! and other lanes build on it. This is the cross-world layout experiment, self-contained so
//! it can be measured against the per-world one without disturbing it.
//!
//! # Mechanics: PLACEHOLDER, and marked as such
//!
//! No Rise of Nations order behaviour has been derived. The five [`Shape`]s are structural
//! stand-ins chosen to cover the scheduling cases (see [`crate::orders::Shape`]); the
//! constants are invented. Nothing here is a fidelity claim, and no number measured on it is
//! a claim about simulation speed — it is a claim about *this layout's* cost of moving the
//! state a tick has to move.

use crate::orders::{Order, OrderParams, Shape, ORDER_COUNT, ORDER_PARAMS};
use crate::partition::Partition;
use crate::reduce::{
    accumulate_i32, accumulate_i32_parallel, accumulate_i32_reference, draw_from_pools,
    draw_from_pools_reference, ReduceScratch,
};

/// PLACEHOLDER map extent in 1/192-tile units (`don_sim::MAP_SPAN`, duplicated rather than
/// depended on so this crate stays free of a sim-core dependency while that lane moves).
/// The real map sizes are underived; this only gives the placeholder integrator a wrap point.
pub const MAP_SPAN: i32 = 256 * 192;

/// Owner slots per world. `Objects::process_all` rotates owner-slot order every frame as
/// `(frame + i) % 10` [measured, `docs/derivation/architecture.md`], so ten is the engine's
/// own number of owner slots. Used here only to size the per-owner resource pools.
pub const OWNERS: usize = 10;

/// How orders are distributed over the population.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mix {
    /// Every archetype equally likely — the maximally divergent case, and the one a
    /// branch predictor cannot help with.
    Uniform,
    /// Skewed the way an RTS army actually is: mostly idle and moving, a minority fighting,
    /// a long tail of everything else. This is the case that decides whether partitioning is
    /// worth it in practice, because a skewed mix is *easier* for a branch predictor.
    Skewed,
    /// One archetype for the whole batch. The best possible case for the branchy path (a
    /// perfectly predicted branch) and therefore the honest lower bound on any speedup.
    Single(Order),
}

/// Which implementation of the per-entity order step to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhaseA {
    /// One indirect call per entity through a 28-entry function-pointer table. This is the
    /// direct port of the engine's virtual `Order` dispatch.
    Virtual,
    /// One `match` per entity on the order's shape. The "obvious Rust rewrite" — strictly
    /// kinder to the predictor than the virtual call, so a fair floor.
    Match,
    /// Partition the whole batch by order once, then run 28 dense kernels.
    Partitioned,
    /// Partition **each world separately** and run 28 kernels per world. Same answer, but
    /// buckets are `worlds` times smaller — this is the measurement that says whether
    /// interleaving worlds into one partition actually pays, rather than assuming it.
    PartitionedPerWorld,
    /// [`PhaseA::Partitioned`], with the buckets handed to `n` worker threads.
    PartitionedParallel(usize),
}

/// How the many-to-one conflicts are resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolve {
    /// The sequential reference: `out[t] += v` in slot order, and the sequential pool draw.
    /// **This defines the answer.**
    Sequential,
    /// Sort by key, segmented reduce / segmented scan. Same bits, parallelisable.
    Segmented,
    /// [`Resolve::Segmented`] across `n` threads.
    SegmentedParallel(usize),
}

#[derive(Clone, Copy, Debug)]
pub struct StepPlan {
    pub phase_a: PhaseA,
    pub resolve: Resolve,
}

impl StepPlan {
    /// The definition of a tick: no partitioning, no reordering.
    pub const REFERENCE: StepPlan = StepPlan {
        phase_a: PhaseA::Virtual,
        resolve: Resolve::Sequential,
    };
}

/// Mutable view of the columns a phase-A kernel may touch.
///
/// Passing this explicitly (rather than `&mut Arena`) is what lets the bucket kernels be
/// plain functions over slices — which is what the autovectoriser needs, and what makes the
/// function-pointer table possible.
pub struct Cols<'a> {
    pub pos_x: &'a mut [i32],
    pub pos_y: &'a mut [i32],
    pub tgt_x: &'a [i32],
    pub tgt_y: &'a [i32],
    pub hits: &'a [i32],
    pub cooldown: &'a mut [i16],
    pub owner: &'a [u8],
    pub target: &'a [u32],
    pub seed: &'a mut [u32],
    /// Per-slot strike contribution: whom to hit, and how hard (0 = did not fire).
    pub strike_target: &'a mut [u32],
    pub strike_damage: &'a mut [i32],
    /// Per-slot pool draw: which pool, and how much is wanted (0 = did not draw).
    pub pool_key: &'a mut [u32],
    pub want: &'a mut [i32],
    /// Rows per world, so a slot can find its world.
    pub cap: u32,
    /// Offsets that place the ids this view emits into the coordinate space its **resolve
    /// step** will use. Entity state itself is position-independent (`target` is a row within
    /// a world), so these are the only things that cross a slice boundary:
    ///
    /// - [`Arena::run_parallel`] resolves per slice, so a worker emits *slice-local* ids and
    ///   both are `(0, its first world)`.
    /// - [`PhaseA::PartitionedParallel`] resolves arena-wide afterwards, so a worker must
    ///   emit *global* ids and both are its true offsets.
    ///
    /// Applying them inside the kernel rather than in a fix-up pass afterwards is deliberate:
    /// the fix-up version was a bug that rebased every slot instead of the ones that wrote a
    /// key that frame, so stale keys drifted out of range over successive frames.
    pub slot_base: u32,
    /// See [`Cols::slot_base`]. Used only by `pool_key`.
    pub world_base: u32,
}

// ---------------------------------------------------------------------------------------
// the five kernels, once each, in branch-free form
// ---------------------------------------------------------------------------------------
//
// Every one is written so that the only per-entity decision left is a *select*, never a
// jump. Partitioning removes the order branch; these remove what is left. `(cond as i32)`
// is a 0/1 with no branch on every backend we target, and the negation trick
// `-(c as i32)` builds an all-ones mask for the i32 selects.

#[inline(always)]
fn wrap_span(v: i32) -> i32 {
    // Same wrap the placeholder integrator in `world.rs` uses, spelled branch-free.
    let lt = -((v < 0) as i32);
    let ge = -((v >= MAP_SPAN) as i32);
    v.wrapping_add(MAP_SPAN & lt).wrapping_sub(MAP_SPAN & ge)
}

#[inline(always)]
fn k_nothing(_c: &mut Cols, _s: u32, _p: OrderParams) {}

#[inline(always)]
fn k_steer(c: &mut Cols, s: u32, p: OrderParams) {
    let i = s as usize;
    let dx = (c.tgt_x[i] - c.pos_x[i]).signum() * p.step;
    let dy = (c.tgt_y[i] - c.pos_y[i]).signum() * p.step;
    c.pos_x[i] = wrap_span(c.pos_x[i].wrapping_add(dx));
    c.pos_y[i] = wrap_span(c.pos_y[i].wrapping_add(dy));
}

#[inline(always)]
fn k_strike(c: &mut Cols, s: u32, p: OrderParams) {
    let i = s as usize;
    let fire = (c.cooldown[i] <= 0) as i32;
    let mask = -fire;
    c.strike_damage[i] = (p.amount + c.owner[i] as i32) & mask;
    // `target` is a row *within the entity's own world*, so the slot it names is built from
    // this entity's own world index — no lookup, and correct in any view. Select it without a
    // branch: fire -> the real target, else self (harmless, the damage is zero).
    let tgt = c.slot_base + (s / c.cap) * c.cap + c.target[i];
    let me = c.slot_base + s;
    c.strike_target[i] = (tgt & (mask as u32)) | (me & (!mask as u32));
    c.cooldown[i] = if fire != 0 {
        p.period
    } else {
        c.cooldown[i] - 1
    };
}

#[inline(always)]
fn k_draw(c: &mut Cols, s: u32, p: OrderParams) {
    let i = s as usize;
    let fire = (c.cooldown[i] <= 0) as i32;
    c.want[i] = p.amount & -fire;
    c.pool_key[i] = (c.world_base + s / c.cap) * OWNERS as u32 + c.owner[i] as u32;
    c.cooldown[i] = if fire != 0 {
        p.period
    } else {
        c.cooldown[i] - 1
    };
}

#[inline(always)]
fn k_churn(c: &mut Cols, s: u32, p: OrderParams) {
    // Data-dependent trip count: this is the case partitioning does **not** fix, and it is
    // in the mix on purpose so the measurement is not flattering.
    let i = s as usize;
    let n = (c.hits[i] & 7) + p.amount;
    let mut r = c.seed[i];
    for _ in 0..n {
        r = r.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    }
    c.seed[i] = r;
}

#[inline(always)]
fn run_shape(c: &mut Cols, s: u32, p: OrderParams) {
    match p.shape {
        Shape::Nothing => k_nothing(c, s, p),
        Shape::Steer => k_steer(c, s, p),
        Shape::Strike => k_strike(c, s, p),
        Shape::Draw => k_draw(c, s, p),
        Shape::Churn => k_churn(c, s, p),
    }
}

/// One entry per order, with the order's constants baked in by monomorphisation. This is the
/// analogue of the engine's vtable slot.
fn vslot<const K: usize>(c: &mut Cols, s: u32) {
    run_shape(c, s, ORDER_PARAMS[K]);
}

type VFn = fn(&mut Cols, u32);

/// The 28-entry jump table. Indexing it and calling through it is exactly the cost the
/// partitioned path is trying to delete.
static VTABLE: [VFn; ORDER_COUNT] = [
    vslot::<0>,
    vslot::<1>,
    vslot::<2>,
    vslot::<3>,
    vslot::<4>,
    vslot::<5>,
    vslot::<6>,
    vslot::<7>,
    vslot::<8>,
    vslot::<9>,
    vslot::<10>,
    vslot::<11>,
    vslot::<12>,
    vslot::<13>,
    vslot::<14>,
    vslot::<15>,
    vslot::<16>,
    vslot::<17>,
    vslot::<18>,
    vslot::<19>,
    vslot::<20>,
    vslot::<21>,
    vslot::<22>,
    vslot::<23>,
    vslot::<24>,
    vslot::<25>,
    vslot::<26>,
    vslot::<27>,
];

/// A whole bucket of one archetype, as one call. `p` is a compile-time-ish constant from the
/// caller's point of view (it is loop-invariant), so the shape match hoists out of the loop
/// and the body is straight-line integer code over a dense index list.
fn run_bucket(c: &mut Cols, slots: &[u32], p: OrderParams) {
    match p.shape {
        Shape::Nothing => {}
        Shape::Steer => {
            for &s in slots {
                k_steer(c, s, p);
            }
        }
        Shape::Strike => {
            for &s in slots {
                k_strike(c, s, p);
            }
        }
        Shape::Draw => {
            for &s in slots {
                k_draw(c, s, p);
            }
        }
        Shape::Churn => {
            for &s in slots {
                k_churn(c, s, p);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// the arena
// ---------------------------------------------------------------------------------------

/// Flat structure-of-arrays over `worlds x capacity` entity slots.
#[derive(Clone)]
pub struct Arena {
    worlds: u32,
    cap: u32,
    live: Vec<u32>,

    pos_x: Vec<i32>,
    pos_y: Vec<i32>,
    tgt_x: Vec<i32>,
    tgt_y: Vec<i32>,
    hits: Vec<i32>,
    carried: Vec<i32>,
    cooldown: Vec<i16>,
    owner: Vec<u8>,
    target: Vec<u32>,
    seed: Vec<u32>,
    order: Vec<u8>,

    /// `worlds * OWNERS` resource pools.
    pool: Vec<i32>,

    // per-frame contribution columns, indexed by slot
    strike_target: Vec<u32>,
    strike_damage: Vec<i32>,
    pool_key: Vec<u32>,
    want: Vec<i32>,
    got: Vec<i32>,
    damage: Vec<i32>,

    /// Live slots in world-major order; rebuilt only when the population changes.
    live_slots: Vec<u32>,
    part: Partition,
    scratch: ReduceScratch,

    pub frame: u64,
}

impl Arena {
    pub fn new(worlds: u32, cap: u32) -> Arena {
        let n = (worlds as usize) * (cap as usize);
        Arena {
            worlds,
            cap,
            live: vec![0; worlds as usize],
            pos_x: vec![0; n],
            pos_y: vec![0; n],
            tgt_x: vec![0; n],
            tgt_y: vec![0; n],
            hits: vec![0; n],
            carried: vec![0; n],
            cooldown: vec![0; n],
            owner: vec![0; n],
            target: vec![0; n],
            seed: vec![1; n],
            order: vec![0; n],
            pool: vec![0; worlds as usize * OWNERS],
            strike_target: vec![0; n],
            strike_damage: vec![0; n],
            pool_key: vec![0; n],
            want: vec![0; n],
            got: vec![0; n],
            damage: vec![0; n],
            live_slots: Vec::with_capacity(n),
            part: Partition::new(ORDER_COUNT),
            scratch: ReduceScratch::new(),
            frame: 0,
        }
    }

    pub fn worlds(&self) -> u32 {
        self.worlds
    }
    pub fn capacity(&self) -> u32 {
        self.cap
    }
    pub fn live_count(&self) -> u64 {
        self.live.iter().map(|&l| l as u64).sum()
    }
    pub fn total_slots(&self) -> usize {
        (self.worlds as usize) * (self.cap as usize)
    }

    /// Fill every world with `per_world` entities, deterministically.
    ///
    /// Seeded by an LCG with the engine's own constants (`s <- s*1664525 + 1013904223`,
    /// [measured] in `Random::get` `0x00a39cf0`) — used here only as a reproducible fixture
    /// generator, **not** as a claim that the engine seeds anything this way.
    pub fn populate(&mut self, per_world: u32, mix: Mix, seed: u64) {
        let per = per_world.min(self.cap);
        let mut s = (seed as u32) | 1;
        // **Take the high bits.** An LCG with a power-of-two modulus has period 2^k in its
        // low k bits, so `rnd() % 28` — which is `% 4` interleaved with `% 7` — reached only
        // **14 of the 28** archetypes, and `rnd() % 16` produced a 16-long repeating cooldown
        // pattern. The first run of this benchmark measured a "uniform" mix that was nothing
        // of the sort. Shifting first costs nothing and makes the fixture mean what it says.
        let mut rnd = move || {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            s >> 8
        };
        self.live_slots.clear();
        for w in 0..self.worlds {
            self.live[w as usize] = per;
            for row in 0..per {
                let slot = (w * self.cap + row) as usize;
                let r = rnd();
                self.pos_x[slot] = (r % MAP_SPAN as u32) as i32;
                self.pos_y[slot] = (rnd() % MAP_SPAN as u32) as i32;
                self.tgt_x[slot] = (rnd() % MAP_SPAN as u32) as i32;
                self.tgt_y[slot] = (rnd() % MAP_SPAN as u32) as i32;
                self.hits[slot] = 100 + (rnd() % 400) as i32;
                self.carried[slot] = 0;
                self.cooldown[slot] = (rnd() % 16) as i16;
                self.owner[slot] = (rnd() % OWNERS as u32) as u8;
                self.seed[slot] = rnd() | 1;
                // Targets stay inside the world: cross-world interaction would not be a
                // simulation, it would be a bug. Stored as a **row within the world**, which
                // is what makes an entity's state position-independent and therefore sliceable.
                self.target[slot] = rnd() % per.max(1);
                self.order[slot] = pick_order(&mut rnd, mix);
                self.live_slots.push(slot as u32);
            }
            // Deliberately small: a pool that never runs dry never exercises the clamped
            // segmented scan, which is the whole reason [`crate::reduce`] has one.
            for k in 0..OWNERS {
                self.pool[w as usize * OWNERS + k] = 20 + (rnd() % 400) as i32;
            }
        }
        self.strike_damage.fill(0);
        self.want.fill(0);
    }

    /// Histogram of live entities by order archetype.
    pub fn order_histogram(&self) -> [u32; ORDER_COUNT] {
        let mut h = [0u32; ORDER_COUNT];
        for &s in &self.live_slots {
            h[self.order[s as usize] as usize] += 1;
        }
        h
    }

    /// Advance one frame under `plan`. Every plan must produce identical state; the tests
    /// assert it and [`Arena::digest`] is how.
    pub fn step(&mut self, plan: StepPlan) {
        // See the same reset inside `run_parallel`: zero payloads, in-range ids.
        self.strike_damage.fill(0);
        self.want.fill(0);
        self.strike_target.fill(0);
        self.pool_key.fill(0);
        self.phase_a(plan.phase_a);
        self.resolve(plan.resolve);
        self.apply();
        self.frame += 1;
    }

    /// Advance every world by `frames`, spawning workers **once for the whole run**.
    ///
    /// This exists because [`Arena::step`] with `PhaseA::PartitionedParallel` was measured at
    /// **0.03x** of single-threaded at 64 worlds — it pays a thread spawn+join per frame, and
    /// `docs/derivation/simd-batch.md` already measured an empty 12-worker
    /// `std::thread::scope` at 126.6 µs against a frame that costs microseconds. Amortising
    /// the pool over the whole rollout is the fix, and it is the same trick
    /// `don_sim::Batch::run_parallel` uses.
    ///
    /// Correctness rests on worlds being independent: targets are rows *within* a world and
    /// pools are per world, so a worker owning a contiguous world range owns every byte it
    /// writes. No locks, no atomics, and the result is identical to `frames` calls to
    /// [`Arena::step`] at any thread count — `run_parallel_matches_stepping` asserts it.
    ///
    /// Each worker runs the **full** frame (phase A, resolve, apply) on its own slice, so
    /// there is no per-frame barrier at all.
    pub fn run_parallel(&mut self, frames: usize, threads: usize, plan: StepPlan) {
        let worlds = self.worlds as usize;
        if frames == 0 {
            return;
        }
        if threads <= 1 || worlds < 2 {
            for _ in 0..frames {
                self.step(plan);
            }
            return;
        }
        let cap = self.cap as usize;
        let capw = self.cap;
        let per = worlds.div_ceil(threads);

        let mut px: &mut [i32] = &mut self.pos_x;
        let mut py: &mut [i32] = &mut self.pos_y;
        let mut tx: &[i32] = &self.tgt_x;
        let mut ty: &[i32] = &self.tgt_y;
        let mut hp: &mut [i32] = &mut self.hits;
        let mut ca: &mut [i32] = &mut self.carried;
        let mut cd: &mut [i16] = &mut self.cooldown;
        let mut ow: &[u8] = &self.owner;
        let mut tg: &[u32] = &self.target;
        let mut sd: &mut [u32] = &mut self.seed;
        let mut od: &[u8] = &self.order;
        let mut st: &mut [u32] = &mut self.strike_target;
        let mut dm: &mut [i32] = &mut self.strike_damage;
        let mut pk: &mut [u32] = &mut self.pool_key;
        let mut wa: &mut [i32] = &mut self.want;
        let mut go: &mut [i32] = &mut self.got;
        let mut da: &mut [i32] = &mut self.damage;
        let mut pl: &mut [i32] = &mut self.pool;
        let live = &self.live;

        std::thread::scope(|scope| {
            let mut lo = 0usize;
            while lo < worlds {
                let hi = (lo + per).min(worlds);
                let n = (hi - lo) * cap;
                let np = (hi - lo) * OWNERS;
                let (px0, r) = px.split_at_mut(n);
                px = r;
                let (py0, r) = py.split_at_mut(n);
                py = r;
                let (tx0, r) = tx.split_at(n);
                tx = r;
                let (ty0, r) = ty.split_at(n);
                ty = r;
                let (hp0, r) = hp.split_at_mut(n);
                hp = r;
                let (ca0, r) = ca.split_at_mut(n);
                ca = r;
                let (cd0, r) = cd.split_at_mut(n);
                cd = r;
                let (ow0, r) = ow.split_at(n);
                ow = r;
                let (tg0, r) = tg.split_at(n);
                tg = r;
                let (sd0, r) = sd.split_at_mut(n);
                sd = r;
                let (od0, r) = od.split_at(n);
                od = r;
                let (st0, r) = st.split_at_mut(n);
                st = r;
                let (dm0, r) = dm.split_at_mut(n);
                dm = r;
                let (pk0, r) = pk.split_at_mut(n);
                pk = r;
                let (wa0, r) = wa.split_at_mut(n);
                wa = r;
                let (go0, r) = go.split_at_mut(n);
                go = r;
                let (da0, r) = da.split_at_mut(n);
                da = r;
                let (pl0, r) = pl.split_at_mut(np);
                pl = r;

                scope.spawn(move || {
                    let mut part = Partition::new(ORDER_COUNT);
                    let mut scratch = ReduceScratch::new();
                    let mut local_keys: Vec<u32> = vec![0; n];
                    let mut items: Vec<u32> = Vec::with_capacity(n);
                    for w in lo..hi {
                        let wbase = (w - lo) * cap;
                        for row in 0..live[w] as usize {
                            items.push((wbase + row) as u32);
                        }
                    }
                    let shift = (lo * OWNERS) as u32;
                    for _ in 0..frames {
                        // Reset the contribution columns to values that are valid *in this
                        // view*: slot 0 and pool 0 of this slice. Both carry a zero payload,
                        // so a non-emitting entity contributes nothing — but the id still has
                        // to be in range, because the reduce indexes with it.
                        dm0.fill(0);
                        wa0.fill(0);
                        st0.fill(0);
                        pk0.fill(shift);
                        {
                            // Reborrow for the duration of phase A only, so the columns stay
                            // usable for the resolve and apply steps below.
                            let mut c = Cols {
                                pos_x: &mut px0[..],
                                pos_y: &mut py0[..],
                                tgt_x: tx0,
                                tgt_y: ty0,
                                hits: &hp0[..],
                                cooldown: &mut cd0[..],
                                owner: ow0,
                                target: tg0,
                                seed: &mut sd0[..],
                                strike_target: &mut st0[..],
                                strike_damage: &mut dm0[..],
                                pool_key: &mut pk0[..],
                                want: &mut wa0[..],
                                cap: capw,
                                slot_base: 0,
                                world_base: lo as u32,
                            };
                            match plan.phase_a {
                                PhaseA::Virtual => {
                                    for &s in &items {
                                        VTABLE[od0[s as usize] as usize](&mut c, s);
                                    }
                                }
                                PhaseA::Match => {
                                    for &s in &items {
                                        run_shape(
                                            &mut c,
                                            s,
                                            ORDER_PARAMS[od0[s as usize] as usize],
                                        );
                                    }
                                }
                                _ => {
                                    part.build(&items, |s| od0[s as usize]);
                                    for k in 0..ORDER_COUNT {
                                        run_bucket(&mut c, part.bucket(k), ORDER_PARAMS[k]);
                                    }
                                }
                            }
                        }
                        da0.fill(0);
                        go0.fill(0);
                        // Pool ids are global, so rebase them into this slice's pool array.
                        // Slot ids need no rebase: `target` is a row within a world, so the
                        // kernels already emit view-relative slots.
                        for (dst, &k) in local_keys.iter_mut().zip(pk0.iter()) {
                            *dst = k - shift;
                        }
                        match plan.resolve {
                            Resolve::Sequential => {
                                accumulate_i32_reference(st0, dm0, da0);
                                draw_from_pools_reference(&local_keys, wa0, pl0, go0);
                            }
                            _ => {
                                accumulate_i32(st0, dm0, da0, &mut scratch);
                                draw_from_pools(&local_keys, wa0, pl0, go0, &mut scratch);
                            }
                        }
                        for i in 0..n {
                            hp0[i] = (hp0[i] - da0[i]).max(0);
                            ca0[i] = ca0[i].wrapping_add(go0[i]);
                        }
                    }
                });
                lo = hi;
            }
        });
        self.frame += frames as u64;
    }

    fn cols(&mut self) -> Cols<'_> {
        Cols {
            pos_x: &mut self.pos_x,
            pos_y: &mut self.pos_y,
            tgt_x: &self.tgt_x,
            tgt_y: &self.tgt_y,
            hits: &self.hits,
            cooldown: &mut self.cooldown,
            owner: &self.owner,
            target: &self.target,
            seed: &mut self.seed,
            strike_target: &mut self.strike_target,
            strike_damage: &mut self.strike_damage,
            pool_key: &mut self.pool_key,
            want: &mut self.want,
            cap: self.cap,
            slot_base: 0,
            world_base: 0,
        }
    }

    fn phase_a(&mut self, how: PhaseA) {
        match how {
            PhaseA::Virtual => {
                let orders = std::mem::take(&mut self.order);
                let slots = std::mem::take(&mut self.live_slots);
                {
                    let mut c = self.cols();
                    for &s in &slots {
                        VTABLE[orders[s as usize] as usize](&mut c, s);
                    }
                }
                self.order = orders;
                self.live_slots = slots;
            }
            PhaseA::Match => {
                let orders = std::mem::take(&mut self.order);
                let slots = std::mem::take(&mut self.live_slots);
                {
                    let mut c = self.cols();
                    for &s in &slots {
                        run_shape(&mut c, s, ORDER_PARAMS[orders[s as usize] as usize]);
                    }
                }
                self.order = orders;
                self.live_slots = slots;
            }
            PhaseA::Partitioned => {
                let mut part = std::mem::take(&mut self.part);
                {
                    let order = &self.order;
                    part.build(&self.live_slots, |s| order[s as usize]);
                }
                {
                    let mut c = self.cols();
                    for k in 0..ORDER_COUNT {
                        run_bucket(&mut c, part.bucket(k), ORDER_PARAMS[k]);
                    }
                }
                self.part = part;
            }
            PhaseA::PartitionedPerWorld => {
                let mut part = std::mem::take(&mut self.part);
                let slots = std::mem::take(&mut self.live_slots);
                {
                    let order = &self.order;
                    let mut lo = 0usize;
                    // live_slots is world-major and contiguous per world.
                    for w in 0..self.worlds as usize {
                        let hi = lo + self.live[w] as usize;
                        part.build(&slots[lo..hi], |s| order[s as usize]);
                        let mut c = Cols {
                            pos_x: &mut self.pos_x,
                            pos_y: &mut self.pos_y,
                            tgt_x: &self.tgt_x,
                            tgt_y: &self.tgt_y,
                            hits: &self.hits,
                            cooldown: &mut self.cooldown,
                            owner: &self.owner,
                            target: &self.target,
                            seed: &mut self.seed,
                            strike_target: &mut self.strike_target,
                            strike_damage: &mut self.strike_damage,
                            pool_key: &mut self.pool_key,
                            want: &mut self.want,
                            cap: self.cap,
                            slot_base: 0,
                            world_base: 0,
                        };
                        for k in 0..ORDER_COUNT {
                            run_bucket(&mut c, part.bucket(k), ORDER_PARAMS[k]);
                        }
                        lo = hi;
                    }
                }
                self.live_slots = slots;
                self.part = part;
            }
            PhaseA::PartitionedParallel(threads) => self.phase_a_parallel(threads.max(1)),
        }
    }

    /// Parallel phase A.
    ///
    /// Splitting by **world range** rather than by bucket is what keeps this safe without a
    /// single lock or atomic: a world's entities only ever write their own world's slots
    /// (targets are intra-world by construction), so two workers on disjoint world ranges
    /// write disjoint memory. Each worker still partitions its own range by order, so it
    /// keeps the dense-kernel property.
    ///
    /// The alternative — split by *bucket* — would have two workers writing the same world's
    /// columns and would need the split to be by slot anyway. World-range splitting is both
    /// simpler and the one that generalises to a real batch.
    fn phase_a_parallel(&mut self, threads: usize) {
        let worlds = self.worlds as usize;
        if threads <= 1 || worlds < 2 {
            return self.phase_a(PhaseA::Partitioned);
        }
        let cap = self.cap as usize;
        let per = worlds.div_ceil(threads);
        let order = &self.order;
        let tgt_x = &self.tgt_x;
        let tgt_y = &self.tgt_y;
        let hits = &self.hits;
        let owner = &self.owner;
        let target = &self.target;
        let live = &self.live;
        let capw = self.cap;

        // Chop every column into per-world-range pieces so each worker owns its slice.
        let mut px: &mut [i32] = &mut self.pos_x;
        let mut py: &mut [i32] = &mut self.pos_y;
        let mut cd: &mut [i16] = &mut self.cooldown;
        let mut sd: &mut [u32] = &mut self.seed;
        let mut st: &mut [u32] = &mut self.strike_target;
        let mut dm: &mut [i32] = &mut self.strike_damage;
        let mut pk: &mut [u32] = &mut self.pool_key;
        let mut wa: &mut [i32] = &mut self.want;

        std::thread::scope(|scope| {
            let mut lo = 0usize;
            while lo < worlds {
                let hi = (lo + per).min(worlds);
                let n = (hi - lo) * cap;
                let (px0, px1) = px.split_at_mut(n);
                let (py0, py1) = py.split_at_mut(n);
                let (cd0, cd1) = cd.split_at_mut(n);
                let (sd0, sd1) = sd.split_at_mut(n);
                let (st0, st1) = st.split_at_mut(n);
                let (dm0, dm1) = dm.split_at_mut(n);
                let (pk0, pk1) = pk.split_at_mut(n);
                let (wa0, wa1) = wa.split_at_mut(n);
                px = px1;
                py = py1;
                cd = cd1;
                sd = sd1;
                st = st1;
                dm = dm1;
                pk = pk1;
                wa = wa1;
                let base = lo * cap;
                scope.spawn(move || {
                    // The worker sees a local arena whose slot 0 is this range's first slot.
                    // Columns are position-independent, so the worker just runs on its
                    // slice; only `world_base` (for pool ids) crosses the boundary.
                    let mut local = Partition::new(ORDER_COUNT);
                    let mut items: Vec<u32> = Vec::with_capacity(n);
                    for w in lo..hi {
                        let wbase = (w - lo) * cap;
                        for row in 0..live[w] as usize {
                            items.push((wbase + row) as u32);
                        }
                    }
                    local.build(&items, |s| order[base + s as usize]);
                    let mut c = Cols {
                        pos_x: px0,
                        pos_y: py0,
                        tgt_x: &tgt_x[base..base + n],
                        tgt_y: &tgt_y[base..base + n],
                        hits: &hits[base..base + n],
                        cooldown: cd0,
                        owner: &owner[base..base + n],
                        target: &target[base..base + n],
                        seed: sd0,
                        strike_target: st0,
                        strike_damage: dm0,
                        pool_key: pk0,
                        want: wa0,
                        cap: capw,
                        slot_base: base as u32,
                        world_base: lo as u32,
                    };
                    for k in 0..ORDER_COUNT {
                        run_bucket(&mut c, local.bucket(k), ORDER_PARAMS[k]);
                    }
                });
                lo = hi;
            }
        });
    }

    fn resolve(&mut self, how: Resolve) {
        self.damage.fill(0);
        self.got.fill(0);
        match how {
            Resolve::Sequential => {
                accumulate_i32_reference(
                    &self.strike_target,
                    &self.strike_damage,
                    &mut self.damage,
                );
                draw_from_pools_reference(
                    &self.pool_key,
                    &self.want,
                    &mut self.pool,
                    &mut self.got,
                );
            }
            Resolve::Segmented => {
                accumulate_i32(
                    &self.strike_target,
                    &self.strike_damage,
                    &mut self.damage,
                    &mut self.scratch,
                );
                draw_from_pools(
                    &self.pool_key,
                    &self.want,
                    &mut self.pool,
                    &mut self.got,
                    &mut self.scratch,
                );
            }
            Resolve::SegmentedParallel(t) => {
                accumulate_i32_parallel(
                    &self.strike_target,
                    &self.strike_damage,
                    &mut self.damage,
                    &mut self.scratch,
                    t,
                );
                draw_from_pools(
                    &self.pool_key,
                    &self.want,
                    &mut self.pool,
                    &mut self.got,
                    &mut self.scratch,
                );
            }
        }
    }

    /// Apply the resolved totals. One clamp, **after** the accumulation — see
    /// [`crate::reduce`] on why clamping mid-reduce would destroy associativity.
    fn apply(&mut self) {
        let n = self.total_slots();
        for i in 0..n {
            self.hits[i] = (self.hits[i] - self.damage[i]).max(0);
            self.carried[i] = self.carried[i].wrapping_add(self.got[i]);
        }
    }

    /// Order-independent digest of every column a tick can touch.
    ///
    /// Folded per slot and combined with `wrapping_add`, so a permutation of the slots is not
    /// observable — the same discipline as [`crate::World::digest`], and for the same reason:
    /// a layout experiment must not be able to pass by reordering.
    pub fn digest(&self) -> u64 {
        let mut acc = 0u64;
        for &s in &self.live_slots {
            let i = s as usize;
            let mut h = 0xcbf2_9ce4_8422_2325u64;
            for v in [
                s as u64,
                self.pos_x[i] as u32 as u64,
                self.pos_y[i] as u32 as u64,
                self.hits[i] as u32 as u64,
                self.carried[i] as u32 as u64,
                self.cooldown[i] as u16 as u64,
                self.seed[i] as u64,
                self.order[i] as u64,
            ] {
                h ^= v;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
            acc = acc.wrapping_add(h);
        }
        for (k, &p) in self.pool.iter().enumerate() {
            acc = acc.wrapping_add((p as u32 as u64) ^ ((k as u64) << 32));
        }
        acc ^ self.frame
    }
}

fn pick_order(rnd: &mut impl FnMut() -> u32, mix: Mix) -> u8 {
    match mix {
        Mix::Uniform => (rnd() % ORDER_COUNT as u32) as u8,
        Mix::Skewed => {
            // 55% Idle, 20% Move, 10% Attack, 5% Gather, 10% spread over the rest — the
            // shape of a real army roster, which is *kinder* to a branch predictor than
            // uniform and therefore the harder case for partitioning to win.
            let r = rnd() % 100;
            match r {
                0..=54 => Order::Idle as u8,
                55..=74 => Order::Move as u8,
                75..=84 => Order::Attack as u8,
                85..=89 => Order::Gather as u8,
                _ => (rnd() % ORDER_COUNT as u32) as u8,
            }
        }
        Mix::Single(o) => o as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(worlds: u32, cap: u32, per: u32, mix: Mix) -> Arena {
        let mut a = Arena::new(worlds, cap);
        a.populate(per, mix, 0xD0_11);
        a
    }

    const PLANS: &[StepPlan] = &[
        StepPlan {
            phase_a: PhaseA::Virtual,
            resolve: Resolve::Sequential,
        },
        StepPlan {
            phase_a: PhaseA::Match,
            resolve: Resolve::Sequential,
        },
        StepPlan {
            phase_a: PhaseA::Partitioned,
            resolve: Resolve::Sequential,
        },
        StepPlan {
            phase_a: PhaseA::PartitionedPerWorld,
            resolve: Resolve::Sequential,
        },
        StepPlan {
            phase_a: PhaseA::Partitioned,
            resolve: Resolve::Segmented,
        },
        StepPlan {
            phase_a: PhaseA::PartitionedPerWorld,
            resolve: Resolve::Segmented,
        },
        StepPlan {
            phase_a: PhaseA::PartitionedParallel(2),
            resolve: Resolve::Segmented,
        },
        StepPlan {
            phase_a: PhaseA::PartitionedParallel(3),
            resolve: Resolve::SegmentedParallel(3),
        },
        StepPlan {
            phase_a: PhaseA::PartitionedParallel(8),
            resolve: Resolve::SegmentedParallel(8),
        },
        StepPlan {
            phase_a: PhaseA::PartitionedParallel(16),
            resolve: Resolve::SegmentedParallel(16),
        },
        StepPlan {
            phase_a: PhaseA::Virtual,
            resolve: Resolve::SegmentedParallel(4),
        },
    ];

    /// **The hard constraint.** Every execution strategy — indirect dispatch, bucketed,
    /// per-world bucketed, threaded, and every conflict-resolution strategy — must produce
    /// bit-identical state. Anything else is a different simulation wearing the same name.
    #[test]
    fn every_execution_plan_produces_identical_state() {
        for &(worlds, cap, per, mix) in &[
            (1u32, 64u32, 64u32, Mix::Uniform),
            (7, 40, 40, Mix::Uniform),
            (13, 31, 17, Mix::Skewed),
            (5, 8, 3, Mix::Single(Order::Attack)),
            (33, 16, 16, Mix::Skewed),
        ] {
            let mut reference = build(worlds, cap, per, mix);
            for _ in 0..60 {
                reference.step(StepPlan::REFERENCE);
            }
            let want = reference.digest();
            for &plan in PLANS {
                let mut a = build(worlds, cap, per, mix);
                for _ in 0..60 {
                    a.step(plan);
                }
                assert_eq!(
                    a.digest(),
                    want,
                    "{plan:?} diverged at {worlds}w x {cap}cap x {per} ({mix:?})"
                );
            }
        }
    }

    /// `run_parallel` spawns once for the whole rollout and lets each worker run complete
    /// frames on its own world range — no per-frame barrier at all. That is only sound
    /// because worlds are independent, so this is the test that says so at every thread
    /// count, for every phase-A and resolve strategy.
    #[test]
    fn run_parallel_matches_stepping_at_every_thread_count() {
        for &(worlds, cap, per, mix) in &[
            (8u32, 32u32, 32u32, Mix::Uniform),
            (13, 24, 11, Mix::Skewed),
            (3, 16, 16, Mix::Single(Order::Gather)),
        ] {
            let mut reference = build(worlds, cap, per, mix);
            for _ in 0..75 {
                reference.step(StepPlan::REFERENCE);
            }
            let want = reference.digest();
            for threads in [1usize, 2, 3, 5, 8, 16] {
                for plan in [
                    StepPlan::REFERENCE,
                    StepPlan {
                        phase_a: PhaseA::Match,
                        resolve: Resolve::Sequential,
                    },
                    StepPlan {
                        phase_a: PhaseA::Partitioned,
                        resolve: Resolve::Sequential,
                    },
                    StepPlan {
                        phase_a: PhaseA::Partitioned,
                        resolve: Resolve::Segmented,
                    },
                ] {
                    let mut a = build(worlds, cap, per, mix);
                    a.run_parallel(75, threads, plan);
                    assert_eq!(
                        a.digest(),
                        want,
                        "run_parallel({threads}, {plan:?}) diverged at {worlds}w x {per} ({mix:?})"
                    );
                    assert_eq!(a.frame, reference.frame, "frame counter drifted");
                }
            }
        }
    }

    /// A digest is a hash; column equality is the real check. Done once, deeply.
    #[test]
    fn columns_are_identical_not_merely_the_digest() {
        let mut a = build(9, 48, 48, Mix::Uniform);
        let mut b = build(9, 48, 48, Mix::Uniform);
        for _ in 0..100 {
            a.step(StepPlan::REFERENCE);
            b.step(StepPlan {
                phase_a: PhaseA::PartitionedParallel(4),
                resolve: Resolve::SegmentedParallel(4),
            });
        }
        assert_eq!(a.pos_x, b.pos_x, "pos_x");
        assert_eq!(a.pos_y, b.pos_y, "pos_y");
        assert_eq!(a.hits, b.hits, "hits");
        assert_eq!(a.carried, b.carried, "carried");
        assert_eq!(a.cooldown, b.cooldown, "cooldown");
        assert_eq!(a.seed, b.seed, "seed");
        assert_eq!(a.pool, b.pool, "pool");
    }

    /// The test above is vacuous unless the fixture actually exercises the interesting
    /// paths: contention on shared HP, and pools that actually run dry.
    #[test]
    fn the_fixture_really_does_contend_and_really_does_exhaust() {
        let mut a = build(4, 64, 64, Mix::Uniform);
        let mut multi_hit = 0;
        for _ in 0..40 {
            a.step(StepPlan::REFERENCE);
            let mut counts = std::collections::HashMap::new();
            for (i, &t) in a.strike_target.iter().enumerate() {
                if a.strike_damage[i] != 0 {
                    *counts.entry(t).or_insert(0u32) += 1;
                }
            }
            multi_hit += counts.values().filter(|&&c| c > 1).count();
        }
        assert!(
            multi_hit > 0,
            "no target was ever hit twice in a frame: no contention to resolve"
        );
        assert!(
            a.pool.iter().any(|&p| p == 0),
            "no pool ever ran dry: the exhaustible-draw path is untested"
        );
    }

    /// Partitioning must not quietly drop or duplicate entities.
    #[test]
    fn the_partition_covers_every_live_entity_exactly_once() {
        let a = build(11, 32, 20, Mix::Skewed);
        let h = a.order_histogram();
        assert_eq!(h.iter().map(|&c| c as u64).sum::<u64>(), a.live_count());
        let mut p = Partition::new(ORDER_COUNT);
        let order = a.order.clone();
        p.build(&a.live_slots, |s| order[s as usize]);
        assert_eq!(p.len() as u64, a.live_count());
        for k in 0..ORDER_COUNT {
            assert_eq!(p.bucket(k).len() as u32, h[k], "bucket {k} size");
        }
    }

    /// Worlds must stay independent: nothing an entity does may reach another world.
    #[test]
    fn worlds_do_not_interact() {
        let mut all = build(6, 32, 32, Mix::Uniform);
        for _ in 0..40 {
            all.step(StepPlan {
                phase_a: PhaseA::PartitionedParallel(4),
                resolve: Resolve::SegmentedParallel(4),
            });
        }
        // The same world simulated alone must reach the same state.
        for w in 0..6u32 {
            let mut one = Arena::new(1, 32);
            one.populate(32, Mix::Uniform, 0xD0_11);
            // Copy world w's initial columns into the singleton, then run it.
            let mut src = build(6, 32, 32, Mix::Uniform);
            let base = (w * 32) as usize;
            one.pos_x.copy_from_slice(&src.pos_x[base..base + 32]);
            one.pos_y.copy_from_slice(&src.pos_y[base..base + 32]);
            one.tgt_x.copy_from_slice(&src.tgt_x[base..base + 32]);
            one.tgt_y.copy_from_slice(&src.tgt_y[base..base + 32]);
            one.hits.copy_from_slice(&src.hits[base..base + 32]);
            one.cooldown.copy_from_slice(&src.cooldown[base..base + 32]);
            one.owner.copy_from_slice(&src.owner[base..base + 32]);
            one.seed.copy_from_slice(&src.seed[base..base + 32]);
            one.order.copy_from_slice(&src.order[base..base + 32]);
            // `target` is a row within a world, so it copies across unchanged — which is
            // exactly the position-independence this layout relies on.
            one.target.copy_from_slice(&src.target[base..base + 32]);
            one.pool
                .copy_from_slice(&src.pool[w as usize * OWNERS..(w as usize + 1) * OWNERS]);
            src.frame = 0;
            for _ in 0..40 {
                one.step(StepPlan::REFERENCE);
            }
            assert_eq!(
                &one.hits[..],
                &all.hits[base..base + 32],
                "world {w} diverged when simulated alone: worlds are not independent"
            );
            assert_eq!(
                &one.pos_x[..],
                &all.pos_x[base..base + 32],
                "world {w} pos_x"
            );
        }
    }
}
