//! Fixed-capacity structure-of-arrays world state.
//!
//! # Storage discipline: dense rows, stable handles
//!
//! Live units occupy rows `0..live` of every column, with **no holes**. A tick system is
//! therefore a straight-line pass over a contiguous prefix with no occupancy test — the
//! shape a vector unit wants, and the reason the per-unit cost stops depending on how
//! empty the world is.
//!
//! Densifying costs index stability, so identity is carried by [`Handle`] instead of by a
//! row number: a handle table maps handle id to row, `despawn` swap-removes the last row
//! into the hole and repairs the one moved handle. This is O(1) and allocation-free.
//! Handles carry a generation counter, so a handle to a despawned unit is *rejected*
//! rather than silently aliasing whatever unit later reuses the id (the old slot-index
//! API had exactly that ABA hazard).
//!
//! Column-per-allocation is deliberate: a tick that touches only `pos_*`, `vel_*` and
//! `cooldown` pulls in no cache line belonging to `armor`/`attack`/`recharge`/`owner`.
//! That is the hot/cold split, achieved by the layout rather than by a second struct.

use crate::simd;

/// Simulation frames per second at normal speed. From the `rules.xml` header comment
/// ("Times are specified in 'frames' or fifteenths of seconds"). Shipped game data, so
/// usable ground truth, but **not yet confirmed against the binary**.
pub const TICK_HZ: u32 = 15;

/// Movement granularity: the engine expresses speeds as fractions of a tile with a
/// largest allowed denominator of 192 (`rules.xml` header). Positions here are therefore
/// kept in 1/192-tile units as integers.
///
/// Note this does *not* imply the engine's own runtime representation is fixed point —
/// measurement shows the binary is float-heavy (`docs/binary-ground-truth.md`). Whether
/// parsed rule values land in `f32` or in fixed point is an open question. Integer
/// positions are chosen here for *our* determinism; if derivation shows the original
/// integrates in `f32`, this changes.
pub const SUBTILE: i32 = 192;

/// Hard per-world unit capacity. A world may be provisioned smaller (see
/// [`World::with_capacity`]) but never larger, so a row index always fits comfortably in
/// `u32`. The real population cap is a rule value we have not derived yet; this is a
/// provisioning decision, not a claim about the game.
pub const MAX_UNITS: usize = 4096;

/// PLACEHOLDER map extent in 1/192-tile units. The real map sizes are underived; this
/// exists to give the placeholder integrator a wrap point.
pub const MAP_SPAN: i32 = 256 * SUBTILE;

const NO_ROW: u32 = u32::MAX;

/// Stable identity for a unit, valid across compaction.
///
/// `generation` is bumped when an id is freed, so a stale handle fails validation instead
/// of addressing whichever unit reused the id.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Handle {
    pub id: u32,
    pub generation: u32,
}

/// One simulation world in structure-of-arrays form.
///
/// Columns are parallel and indexed by *row*; rows `0..live_count()` are occupied and rows
/// beyond that are undefined. Use [`Handle`] for anything that must survive a despawn.
#[derive(Clone)]
pub struct World {
    // ---- hot columns: touched every frame by the tick systems ----
    pos_x: Vec<i32>,
    pos_y: Vec<i32>,
    vel_x: Vec<i32>,
    vel_y: Vec<i32>,
    /// Frames remaining before this unit may attack again.
    cooldown: Vec<i16>,

    // ---- cold columns: not touched by the current tick systems ----
    // (names mirror the binary's own attribute names; `hits` moves to the hot set the
    // moment a derived combat system lands)
    hits: Vec<i32>,
    armor: Vec<i16>,
    attack: Vec<i16>,
    recharge: Vec<i16>,
    owner: Vec<u8>,

