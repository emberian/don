//! Raw C-ABI wasm shim: exposes `don-sim`'s structure-of-arrays columns to a browser.
//!
//! # Why there is no `wasm-bindgen` here
//!
//! `wasm-bindgen` exists to marshal *values* across the JS boundary. This shim marshals
//! nothing: the only things that cross are `u32` scalars and **byte offsets into wasm
//! linear memory**. JavaScript then constructs `Int32Array` / `Uint32Array` views over
//! `WebAssembly.Memory.buffer` at those offsets and reads the simulation's own columns in
//! place. There is no serialisation step to make faster, so there is no glue worth
//! generating — and no `wasm_bindgen` shim call in the per-frame path.
//!
//! # The two column surfaces, and what each costs
//!
//! 1. **Per-world, zero copy.** [`sim_world_x_ptr`] / [`sim_world_y_ptr`] return the base
//!    of a live `World`'s `pos_x` / `pos_y` `Vec<i32>`. A JS view at that offset *is* the
//!    simulation column. Nothing is copied on the CPU at all; the only copy in the whole
//!    path is the one the graphics API performs on upload, which no API lets us avoid.
//!
//! 2. **Cluster mirror, one copy.** `don_sim::Batch` allocates each world's columns
//!    separately, so no single contiguous byte range spans worlds — and drawing thousands
//!    of worlds in one instanced call needs exactly that. [`sim_gather`] `memcpy`s each
//!    world's live prefix into a strided cluster mirror. This copy is a **consequence of
//!    `Batch`'s allocation strategy, not of the browser**; a cluster-native world pool
//!    (one column per attribute for the whole batch, world-major with fixed stride) would
//!    remove it, at the cost of `Batch`'s right-sizing. It is measured, not assumed — see
//!    `docs/tracks/web-frontend.md`.
//!
//! Nothing is converted to `f32` on the way out. Positions are the engine's 1/192-tile
//! integers and they reach the vertex shader as `sint32`; normalisation into clip space is
//! one multiply in the shader, on the GPU, per instance.
//!
//! # Fidelity
//!
//! Everything simulated here is `don-sim`'s **PLACEHOLDER** mechanics (see that crate's
//! docs). This shim adds one more placeholder of its own — the order-follow pass in
//! [`Sim::apply_orders`] — which exists to demonstrate the *shape* of the drop-in-to-play
//! path (an order queue drained at a tick boundary), not any behaviour of the original
//! engine. No claim in this file is a fidelity claim.
//!
//! # Memory growth is a JS-visible hazard
//!
//! Any wasm allocation can grow linear memory, which **detaches** every existing JS
//! `ArrayBuffer` view. All per-frame storage is therefore allocated once inside
//! [`sim_create`]; the steady-state loop allocates nothing, so views stay valid. JS still
//! revalidates on the buffer identity — see `public/js/wasm.js`.

use don_sim::{Batch, Handle, World, MAP_SPAN};

/// Per-world scalars produced by [`sim_gather_stats`], in `u32`s.
///
/// Fixed-width so the whole table is one strided upload and the aggregate view can index
/// it without a descriptor.
pub const STATS_PER_WORLD: usize = 4;

/// Order kinds, keyed by the engine's **real** command opcodes.
///
/// These are not invented. `schema/command-wire.json` (derived by the `headless-client`
/// lane from the shipped `rise.pdb` plus `CommandPackage::process` = `FUN_0094a700`) gives
/// all 82 opcodes with their struct names, byte sizes and field layouts. This shim uses the
/// real opcode numbers so that the demo's queue is already carrying values from the engine's
/// own action space rather than a parallel invented one.
///
/// What is *not* claimed: this shim does not implement those commands. It honours a
/// strict subset of each struct's fields (listed per constant) and ignores the rest. The
/// placeholder `don-sim` mechanics have nothing for `queued`, `form`, `width`, `orders`,
/// `angle` or `disembark` to mean yet.
pub mod order_kind {
    /// `MoveToCommand`, opcode `0x07`, 22 bytes.
    /// Real fields: `to_x, to_y, set_angle, angle, orders, queued, form, width, disembark`.
    /// **Honoured here: `to_x`, `to_y` only.**
    pub const MOVE_TO: u32 = 0x07;
    /// `HaltCommand`, opcode `0x0c`, 1 byte, no fields.
    /// Honoured here as "clear standing targets for this owner".
    pub const HALT: u32 = 0x0c;
    /// **Not an engine command.** A demo affordance for adding a unit to a world. Kept
    /// deliberately outside the engine's `0x00`–`0x51` range so it can never be confused
    /// for one.
    pub const DEMO_SPAWN: u32 = 0x1000;
}

