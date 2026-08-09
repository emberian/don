//! Raw C-ABI wasm shim: exposes the simulation's structure-of-arrays columns to a browser.
//!
//! # Why there is no `wasm-bindgen` here
//!
//! `wasm-bindgen` exists to marshal *values* across the JS boundary. This shim marshals
//! nothing: the only things that cross are `u32` scalars, **byte offsets into wasm linear
//! memory**, and — for commands — raw engine wire bytes. JavaScript constructs
//! `Int32Array` / `Uint32Array` views over `WebAssembly.Memory.buffer` at those offsets and
//! reads the simulation's own columns in place. There is no serialisation step to make
//! faster, so there is no glue worth generating, and the module has **zero imports**.
//!
//! # The three surfaces
//!
//! 1. **Per-world, zero copy.** [`sim_world_x_ptr`] / [`sim_world_y_ptr`] /
//!    [`sim_world_tag_ptr`] return the bases of a live world's own `pos_x`, `pos_y` and
//!    `tag` allocations. A JS view at that offset *is* the simulation column. Nothing is
//!    copied on the CPU at all; the only copy in the path is the one the graphics API
//!    performs on upload, which no API lets us avoid.
//! 2. **Cluster mirror, one copy.** Each world allocates its own columns, so no contiguous
//!    byte range spans worlds — and drawing thousands of worlds in one instanced call needs
//!    exactly that. [`sim_gather`] `memcpy`s each world's live prefix into a strided
//!    cluster mirror. Three `memcpy`s per world, no per-entity loop.
//! 3. **Commands in, wire bytes.** [`sim_cmd_ptr`] hands JS a scratch buffer; JS writes an
//!    encoded engine command into it and calls [`sim_submit`]. The bytes are the engine's
//!    own packet layout (`schema/command-wire.json` -> `crate::wire_gen`), and they are
//!    drained at a tick boundary, in arrival order.
//!
//! # Fidelity
//!
//! The damage arithmetic, the unit-type table, the balance table, the combat rules and the
//! owner-slot rotation are derived; target acquisition, movement and the attack angle are
//! this crate's placeholders. [`crate::real`] states exactly which is which and that
//! statement is the one to read before believing anything on screen.
//!
//! # Memory growth is a JS-visible hazard
//!
//! Any wasm allocation can grow linear memory, which **detaches** every existing JS
//! `ArrayBuffer` view. All per-frame storage is allocated inside [`sim_create`]; the
//! steady-state loop allocates nothing, so views stay valid. JS still revalidates on the
//! buffer identity — see `public/js/wasm.js`.

pub mod game;
pub mod game_abi;
pub mod gamedata;
pub mod real;
pub mod wire_gen;

use gamedata::GameData;
use real::{RealBatch, MAP_SPAN};
use std::cell::UnsafeCell;

/// Per-world scalars produced by [`sim_gather_stats`], in `u32`s.
///
/// Eight, in two `vec4u`s, because the aggregate view's vertex shader reads them as vertex
/// attributes and wants both halves: population/hit-points/digest/frame, then
/// kills/damage/rounds/owners.
pub const STATS_PER_WORLD: usize = 8;

/// Bytes of command scratch. The largest engine command is 522 bytes (`ChatCommand`, a
/// `wchar_t[256]`); a selection of every unit in a full world is 3 + 2*4096.
const CMD_SCRATCH: usize = 8 * 1024;
/// Maximum ids one pick can return.
const PICK_MAX: usize = 4096;

/// Staging buffer for the packed game data, filled by JS before [`sim_create`].
///
/// A `static mut` behind `UnsafeCell` rather than a `Mutex`: wasm32-unknown-unknown is
/// single-threaded and every access here happens on one thread between JS calls, so a lock
/// would be pure ceremony. The native twin binary (`bin/digest.rs`) is single-threaded too.
struct Stage(UnsafeCell<Vec<u8>>);
// SAFETY: never accessed from two threads; see the type's doc comment.
unsafe impl Sync for Stage {}
static STAGE: Stage = Stage(UnsafeCell::new(Vec::new()));

#[allow(clippy::mut_from_ref)]
fn stage() -> &'static mut Vec<u8> {
    // SAFETY: single-threaded by construction.
    unsafe { &mut *STAGE.0.get() }
}