    // ---- identity ----
    /// A permutation of `0..capacity` at all times: entries `0..live` are the handle id of
    /// each live row, and entries `live..capacity` are the pool of unused ids. Keeping the
    /// free pool in the tail of this array is what a separate free-list vector would
    /// otherwise cost, and it makes `despawn` a single swap inside one permutation.
    handle_of_row: Vec<u32>,
    /// handle id -> row. Meaningful only for ids whose generation still matches a live
    /// handle; the generation check in [`World::row_of`] is what rejects the rest, so this
    /// array is never consulted for a dead id.
    row_of_handle: Vec<u32>,
    /// handle id -> generation, bumped on despawn so a stale handle cannot resolve.
    generation: Vec<u32>,
    live: u32,
    capacity: u32,

    /// Frames elapsed.
    pub frame: u64,
    /// Per-world deterministic RNG state.
    ///
    /// PLACEHOLDER generator. The engine's own RNG algorithm and call sites are on the
    /// derivation worklist; replaying the original requires *its* generator, not a good
    /// one. This exists so the scheduler is deterministic today.
    rng: u64,
}

impl World {
    /// A world provisioned to the hard capacity [`MAX_UNITS`].
    pub fn new(seed: u64) -> World {
        World::with_capacity(MAX_UNITS, seed)
    }