/// Subtiles per frame a unit under a standing order closes on its target.
///
/// PLACEHOLDER. Chosen only so ordered motion is legible against the placeholder
/// integrator's +/-128-subtile wander; it is not a derived speed.
const ORDER_SPEED: i32 = 384;

/// A queued order, in the form the network/replay layer carries.
///
/// Deliberately fixed-size and integer-only, because that is what the engine's own command
/// records are: `docs/replay-format.md` establishes that the `.rcx` command stream *is* the
/// network protocol, and `schema/command-wire.json` gives every packet's exact layout. It is
/// also the shape an RL action-tensor row has. This struct is a superset envelope, not any
/// one command: `kind` is the real opcode, and which of `sx/sy/tx/ty/radius` mean anything
/// depends on it.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Order {
    pub kind: u32,
    pub world: u32,
    pub owner: u32,
    pub sx: i32,
    pub sy: i32,
    pub tx: i32,
    pub ty: i32,
    pub radius: i32,
}

/// One shard of the cluster: some number of independent worlds plus their render mirror.
///
/// A browser worker owns exactly one of these. Cluster-level parallelism is *worker*
/// parallelism — `don_sim::Batch::run_parallel` cannot be used, because
/// `wasm32-unknown-unknown` has no threads.
pub struct Sim {
    batch: Batch,
    /// Rows reserved per world in the mirror. Equals each world's provisioned capacity.
    stride: u32,
    capacity: u32,
    /// Cluster mirror of `pos_x`, world-major, `stride` rows per world.
    mirror_x: Vec<i32>,
    /// Cluster mirror of `pos_y`.
    mirror_y: Vec<i32>,
    /// Per-instance render tag: bit 31 = occupied, bits 0..7 = owner.
    ///
    /// Changes only when a world's population changes, so it is uploaded on a dirty flag
    /// rather than every frame.
    tag: Vec<u32>,
    tag_dirty: bool,
    /// Per-world stats, `STATS_PER_WORLD` `u32`s each.
    stats: Vec<u32>,
    /// Standing move target per (world, handle id). Handle-keyed, not row-keyed, so a
    /// despawn compacting rows cannot re-point an order at a different unit.
    target_x: Vec<i32>,
    target_y: Vec<i32>,
    has_target: Vec<u8>,
    /// Orders accepted since the last tick, drained in arrival order at the tick boundary.
    pending: Vec<Order>,
    /// Total nanosecond-free counters, for reporting only.
    orders_applied: u64,
}

impl Sim {
    fn new(worlds: usize, units_per_world: usize, capacity: usize, seed: u64) -> Sim {
        let capacity = capacity.max(units_per_world).max(1).min(don_sim::MAX_UNITS);
        let mut batch = Batch::with_capacity(worlds, seed, capacity);
        batch.populate(units_per_world);
        let stride = capacity as u32;
        let cells = worlds * capacity;
        let mut s = Sim {
            batch,
            stride,
            capacity: capacity as u32,
            mirror_x: vec![0; cells],
            mirror_y: vec![0; cells],
            tag: vec![0; cells],
            tag_dirty: true,
            stats: vec![0; worlds * STATS_PER_WORLD],
            target_x: vec![0; cells],
            target_y: vec![0; cells],
            has_target: vec![0; cells],
            pending: Vec::with_capacity(256),
            orders_applied: 0,
        };
        s.gather();
        s.gather_stats();
        s
    }

