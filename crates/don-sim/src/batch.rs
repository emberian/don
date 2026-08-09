//! Batch of independent worlds, stepped in parallel.
//!
//! Worlds share nothing, so the batch dimension is embarrassingly parallel — the property
//! the whole RL-throughput plan rests on. Threads are raw `std::thread` scoped workers
//! rather than a dependency, keeping the core crate dependency-free.
//!
//! # Two shapes of parallel run, and why both exist
//!
//! [`Batch::step_parallel`] advances the batch by exactly one frame and joins. It is the
//! shape an interactive stepper needs, and it pays a thread spawn+join per frame:
//! **[measured] 126.6 µs at 12 threads, 86.2 µs at 8, 28.1 µs at 2** on an Apple M2 Max
//! (2000 iterations of an empty `std::thread::scope`). That is a fixed cost per frame
//! regardless of how little work the frame contains, and for small batches it dominates.
//!
//! [`Batch::run_parallel`] advances by `frames` frames inside a **single** scope, with
//! workers pulling worlds from a shared cursor and running one world all the way through
//! before moving to the next. Spawn cost is paid once per run instead of once per frame,
//! the world stays resident in cache across its frames, and uneven populations load
//! balance themselves. This is the rollout API, and it is what the throughput numbers in
//! `docs/derivation/simd-batch.md` are measured with.
//!
//! Both are bit-identical to serial stepping at every thread count, because worlds never
//! interact — the tests assert exactly that, since silent thread-count-dependent
//! divergence would poison every later determinism claim.

use crate::world::World;
use std::sync::Mutex;

pub struct Batch {
    pub worlds: Vec<World>,
}

impl Batch {
    /// Create `n` worlds with distinct, reproducible seeds, each provisioned to the hard
    /// capacity [`crate::MAX_UNITS`].
    pub fn new(n: usize, base_seed: u64) -> Batch {
        Batch::with_capacity(n, base_seed, crate::MAX_UNITS)
    }