/// One shard of the cluster: some number of independent worlds plus their render mirror.
///
/// A browser worker owns exactly one of these. Cluster-level parallelism is *worker*
/// parallelism: `wasm32-unknown-unknown` has no threads, and worlds share nothing, so N
/// workers over N disjoint world sets is the same decomposition a native batch would use.
pub struct Sim {
    batch: RealBatch,
    /// Rows reserved per world in the mirror. Equals each world's provisioned capacity.
    stride: u32,
    #[allow(dead_code)]
    capacity: u32,
    mirror_x: Vec<i32>,
    mirror_y: Vec<i32>,
    mirror_tag: Vec<u32>,
    stats: Vec<u32>,
    /// Framed command queue: `[world u32][who u32][len u32][bytes ...]`, drained in arrival
    /// order at the next tick boundary. One allocation, reused.
    pending: Vec<u8>,
    cmd: Vec<u8>,
    pick: Vec<i16>,
    /// Scalars returned by query calls, so no query has to allocate or return a struct.
    info: Vec<i32>,
    orders_applied: u64,
    commands_seen: u64,
}

impl Sim {
    fn new(
        gd: GameData,
        worlds: usize,
        owners: u8,
        per_owner: u32,
        capacity: usize,
        seed: u64,
    ) -> Sim {
        let capacity = capacity
            .max((owners as usize).max(1) * per_owner as usize)
            .max(1)
            .min(don_sim::MAX_UNITS);
        let batch = RealBatch::new(gd, worlds, capacity, owners, per_owner, seed);
        let stride = capacity as u32;
        let cells = worlds * capacity;
        let mut s = Sim {
            batch,
            stride,
            capacity: capacity as u32,
            mirror_x: vec![0; cells],
            mirror_y: vec![0; cells],
            mirror_tag: vec![0; cells],
            stats: vec![0; worlds * STATS_PER_WORLD],
            pending: Vec::with_capacity(4096),
            cmd: vec![0; CMD_SCRATCH],
            pick: vec![0; PICK_MAX],
            info: vec![0; 16],
            orders_applied: 0,
            commands_seen: 0,
        };
        for w in s.batch.worlds.iter_mut() {
            w.refresh_tags(&s.batch.gd);
        }
        s.gather();
        s.gather_stats();
        s
    }

    /// Drain the command queue into world state. Runs **before** the tick, in arrival
    /// order, so the result depends only on the queue contents — the lockstep discipline,
    /// and the property that makes spectating, playing and replaying the same code path.
    fn apply_commands(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let q = std::mem::take(&mut self.pending);
        let mut o = 0usize;
        while o + 12 <= q.len() {
            let rd = |k: usize| u32::from_le_bytes([q[k], q[k + 1], q[k + 2], q[k + 3]]);
            let world = rd(o) as usize;
            let who = rd(o + 4) as u8;
            let len = rd(o + 8) as usize;
            o += 12;
            if o + len > q.len() {
                break;
            }
            let b = &q[o..o + len];
            o += len;
            let Some(w) = self.batch.worlds.get_mut(world) else { continue };
            if b.is_empty() {
                continue;
            }
            self.commands_seen += 1;
            // Dispatch on byte 0, exactly as `CommandPackage::process` does.
            self.orders_applied += match b[0] {
                wire_gen::op::GROUP => {
                    let n = wire_gen::group::num(b);
                    let who_field = wire_gen::group::who(b);
                    // The packet names its own player; the transport's `who` is the
                    // fallback for a stream that carries it out of band.
                    let player = if who_field >= 0 { who_field as u8 } else { who };
                    let ids = (0..n).filter_map(|k| wire_gen::group::entry(b, k));
                    w.cmd_group(player, ids) as u64
                }
                wire_gen::op::MOVE_TO => {
                    let x = wire_gen::move_to::to_x(b);
                    let y = wire_gen::move_to::to_y(b);
                    w.cmd_move_to(who, x, y) as u64
                }
                wire_gen::op::ATTACK => w.cmd_attack(who, wire_gen::attack::whom(b)) as u64,
                wire_gen::op::HALT => w.cmd_halt(who) as u64,
                _ => 0,
            };
        }
        self.pending = q;
        self.pending.clear();
    }