    /// Drain the order queue into world state. Runs *before* the tick, in arrival order,
    /// so the result depends only on the queue contents — the lockstep discipline.
    fn apply_orders(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let cap = self.capacity as usize;
        // `take` so the borrow of `self.pending` ends before the world loop.
        let pending = std::mem::take(&mut self.pending);
        for o in &pending {
            let Some(world) = self.batch.worlds.get_mut(o.world as usize) else { continue };
            let base = o.world as usize * cap;
            match o.kind {
                order_kind::DEMO_SPAWN => {
                    if world.spawn(o.owner as u8).is_some() {
                        self.tag_dirty = true;
                    }
                }
                order_kind::HALT => {
                    for k in 0..cap {
                        self.has_target[base + k] = 0;
                    }
                }
                _ => {
                    let live = world.live_count() as usize;
                    let r2 = (o.radius as i64) * (o.radius as i64);
                    for row in 0..live {
                        if world.owner()[row] as u32 != o.owner {
                            continue;
                        }
                        let dx = (world.pos_x()[row] - o.sx) as i64;
                        let dy = (world.pos_y()[row] - o.sy) as i64;
                        if dx * dx + dy * dy > r2 {
                            continue;
                        }
                        let id = world.handles()[row] as usize;
                        self.target_x[base + id] = o.tx;
                        self.target_y[base + id] = o.ty;
                        self.has_target[base + id] = 1;
                        self.orders_applied += 1;
                    }
                }
            }
        }
        // Reuse the allocation: steady state must not touch the allocator.
        self.pending = pending;
        self.pending.clear();
    }

    /// Pull units with a standing target toward it. PLACEHOLDER motion; see module docs.
    fn follow_targets(&mut self) {
        let cap = self.capacity as usize;
        for (w, world) in self.batch.worlds.iter_mut().enumerate() {
            let base = w * cap;
            let live = world.live_count() as usize;
            for row in 0..live {
                let id = world.handles()[row] as usize;
                if self.has_target[base + id] == 0 {
                    continue;
                }
                let (tx, ty) = (self.target_x[base + id], self.target_y[base + id]);
                let (px, py) = (world.pos_x()[row], world.pos_y()[row]);
                let (dx, dy) = (tx - px, ty - py);
                if dx.abs() <= ORDER_SPEED && dy.abs() <= ORDER_SPEED {
                    world.set_pos(row, tx, ty);
                    self.has_target[base + id] = 0;
                    continue;
                }
                // Integer step, no trig, no float: clamp each axis independently. Cheap
                // and deterministic; diagonal speed is the usual Chebyshev overshoot.
                let nx = px + dx.signum() * dx.abs().min(ORDER_SPEED);
                let ny = py + dy.signum() * dy.abs().min(ORDER_SPEED);
                world.set_pos(row, nx.rem_euclid(MAP_SPAN), ny.rem_euclid(MAP_SPAN));
            }
        }
    }

    fn step(&mut self, frames: u32) {
        for _ in 0..frames {
            self.apply_orders();
            for w in &mut self.batch.worlds {
                w.step();
            }
            self.follow_targets();
        }
    }

    /// Refresh the cluster mirror from the authoritative world columns.
    ///
    /// Two `copy_from_slice` calls per world; both are contiguous `memcpy`s of the live
    /// prefix. This is the one CPU-side copy the cluster path pays, and the reason is in
    /// the module docs.
    fn gather(&mut self) {
        let stride = self.stride as usize;
        for (w, world) in self.batch.worlds.iter().enumerate() {
            let base = w * stride;
            let live = world.live_count() as usize;
            self.mirror_x[base..base + live].copy_from_slice(world.pos_x());
            self.mirror_y[base..base + live].copy_from_slice(world.pos_y());
        }
        if self.tag_dirty {
            for (w, world) in self.batch.worlds.iter().enumerate() {
                let base = w * stride;
                let live = world.live_count() as usize;
                for row in 0..live {
                    self.tag[base + row] = 0x8000_0000 | world.owner()[row] as u32;
                }
                for slot in live..stride {
                    self.tag[base + slot] = 0;
                }
            }
            self.tag_dirty = false;
        }
    }