    /// Create `n` worlds provisioned for `capacity` units each.
    ///
    /// Right-sizing matters at batch scale: a full-capacity world reserves ~110 KiB of
    /// columns, so 4096 of them reserve ~450 MiB and first-touch every page of it, most
    /// of which a 64-unit population never uses.
    pub fn with_capacity(n: usize, base_seed: u64, capacity: usize) -> Batch {
        Batch {
            worlds: (0..n)
                .map(|i| {
                    World::with_capacity(
                        capacity,
                        base_seed ^ ((i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)),
                    )
                })
                .collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.worlds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.worlds.is_empty()
    }

    pub fn populate(&mut self, units_per_world: usize) {
        for w in &mut self.worlds {
            for _ in 0..units_per_world {
                w.spawn(0);
            }
        }
    }

    /// Total live units across the batch.
    pub fn live_units(&self) -> u64 {
        self.worlds.iter().map(|w| w.live_count() as u64).sum()
    }

    /// Step every world once, single-threaded.
    pub fn step_serial(&mut self) {
        for w in &mut self.worlds {
            w.step();
        }
    }

    /// Advance every world by `frames`, single-threaded, one world at a time.
    ///
    /// World-major rather than frame-major: identical results (worlds are independent),
    /// but the world's columns stay in cache for all of its frames.
    pub fn run_serial(&mut self, frames: usize) {
        for w in &mut self.worlds {
            for _ in 0..frames {
                w.step();
            }
        }
    }

    /// Step every world once across `threads` workers, then join.
    ///
    /// Worlds are split into contiguous chunks. Because worlds never interact, the result
    /// is identical to `step_serial` regardless of thread count. Note the per-call spawn
    /// cost documented on this module: prefer [`Batch::run_parallel`] when advancing more
    /// than one frame.
    pub fn step_parallel(&mut self, threads: usize) {
        let threads = threads.max(1);
        if threads == 1 || self.worlds.len() < 2 {
            return self.step_serial();
        }
        let chunk = self.worlds.len().div_ceil(threads);
        std::thread::scope(|scope| {
            for part in self.worlds.chunks_mut(chunk) {
                scope.spawn(move || {
                    for w in part {
                        w.step();
                    }
                });
            }
        });
    }

    /// Advance every world by `frames` across `threads` workers, spawning once.
    ///
    /// Workers pull groups of worlds from a shared cursor, so a batch whose worlds hold
    /// very different populations still balances. The assignment of worlds to workers is
    /// nondeterministic and **that is fine**: each world's trajectory depends only on its
    /// own state, so the resulting batch state is identical to `run_serial` for any
    /// assignment. The tests assert it.
    pub fn run_parallel(&mut self, frames: usize, threads: usize) {
        let threads = threads.max(1);
        if frames == 0 {
            return;
        }
        if threads == 1 || self.worlds.len() < 2 {
            return self.run_serial(frames);
        }
        // Several groups per worker so a late-finishing group cannot leave a core idle,
        // but few enough that the cursor is not a contention point.
        let grab = (self.worlds.len() / (threads * 4)).max(1);
        let queue = Mutex::new(self.worlds.chunks_mut(grab));
        std::thread::scope(|scope| {
            for _ in 0..threads {
                let queue = &queue;
                scope.spawn(move || loop {
                    // Lock is held only for the pop, never across simulation work.
                    let next = queue.lock().expect("worker panicked").next();
                    let Some(part) = next else { break };
                    for w in part {
                        for _ in 0..frames {
                            w.step();
                        }
                    }
                });
            }
        });
    }

    /// Advance every world by one frame of the **element-wise subset**
    /// ([`World::step_hot`]), single-threaded.
    ///
    /// Exists so the lane-major experiment has a like-for-like reference. It is not the
    /// tick; see the note on `World::step_hot`.
    pub fn step_hot_serial(&mut self) {
        for w in &mut self.worlds {
            w.step_hot();
        }
    }

    /// [`Batch::step_hot_serial`] for `frames` frames, world-major.
    pub fn run_hot_serial(&mut self, frames: usize) {
        for w in &mut self.worlds {
            for _ in 0..frames {
                w.step_hot();
            }
        }
    }

    /// [`Batch::run_hot_serial`] across `threads` workers, spawning once.
    pub fn run_hot_parallel(&mut self, frames: usize, threads: usize) {
        let threads = threads.max(1);
        if frames == 0 {
            return;
        }
        if threads == 1 || self.worlds.len() < 2 {
            return self.run_hot_serial(frames);
        }
        let grab = (self.worlds.len() / (threads * 4)).max(1);
        let queue = Mutex::new(self.worlds.chunks_mut(grab));
        std::thread::scope(|scope| {
            for _ in 0..threads {
                let queue = &queue;
                scope.spawn(move || loop {
                    let next = queue.lock().expect("worker panicked").next();
                    let Some(part) = next else { break };
                    for w in part {
                        for _ in 0..frames {
                            w.step_hot();
                        }
                    }
                });
            }
        });
    }

    /// Give every live unit a non-zero movement step and cooldown.
    ///
    /// The element-wise kernels map zero to zero, so a batch populated with resting units
    /// makes any comparison between two stepping strategies **vacuously** true. Anything
    /// measuring or asserting on `step_hot` must call this first.
    pub fn energise(&mut self) {
        for (wi, w) in self.worlds.iter_mut().enumerate() {
            for row in 0..w.live_count() as usize {
                let k = (wi * 7 + row * 13) as i32;
                w.set_move_step(row, (k % 251) - 125, (k % 197) - 98);
                w.cooldown_mut()[row] = (k % 23) as i16;
            }
        }
    }

    /// Combined digest across all worlds, for determinism checks.
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for w in &self.worlds {
            h ^= w.digest();
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build() -> Batch {
        let mut b = Batch::new(37, 0xABCD);
        b.populate(48);
        b
    }

    #[test]
    fn parallel_matches_serial_regardless_of_thread_count() {
        let mut serial = build();
        for _ in 0..120 {
            serial.step_serial();
        }
        let want = serial.digest();

        for threads in [2usize, 3, 8, 16] {
            let mut b = build();
            for _ in 0..120 {
                b.step_parallel(threads);
            }
            assert_eq!(
                b.digest(),
                want,
                "thread count {threads} changed the result"
            );
        }
    }

    #[test]
    fn run_parallel_matches_serial_regardless_of_thread_count() {
        let mut serial = build();
        serial.run_serial(120);
        let want = serial.digest();

        for threads in [2usize, 3, 8, 16] {
            let mut b = build();
            b.run_parallel(120, threads);
            assert_eq!(
                b.digest(),
                want,
                "thread count {threads} changed the result"
            );
        }
    }

    /// Frame-major and world-major traversals must agree; worlds are independent, so any
    /// interleaving of their frames is the same computation.
    #[test]
    fn run_and_repeated_step_agree() {
        let mut a = build();
        for _ in 0..77 {
            a.step_serial();
        }
        let mut b = build();
        b.run_serial(77);
        assert_eq!(a.digest(), b.digest());
        let mut c = build();
        c.run_parallel(77, 4);
        assert_eq!(c.digest(), b.digest());
    }

    /// Uneven populations are the case dynamic pulling exists for; it must not change the
    /// answer.
    #[test]
    fn uneven_populations_still_match_serial() {
        let build_uneven = || {
            let mut b = Batch::with_capacity(23, 0x5A5A, 300);
            for (i, w) in b.worlds.iter_mut().enumerate() {
                for _ in 0..(i * i) % 300 {
                    w.spawn((i % 3) as u8);
                }
            }
            b
        };
        let mut serial = build_uneven();
        serial.run_serial(60);
        let want = serial.digest();
        for threads in [2usize, 5, 16] {
            let mut b = build_uneven();
            b.run_parallel(60, threads);
            assert_eq!(b.digest(), want);
        }
    }

    #[test]
    fn seeds_are_distinct_per_world() {
        let mut b = Batch::new(8, 1);
        b.populate(16);
        for _ in 0..10 {
            b.step_serial();
        }
        let digests: Vec<u64> = b.worlds.iter().map(|w| w.digest()).collect();
        let mut uniq = digests.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), digests.len(), "worlds must not be identical");
    }

    #[test]
    fn capacity_does_not_change_the_simulation() {
        // A right-sized world and a full-capacity world must agree: capacity is a
        // provisioning decision, not a semantic one.
        let mut small = Batch::with_capacity(5, 77, 64);
        let mut full = Batch::with_capacity(5, 77, crate::MAX_UNITS);
        small.populate(64);
        full.populate(64);
        small.run_serial(200);
        full.run_serial(200);
        assert_eq!(small.digest(), full.digest());
        assert_eq!(small.live_units(), 5 * 64);
    }
}