    fn step(&mut self, frames: u32) {
        for _ in 0..frames {
            self.apply_commands();
            self.batch.step();
        }
    }

    /// Refresh the cluster mirror from the authoritative world columns: three contiguous
    /// `memcpy`s per world of its live prefix, and no per-entity work anywhere.
    fn gather(&mut self) {
        let stride = self.stride as usize;
        for (w, world) in self.batch.worlds.iter().enumerate() {
            let base = w * stride;
            let live = world.live_count() as usize;
            self.mirror_x[base..base + live].copy_from_slice(world.pos_x());
            self.mirror_y[base..base + live].copy_from_slice(world.pos_y());
            self.mirror_tag[base..base + live].copy_from_slice(world.tags());
            // The tail must read as unoccupied, or a shrinking world leaves ghosts.
            for slot in base + live..base + stride {
                self.mirror_tag[slot] = 0;
            }
        }
    }

    fn gather_stats(&mut self) {
        for (w, world) in self.batch.worlds.iter().enumerate() {
            let live = world.live_count() as usize;
            let mut owners_alive = 0u32;
            let mut seen = [false; real::OWNER_SLOTS];
            for &o in world.owner_col() {
                let k = (o as usize) % real::OWNER_SLOTS;
                if !seen[k] {
                    seen[k] = true;
                    owners_alive += 1;
                }
            }
            let s = &mut self.stats[w * STATS_PER_WORLD..(w + 1) * STATS_PER_WORLD];
            s[0] = live as u32;
            s[1] = world.total_hits().min(u32::MAX as u64) as u32;
            s[2] = world.digest() as u32;
            s[3] = world.frame as u32;
            s[4] = world.kills;
            s[5] = world.damage_dealt.min(u32::MAX as u64) as u32;
            s[6] = world.rounds;
            s[7] = owners_alive;
        }
    }
}

// ---------------------------------------------------------------------------------------
// C ABI. Every `*mut Sim` is a handle JS holds; there is no global simulation state, so one
// wasm instance can host several shards if a caller ever wants that.
// ---------------------------------------------------------------------------------------

/// Reserve `len` bytes of staging space for the packed game data and return its offset.
///
/// Called once, before [`sim_create`], so the allocation cannot detach a view any running
/// frame loop depends on.
#[no_mangle]
pub extern "C" fn sim_data_alloc(len: u32) -> *mut u8 {
    let s = stage();
    s.clear();
    s.resize(len as usize, 0);
    s.as_mut_ptr()
}

/// Whether the staged bytes parse as a game-data pack. `1` real, `0` not (the caller then
/// gets the synthetic table and must say so on screen).
#[no_mangle]
pub extern "C" fn sim_data_check() -> u32 {
    match GameData::parse(stage()) {
        Some(_) => 1,
        None => 0,
    }
}