    fn gather_stats(&mut self) {
        for (w, world) in self.batch.worlds.iter().enumerate() {
            let live = world.live_count() as usize;
            let mut hits: u64 = 0;
            for row in 0..live {
                hits += world.hits()[row].max(0) as u64;
            }
            let d = world.digest();
            let s = &mut self.stats[w * STATS_PER_WORLD..(w + 1) * STATS_PER_WORLD];
            s[0] = live as u32;
            s[1] = hits.min(u32::MAX as u64) as u32;
            s[2] = d as u32;
            s[3] = world.frame as u32;
        }
    }
}

// ---------------------------------------------------------------------------------------
// C ABI. Every `*mut Sim` is a handle JS holds; there is no global state, so one wasm
// instance can host several shards if a caller ever wants that.
// ---------------------------------------------------------------------------------------

/// Create a shard. Returns a handle, or null if `worlds` is zero.
///
/// The seed is split because the C ABI over wasm32 passes `u64` as a BigInt in JS, and a
/// BigInt in the hot path is a foot-gun worth not having near this API at all.
#[no_mangle]
pub extern "C" fn sim_create(
    worlds: u32,
    units_per_world: u32,
    capacity: u32,
    seed_lo: u32,
    seed_hi: u32,
) -> *mut Sim {
    if worlds == 0 {
        return std::ptr::null_mut();
    }
    let seed = ((seed_hi as u64) << 32) | seed_lo as u64;
    let s = Box::new(Sim::new(
        worlds as usize,
        units_per_world as usize,
        capacity as usize,
        seed,
    ));
    Box::into_raw(s)
}

/// # Safety
/// `s` must be a handle from [`sim_create`] that has not already been destroyed.
#[no_mangle]
pub unsafe extern "C" fn sim_destroy(s: *mut Sim) {
    if !s.is_null() {
        // SAFETY: caller guarantees `s` came from `Box::into_raw` in `sim_create`.
        drop(unsafe { Box::from_raw(s) });
    }
}

macro_rules! sim_ref {
    ($s:expr) => {
        // SAFETY: every exported entry point documents that `s` must be a live handle from
        // `sim_create`; the null check keeps a zero handle from dereferencing.
        match unsafe { $s.as_mut() } {
            Some(r) => r,
            None => return Default::default(),
        }
    };
}

/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_worlds(s: *mut Sim) -> u32 {
    sim_ref!(s).batch.len() as u32
}

/// Rows reserved per world in the mirror. Instance `i` belongs to world `i / stride`.
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_stride(s: *mut Sim) -> u32 {
    sim_ref!(s).stride
}

/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_live_total(s: *mut Sim) -> u32 {
    sim_ref!(s).batch.live_units() as u32
}

/// Frame counter of world 0 (all worlds in a shard advance together).
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_frame(s: *mut Sim) -> u32 {
    let sim = sim_ref!(s);
    sim.batch.worlds.first().map(|w| w.frame as u32).unwrap_or(0)
}

/// Advance the shard by `frames` frames, draining the order queue at each tick boundary.
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_step(s: *mut Sim, frames: u32) {
    let sim = match unsafe { s.as_mut() } {
        Some(r) => r,
        None => return,
    };
    sim.step(frames);
}

/// Refresh the cluster mirror (and the tag column if the population changed).
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_gather(s: *mut Sim) {
    let sim = match unsafe { s.as_mut() } {
        Some(r) => r,
        None => return,
    };
    sim.gather();
}

/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_gather_stats(s: *mut Sim) {
    let sim = match unsafe { s.as_mut() } {
        Some(r) => r,
        None => return,
    };
    sim.gather_stats();
}

/// Byte offset of the cluster mirror's x column. Length is `worlds * stride` `i32`s.
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_mirror_x_ptr(s: *mut Sim) -> *const i32 {
    let sim = match unsafe { s.as_mut() } {
        Some(r) => r,
        None => return std::ptr::null(),
    };
    sim.mirror_x.as_ptr()
}