    /// A world provisioned for at most `capacity` units (clamped to [`MAX_UNITS`]).
    ///
    /// Capacity is fixed at construction, so stepping never allocates; sizing it to the
    /// population a batch actually spawns is what keeps 4096 small worlds from reserving
    /// (and page-faulting) 4096 full-size worlds' worth of columns.
    pub fn with_capacity(capacity: usize, seed: u64) -> World {
        let n = capacity.min(MAX_UNITS);
        World {
            pos_x: vec![0; n],
            pos_y: vec![0; n],
            vel_x: vec![0; n],
            vel_y: vec![0; n],
            cooldown: vec![0; n],
            hits: vec![0; n],
            armor: vec![0; n],
            attack: vec![0; n],
            recharge: vec![0; n],
            owner: vec![0; n],
            handle_of_row: (0..n as u32).collect(),
            row_of_handle: vec![NO_ROW; n],
            generation: vec![0; n],
            live: 0,
            capacity: n as u32,
            frame: 0,
            rng: seed | 1,
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

    /// Bytes of column storage this world reserves.
    ///
    /// Reported rather than estimated because provisioning is the dominant memory term at
    /// batch scale: a world costs this much whether it holds one unit or `capacity` of
    /// them, and a batch of 4096 pays it 4096 times.
    pub fn bytes_reserved(&self) -> usize {
        let c = self.capacity as usize;
        c * (4 * 4          // pos_x, pos_y, vel_x, vel_y
            + 4             // hits
            + 4 * 2         // cooldown, armor, attack, recharge
            + 1             // owner
            + 4 * 3)        // handle_of_row, row_of_handle, generation
    }

    #[inline]
    fn next_rand(&mut self) -> u64 {
        // xorshift64*: deterministic and cheap. PLACEHOLDER, see field docs.
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Append a unit at the end of the dense region.
    ///
    /// Consumes exactly one RNG draw, as before the densification, so a given spawn
    /// schedule produces the same units regardless of how rows are arranged.
    pub fn spawn(&mut self, owner: u8) -> Option<Handle> {
        if self.live >= self.capacity {
            return None;
        }
        let row = self.live as usize;
        // The id at the head of the free pool, i.e. the permutation entry just past the
        // dense region. Taking it needs no write: the row it will occupy already names it.
        let id = self.handle_of_row[row];
        let r = self.next_rand();
        self.pos_x[row] = (r as u32 % (256 * SUBTILE as u32)) as i32;
        self.pos_y[row] = ((r >> 32) as u32 % (256 * SUBTILE as u32)) as i32;
        self.vel_x[row] = ((r >> 8) as i8) as i32;
        self.vel_y[row] = ((r >> 16) as i8) as i32;
        self.hits[row] = 100;
        self.armor[row] = 2;
        self.attack[row] = 10;
        self.recharge[row] = 15;
        self.cooldown[row] = 0;
        self.owner[row] = owner;
        self.row_of_handle[id as usize] = row as u32;
        self.live += 1;
        Some(Handle { id, generation: self.generation[id as usize] })
    }

    /// Row currently holding `h`, or `None` if the handle is stale or never existed.
    ///
    /// Two independent checks, and both are load-bearing. The generation catches an id
    /// that has been freed and handed out again (the ABA case). The row/id round trip —
    /// the id claims a row, and that row must claim the id back — catches an id that is
    /// simply dead, without needing a per-id liveness flag: a despawn either moves another
    /// unit into the row (so the row disagrees) or shrinks the dense region past it (so
    /// the row is out of range). Freed entries in `row_of_handle` are therefore left
    /// stale on purpose; nothing reads them.
    #[inline]
    pub fn row_of(&self, h: Handle) -> Option<usize> {
        let id = h.id as usize;
        if id >= self.capacity as usize || self.generation[id] != h.generation {
            return None;
        }
        let row = self.row_of_handle[id] as usize;
        if row >= self.live as usize || self.handle_of_row[row] != h.id {
            return None;
        }
        Some(row)
    }

    #[inline]
    pub fn is_alive(&self, h: Handle) -> bool {
        self.row_of(h).is_some()
    }

    /// Remove a unit by swapping the last live row into its place.
    ///
    /// Returns whether anything was removed, so a double-despawn is a no-op rather than a
    /// panic. O(1): exactly one row copy and one handle repair.
    pub fn despawn(&mut self, h: Handle) -> bool {
        let Some(row) = self.row_of(h) else {
            return false;
        };
        let last = self.live as usize - 1;
        if row != last {
            self.pos_x[row] = self.pos_x[last];
            self.pos_y[row] = self.pos_y[last];
            self.vel_x[row] = self.vel_x[last];
            self.vel_y[row] = self.vel_y[last];
            self.cooldown[row] = self.cooldown[last];
            self.hits[row] = self.hits[last];
            self.armor[row] = self.armor[last];
            self.attack[row] = self.attack[last];
            self.recharge[row] = self.recharge[last];
            self.owner[row] = self.owner[last];
            let moved = self.handle_of_row[last];
            self.handle_of_row[row] = moved;
            self.row_of_handle[moved as usize] = row as u32;
        }
        // Complete the swap inside the id permutation: the despawned id goes to the row
        // just vacated at the end, which is the head of the free pool once `live` drops.
        self.handle_of_row[last] = h.id;
        self.live -= 1;
        let id = h.id as usize;
        self.generation[id] = self.generation[id].wrapping_add(1);
        true
    }

    // ---- column views over the live region ----------------------------------------
    //
    // Every accessor is already trimmed to `live`, so callers cannot accidentally
    // reintroduce the scan-the-whole-capacity pattern this layout exists to remove.

    #[inline]
    pub fn pos_x(&self) -> &[i32] {
        &self.pos_x[..self.live as usize]
    }
    #[inline]
    pub fn pos_y(&self) -> &[i32] {
        &self.pos_y[..self.live as usize]
    }
    #[inline]
    pub fn vel_x(&self) -> &[i32] {
        &self.vel_x[..self.live as usize]
    }
    #[inline]
    pub fn vel_y(&self) -> &[i32] {
        &self.vel_y[..self.live as usize]
    }
    #[inline]
    pub fn cooldown(&self) -> &[i16] {
        &self.cooldown[..self.live as usize]
    }
    #[inline]
    pub fn hits(&self) -> &[i32] {
        &self.hits[..self.live as usize]
    }
    #[inline]
    pub fn armor(&self) -> &[i16] {
        &self.armor[..self.live as usize]
    }
    #[inline]
    pub fn attack(&self) -> &[i16] {
        &self.attack[..self.live as usize]
    }
    #[inline]
    pub fn recharge(&self) -> &[i16] {
        &self.recharge[..self.live as usize]
    }
    #[inline]
    pub fn owner(&self) -> &[u8] {
        &self.owner[..self.live as usize]
    }
    #[inline]
    pub fn handles(&self) -> &[u32] {
        &self.handle_of_row[..self.live as usize]
    }
    /// The whole id permutation, live region followed by the free pool. Exposed so the
    /// invariant that makes the free pool free can actually be tested.
    #[inline]
    pub fn all_handle_ids(&self) -> &[u32] {
        &self.handle_of_row
    }
    /// Write a live row's position.
    ///
    /// Positions are exposed for writing only through this pair-setter, because the two
    /// axes are one piece of state: an alternative layout that writes back only one of
    /// them is a bug, and this makes that bug impossible to spell.
    #[inline]
    pub fn set_pos(&mut self, row: usize, x: i32, y: i32) {
        debug_assert!(row < self.live as usize);
        self.pos_x[row] = x;
        self.pos_y[row] = y;
    }

    #[inline]
    pub fn cooldown_mut(&mut self) -> &mut [i16] {
        &mut self.cooldown[..self.live as usize]
    }
    #[inline]
    pub fn hits_mut(&mut self) -> &mut [i32] {
        &mut self.hits[..self.live as usize]
    }

    /// Advance one frame.
    ///
    /// PLACEHOLDER mechanics throughout — see the crate docs. The purpose is the memory
    /// access pattern, so throughput of this layout can be measured before real systems
    /// land. Each system is a kernel call over a dense column pair; the kernels pick a
    /// vector path per target and are asserted bit-identical to the scalar reference.
    pub fn step(&mut self) {
        let n = self.live as usize;
        simd::integrate_wrap(&mut self.pos_x[..n], &self.vel_x[..n], MAP_SPAN);
        simd::integrate_wrap(&mut self.pos_y[..n], &self.vel_y[..n], MAP_SPAN);
        simd::tick_down(&mut self.cooldown[..n]);
        self.frame += 1;
    }

    /// The same frame, forced through the scalar reference kernels.
    ///
    /// Public so tests can assert the vector path is *bit-identical*, not merely close.
    /// Not for production stepping — [`World::step`] is the one that dispatches.
    pub fn step_scalar_reference(&mut self) {
        let n = self.live as usize;
        simd::integrate_wrap_scalar(&mut self.pos_x[..n], &self.vel_x[..n], MAP_SPAN);
        simd::integrate_wrap_scalar(&mut self.pos_y[..n], &self.vel_y[..n], MAP_SPAN);
        simd::tick_down_scalar(&mut self.cooldown[..n]);
        self.frame += 1;
    }

    /// The same frame through the explicitly vectorised kernels ([`crate::simd::hand`]).
    ///
    /// Public so the benchmark can compare it against [`World::step`] on whatever machine
    /// it runs on, and so the bit-identity test has something to compare. It is not the
    /// shipped path: on both architectures measured, LLVM's vectorisation of the portable
    /// form is slightly faster. See `crate::simd` for the numbers.
    pub fn step_hand_simd(&mut self) {
        let n = self.live as usize;
        simd::hand::integrate_wrap(&mut self.pos_x[..n], &self.vel_x[..n], MAP_SPAN);
        simd::hand::integrate_wrap(&mut self.pos_y[..n], &self.vel_y[..n], MAP_SPAN);
        simd::hand::tick_down(&mut self.cooldown[..n]);
        self.frame += 1;
    }

    /// Digest of sim-critical state, independent of row order.
    ///
    /// Each unit is hashed with its *handle*, and the per-unit hashes are combined with a
    /// commutative op, so the digest is invariant under compaction (a unit moving row is
    /// not a state change) while still catching two units exchanging attributes. The old
    /// row-ordered fold could not distinguish those two cases from each other.
    ///
    /// Deliberately *not* modelled on the engine's own checksum: measurement confirms a
    /// `CheckSum` / `DataWalk` visitor exists in the binary, and once its field set is
    /// recovered this should be replaced by that definition, since the engine's notion of
    /// "sim-critical" is the authoritative one.
    pub fn digest(&self) -> u64 {
        let n = self.live as usize;
        let mut acc: u64 = 0;
        for row in 0..n {
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for v in [
                self.handle_of_row[row] as u64,
                self.pos_x[row] as u32 as u64,
                self.pos_y[row] as u32 as u64,
                self.hits[row] as u32 as u64,
                self.cooldown[row] as u16 as u64,
            ] {
                h ^= v;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
            acc = acc.wrapping_add(h);
        }
        let mut out = acc ^ self.frame;
        out = out.wrapping_mul(0x0000_0100_0000_01B3);
        out ^ (self.live as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_and_despawn_track_occupancy() {
        let mut w = World::new(7);
        assert_eq!(w.live_count(), 0);
        let a = w.spawn(0).unwrap();
        let b = w.spawn(1).unwrap();
        assert_eq!(w.live_count(), 2);
        assert!(w.despawn(a));
        assert_eq!(w.live_count(), 1);
        assert!(!w.despawn(a), "double despawn must be a no-op");
        assert_eq!(w.live_count(), 1);
        assert!(w.despawn(b));
        assert_eq!(w.live_count(), 0);
    }

    #[test]
    fn capacity_is_bounded_and_handles_recycle() {
        let mut w = World::new(1);
        let mut hs = Vec::new();
        for _ in 0..MAX_UNITS {
            hs.push(w.spawn(0).expect("within capacity"));
        }
        assert!(w.spawn(0).is_none(), "must not exceed fixed capacity");
        assert!(w.despawn(hs[0]));
        assert!(w.spawn(0).is_some(), "freed handle must be reusable");
    }

    #[test]
    fn stale_handles_are_rejected_after_reuse() {
        let mut w = World::new(3);
        let a = w.spawn(0).unwrap();
        w.spawn(1).unwrap();
        assert!(w.despawn(a));
        let c = w.spawn(2).unwrap();
        assert_eq!(c.id, a.id, "id is recycled");
        assert_ne!(c.generation, a.generation, "generation must move");
        assert!(!w.is_alive(a), "stale handle must not resolve");
        assert!(!w.despawn(a), "stale handle must not kill the unit that reused its id");
        assert!(w.is_alive(c));
    }

    #[test]
    fn rows_stay_dense_under_churn() {
        let mut w = World::new(11);
        let hs: Vec<Handle> = (0..500).map(|k| w.spawn((k % 3) as u8).unwrap()).collect();
        for (k, h) in hs.iter().enumerate() {
            if k % 2 == 0 {
                assert!(w.despawn(*h));
            }
        }
        assert_eq!(w.live_count(), 250);
        // Every surviving handle must still resolve, and to a row inside the dense region.
        for (k, h) in hs.iter().enumerate() {
            if k % 2 == 1 {
                let row = w.row_of(*h).expect("survivor must resolve");
                assert!(row < w.live_count() as usize);
            } else {
                assert!(w.row_of(*h).is_none());
            }
        }
        // Rows are a permutation of the surviving handle ids: dense, no holes, no dupes.
        let mut ids: Vec<u32> = w.handles().to_vec();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), w.live_count() as usize);
    }

    /// The id array must stay a permutation of `0..capacity` — that invariant is what lets
    /// the free pool live in its tail, and a leak there would silently cap the world.
    #[test]
    fn handle_ids_remain_a_permutation_under_churn() {
        let cap = 96;
        let mut w = World::with_capacity(cap, 0xBEE5);
        let mut alive: Vec<Handle> = Vec::new();
        let mut rng: u64 = 1;
        for round in 0..400 {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let r = (rng >> 33) as usize;
            if alive.len() < cap && (alive.is_empty() || (r & 3) != 0) {
                if let Some(h) = w.spawn((round % 4) as u8) {
                    alive.push(h);
                }
            } else if !alive.is_empty() {
                let h = alive.swap_remove(r % alive.len());
                assert!(w.despawn(h));
            }
            assert_eq!(w.live_count() as usize, alive.len());
            let mut ids = w.all_handle_ids().to_vec();
            ids.sort_unstable();
            assert!(ids.iter().copied().eq(0..cap as u32), "id permutation broken at round {round}");
            for h in &alive {
                assert!(w.is_alive(*h), "live handle stopped resolving at round {round}");
            }
        }
        // And the world must still fill to capacity after all that churn.
        while w.live_count() < cap as u32 {
            assert!(w.spawn(0).is_some());
        }
        assert!(w.spawn(0).is_none());
    }

    #[test]
    fn stepping_is_deterministic_for_a_given_seed() {
        let run = || {
            let mut w = World::new(0xDEAD_BEEF);
            for _ in 0..64 {
                w.spawn(0);
            }
            for _ in 0..500 {
                w.step();
            }
            w.digest()
        };
        assert_eq!(run(), run(), "same seed must give the same digest");
    }

    #[test]
    fn different_seeds_diverge() {
        let run = |s| {
            let mut w = World::new(s);
            for _ in 0..64 {
                w.spawn(0);
            }
            for _ in 0..100 {
                w.step();
            }
            w.digest()
        };
        assert_ne!(run(1), run(2));
    }

    #[test]
    fn positions_stay_in_bounds() {
        let mut w = World::new(99);
        for _ in 0..256 {
            w.spawn(0);
        }
        for _ in 0..2000 {
            w.step();
        }
        for row in 0..w.live_count() as usize {
            assert!((0..MAP_SPAN).contains(&w.pos_x()[row]));
            assert!((0..MAP_SPAN).contains(&w.pos_y()[row]));
        }
    }

    /// The whole point of the vector kernels: same bits, not similar numbers.
    #[test]
    fn every_kernel_path_steps_a_world_identically() {
        // Sizes chosen to straddle every vector width and leave awkward tails.
        for &units in &[0usize, 1, 3, 4, 5, 7, 8, 9, 15, 16, 17, 31, 33, 64, 257, 1000] {
            let build = || {
                let mut w = World::with_capacity(units.max(1), 0x1234_5678 ^ units as u64);
                for k in 0..units {
                    let h = w.spawn((k % 5) as u8).unwrap();
                    // Exercise the cooldown kernel on a mix of zero, positive and (illegal
                    // but well-defined) negative counters.
                    let row = w.row_of(h).unwrap();
                    w.cooldown_mut()[row] = ((k as i32 % 7) - 2) as i16;
                }
                w
            };
            let mut vecw = build();
            let mut scaw = build();
            let mut porw = build();
            for _ in 0..97 {
                vecw.step();
                scaw.step_scalar_reference();
                porw.step_hand_simd();
            }
            assert_eq!(vecw.digest(), scaw.digest(), "digest diverged at {units} units");
            assert_eq!(vecw.pos_x(), scaw.pos_x(), "pos_x diverged at {units} units");
            assert_eq!(vecw.pos_y(), scaw.pos_y(), "pos_y diverged at {units} units");
            assert_eq!(vecw.cooldown(), scaw.cooldown(), "cooldown diverged at {units}");
            assert_eq!(vecw.digest(), porw.digest(), "hand simd diverged at {units} units");
            assert_eq!(vecw.pos_x(), porw.pos_x(), "hand simd pos_x diverged at {units}");
            assert_eq!(vecw.cooldown(), porw.cooldown(), "hand simd cooldown diverged at {units}");
        }
    }

    /// Compaction must not be observable in the digest: only *state* is.
    #[test]
    fn digest_is_independent_of_row_order() {
        let mut a = World::new(5);
        let ha: Vec<Handle> = (0..64).map(|k| a.spawn((k % 4) as u8).unwrap()).collect();
        let mut b = a.clone();
        let hb: Vec<Handle> = ha.clone();
        // Kill the same logical units in opposite orders: same survivors, different rows.
        for k in [3usize, 17, 40, 41, 5] {
            assert!(a.despawn(ha[k]));
        }
        for k in [5usize, 41, 40, 17, 3] {
            assert!(b.despawn(hb[k]));
        }
        assert_ne!(
            a.handles(),
            b.handles(),
            "test is vacuous unless the two row orders actually differ"
        );
        assert_eq!(a.digest(), b.digest());
        for _ in 0..50 {
            a.step();
            b.step();
        }
        assert_eq!(a.digest(), b.digest(), "row order must not leak into the digest");
    }
}