/// Create a shard. Returns a handle, or null if `worlds` is zero.
///
/// If staged game data is present and parses, this shard runs on the real unit-type and
/// balance tables; otherwise it falls back to the synthetic stand-in and [`sim_is_real`]
/// returns 0. The seed is split because a `u64` across the wasm C ABI becomes a BigInt in
/// JS, and a BigInt in the hot path is a foot-gun not worth having near this API.
#[no_mangle]
pub extern "C" fn sim_create(
    worlds: u32,
    owners: u32,
    per_owner: u32,
    #[allow(dead_code)]
    capacity: u32,
    seed_lo: u32,
    seed_hi: u32,
) -> *mut Sim {
    if worlds == 0 {
        return std::ptr::null_mut();
    }
    let gd = GameData::parse(stage()).unwrap_or_else(GameData::synthetic);
    let seed = ((seed_hi as u64) << 32) | seed_lo as u64;
    Box::into_raw(Box::new(Sim::new(
        gd,
        worlds as usize,
        owners.clamp(2, real::OWNER_SLOTS as u32) as u8,
        per_owner.max(1),
        capacity as usize,
        seed,
    )))
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
macro_rules! sim_ptr {
    ($s:expr) => {
        match unsafe { $s.as_mut() } {
            Some(r) => r,
            None => return std::ptr::null_mut(),
        }
    };
}

/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_worlds(s: *mut Sim) -> u32 {
    sim_ref!(s).batch.worlds.len() as u32
}

/// Rows reserved per world in the mirror. Instance `i` belongs to world `i / stride`.
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_stride(s: *mut Sim) -> u32 {
    sim_ref!(s).stride
}

/// 1 when this shard is running on the packed game data, 0 on the synthetic stand-in.
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_is_real(s: *mut Sim) -> u32 {
    sim_ref!(s).batch.gd.is_real as u32
}

/// Distinct values in the loaded balance table — a fingerprint the UI shows so "real data"
/// is a claim the page can back up rather than assert.
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_balance_distinct(s: *mut Sim) -> u32 {
    sim_ref!(s).batch.gd.balance_distinct()
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

/// Total kills across the shard — an output of the derived damage chain, so it is the
/// cheapest evidence that the chain is actually running.
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_kills(s: *mut Sim) -> u32 {
    sim_ref!(s).batch.worlds.iter().map(|w| w.kills).sum()
}

/// Total damage dealt across the shard, low 32 bits.
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_damage_lo(s: *mut Sim) -> u32 {
    sim_ref!(s).batch.worlds.iter().map(|w| w.damage_dealt).sum::<u64>() as u32
}

/// Advance the shard by `frames` frames, draining the command queue at each tick boundary.
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

/// Refresh the cluster mirror.
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
pub unsafe extern "C" fn sim_mirror_x_ptr(s: *mut Sim) -> *mut i32 {
    sim_ptr!(s).mirror_x.as_mut_ptr()
}
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_mirror_y_ptr(s: *mut Sim) -> *mut i32 {
    sim_ptr!(s).mirror_y.as_mut_ptr()
}
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_mirror_tag_ptr(s: *mut Sim) -> *mut u32 {
    sim_ptr!(s).mirror_tag.as_mut_ptr()
}
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_stats_ptr(s: *mut Sim) -> *mut u32 {
    sim_ptr!(s).stats.as_mut_ptr()
}

/// **The zero-copy surface.** Byte offset of world `w`'s own `pos_x` column; the length is
/// [`sim_world_live`]. A JS `Int32Array` at this offset aliases the simulation itself.
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_world_x_ptr(s: *mut Sim, w: u32) -> *const i32 {
    let sim = sim_ptr!(s);
    match sim.batch.worlds.get(w as usize) {
        Some(world) => world.pos_x_base(),
        None => std::ptr::null_mut(),
    }
}
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_world_y_ptr(s: *mut Sim, w: u32) -> *const i32 {
    let sim = sim_ptr!(s);
    match sim.batch.worlds.get(w as usize) {
        Some(world) => world.pos_y_base(),
        None => std::ptr::null_mut(),
    }
}
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_world_tag_ptr(s: *mut Sim, w: u32) -> *const u32 {
    let sim = sim_ptr!(s);
    match sim.batch.worlds.get(w as usize) {
        Some(world) => world.tag_base(),
        None => std::ptr::null_mut(),
    }
}

/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_world_live(s: *mut Sim, w: u32) -> u32 {
    let sim = sim_ref!(s);
    sim.batch.worlds.get(w as usize).map(|x| x.live_count()).unwrap_or(0)
}

// ---- commands --------------------------------------------------------------------------

/// Scratch buffer JS writes an encoded engine command into, then calls [`sim_submit`].
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_cmd_ptr(s: *mut Sim) -> *mut u8 {
    sim_ptr!(s).cmd.as_mut_ptr()
}

/// Capacity of the command scratch buffer, in bytes.
#[no_mangle]
pub extern "C" fn sim_cmd_capacity() -> u32 {
    CMD_SCRATCH as u32
}

/// Queue the first `len` bytes of the command scratch as a command for `world`, issued by
/// player `who`.
///
/// `who` comes from the transport, not from the packet: in the engine the command stream is
/// per-player and the packet body carries only the command's own fields. `GroupCommand` is
/// the exception — it names its player in a `who` field — and the dispatcher honours that.
///
/// Returns the number of commands now queued.
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_submit(s: *mut Sim, world: u32, who: u32, len: u32) -> u32 {
    let sim = sim_ref!(s);
    let n = (len as usize).min(sim.cmd.len());
    sim.pending.extend_from_slice(&world.to_le_bytes());
    sim.pending.extend_from_slice(&who.to_le_bytes());
    sim.pending.extend_from_slice(&(n as u32).to_le_bytes());
    let bytes = sim.cmd[..n].to_vec();
    sim.pending.extend_from_slice(&bytes);
    sim.pending.len() as u32
}

/// Object ids of `who`'s units within `radius` subtiles of a point in `world`.
///
/// This is a *query*, the equivalent of the engine's mouse pick — it changes nothing. The
/// selection it feeds is applied by a `GroupCommand`, which is a command. Ids land in the
/// buffer at [`sim_pick_ptr`] as `i16`, the width of a `GroupCommand` list entry.
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_pick_box(
    s: *mut Sim,
    world: u32,
    who: u32,
    x: i32,
    y: i32,
    radius: i32,
) -> u32 {
    let sim = sim_ref!(s);
    let Some(w) = sim.batch.worlds.get(world as usize) else { return 0 };
    w.pick(who as u8, x, y, radius, &mut sim.pick) as u32
}

/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_pick_ptr(s: *mut Sim) -> *mut i16 {
    sim_ptr!(s).pick.as_mut_ptr()
}

/// Nearest unit to a point, of any owner. Fills the info block and returns 1 if found.
///
/// Info layout: `[id, owner, type_id, hits, max_hits, roster_index, attack_x10, armor]`.
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_pick_nearest(s: *mut Sim, world: u32, x: i32, y: i32) -> u32 {
    let sim = sim_ref!(s);
    let gd = &sim.batch.gd;
    let Some(w) = sim.batch.worlds.get(world as usize) else { return 0 };
    let Some((id, owner, tidx, hits, maxh)) = w.pick_nearest(x, y) else { return 0 };
    let ut = gd.units[tidx as usize];
    let i = &mut sim.info;
    i[0] = id as i32;
    i[1] = owner as i32;
    i[2] = ut.type_id;
    i[3] = hits;
    i[4] = maxh;
    i[5] = ut.roster;
    i[6] = ut.attack;
    i[7] = ut.armor;
    1
}

/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_info_ptr(s: *mut Sim) -> *mut i32 {
    sim_ptr!(s).info.as_mut_ptr()
}

/// Units affected by commands so far — selections made plus orders applied.
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_orders_applied(s: *mut Sim) -> u32 {
    sim_ref!(s).orders_applied as u32
}

/// Commands dispatched so far.
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_commands_seen(s: *mut Sim) -> u32 {
    sim_ref!(s).commands_seen as u32
}

/// Low 32 bits of the batch digest, and its high half in [`sim_digest_hi`].
///
/// Exported so a browser run and a native run of the same seed can be compared. That
/// comparison is the point: it is a check that could fail.
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_digest_lo(s: *mut Sim) -> u32 {
    sim_ref!(s).batch.digest() as u32
}
/// # Safety
/// `s` must be a live handle from [`sim_create`].
#[no_mangle]
pub unsafe extern "C" fn sim_digest_hi(s: *mut Sim) -> u32 {
    (sim_ref!(s).batch.digest() >> 32) as u32
}

/// Map span in subtiles, so JS never hard-codes a constant that lives in Rust.
#[no_mangle]
pub extern "C" fn sim_map_span() -> i32 {
    MAP_SPAN
}

/// Milliseconds per simulation tick at Normal speed.
///
/// **67** — `TurnControl::timings` at `0x00AFC4A4` is `{200, 125, 67, 50, 1}` ms and Normal
/// is the middle entry [measured]. That is 14.93 Hz, not the 15 Hz `don_sim::TICK_HZ` still
/// carries from the `rules.xml` header; the difference matters for anything that converts
/// frames to seconds.
#[no_mangle]
pub extern "C" fn sim_tick_ms() -> u32 {
    67
}

/// `u32`s per world in the stats table.
#[no_mangle]
pub extern "C" fn sim_stats_per_world() -> u32 {
    STATS_PER_WORLD as u32
}

/// Owner slots the frame scheduler rotates through.
#[no_mangle]
pub extern "C" fn sim_owner_slots() -> u32 {
    real::OWNER_SLOTS as u32
}