/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_mirror_y_ptr(s: *mut Sim) -> *const i32 {
    let sim = match unsafe { s.as_mut() } {
        Some(r) => r,
        None => return std::ptr::null(),
    };
    sim.mirror_y.as_ptr()
}

/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_tag_ptr(s: *mut Sim) -> *const u32 {
    let sim = match unsafe { s.as_mut() } {
        Some(r) => r,
        None => return std::ptr::null(),
    };
    sim.tag.as_ptr()
}

/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_stats_ptr(s: *mut Sim) -> *const u32 {
    let sim = match unsafe { s.as_mut() } {
        Some(r) => r,
        None => return std::ptr::null(),
    };
    sim.stats.as_ptr()
}

/// **The zero-copy surface.** Byte offset of world `w`'s own `pos_x` column; the length is
/// [`sim_world_live`]. A JS `Int32Array` at this offset aliases the simulation itself.
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_world_x_ptr(s: *mut Sim, w: u32) -> *const i32 {
    world_col(s, w, World::pos_x)
}

/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_world_y_ptr(s: *mut Sim, w: u32) -> *const i32 {
    world_col(s, w, World::pos_y)
}

/// Shared body of the two per-world column accessors. Not `unsafe fn` so the `unsafe`
/// blocks inside stay meaningful rather than being implied by the signature.
fn world_col(s: *mut Sim, w: u32, f: fn(&World) -> &[i32]) -> *const i32 {
    let sim = match unsafe { s.as_mut() } {
        Some(r) => r,
        None => return std::ptr::null(),
    };
    match sim.batch.worlds.get(w as usize) {
        // `pos_x()` is trimmed to the live prefix, but the *base* pointer is the column
        // base, which is what a vertex buffer binding needs.
        Some(world) => f(world).as_ptr(),
        None => std::ptr::null(),
    }
}

/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_world_live(s: *mut Sim, w: u32) -> u32 {
    let sim = sim_ref!(s);
    sim.batch.worlds.get(w as usize).map(|x| x.live_count()).unwrap_or(0)
}

/// Queue one order. Applied at the next tick boundary, in call order.
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub unsafe extern "C" fn sim_order(
    s: *mut Sim,
    kind: u32,
    world: u32,
    owner: u32,
    sx: i32,
    sy: i32,
    tx: i32,
    ty: i32,
    radius: i32,
) -> u32 {
    let sim = match unsafe { s.as_mut() } {
        Some(r) => r,
        None => return 0,
    };
    sim.pending.push(Order { kind, world, owner, sx, sy, tx, ty, radius });
    sim.pending.len() as u32
}

/// Low 32 bits of the batch digest — the same function the native tests use.
///
/// Exported so a browser run and a native run of the same seed can be compared. That
/// comparison is the point: it is a check that could fail.
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_digest_lo(s: *mut Sim) -> u32 {
    sim_ref!(s).batch.digest() as u32
}

/// High 32 bits of the batch digest.
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_digest_hi(s: *mut Sim) -> u32 {
    (sim_ref!(s).batch.digest() >> 32) as u32
}

/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_orders_applied(s: *mut Sim) -> u32 {
    sim_ref!(s).orders_applied as u32
}

/// Map span in subtiles, so JS never hard-codes a constant that lives in Rust.
#[no_mangle]
pub extern "C" fn sim_map_span() -> i32 {
    MAP_SPAN
}

/// Simulation frames per second at normal speed.
#[no_mangle]
pub extern "C" fn sim_tick_hz() -> u32 {
    don_sim::TICK_HZ
}

/// `u32`s per world in the stats table.
#[no_mangle]
pub extern "C" fn sim_stats_per_world() -> u32 {
    STATS_PER_WORLD as u32
}

/// Keeps `Handle` in the public surface so the handle-keyed order table's type is
/// documented where a reader of this file will find it.
#[allow(dead_code)]
fn _handle_is_the_identity(_: Handle) {}
