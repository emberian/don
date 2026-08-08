//! Batch of independent worlds, stepped in parallel.
//!
//! Worlds share nothing, so the batch dimension is embarrassingly parallel — the property
//! the whole RL-throughput plan rests on. Threads are raw `std::thread` scoped workers
//! rather than a dependency, keeping the core crate dependency-free.

use crate::world::World;

pub struct Batch {
    pub worlds: Vec<World>,
}

impl Batch {
    /// Create `n` worlds with distinct, reproducible seeds.
    pub fn new(n: usize, base_seed: u64) -> Batch {
        Batch {
            worlds: (0..n)
                .map(|i| World::new(base_seed ^ ((i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))))
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

    /// Step every world once, single-threaded.
    pub fn step_serial(&mut self) {
        for w in &mut self.worlds {
            w.step();
        }
    }

    /// Step every world once across `threads` workers.
    ///
    /// Worlds are split into contiguous chunks. Because worlds never interact, the result
    /// is identical to `step_serial` regardless of thread count — a property the tests
    /// assert, since silent thread-count-dependent divergence would poison every later
    /// determinism claim.
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

    #[test]
    fn parallel_matches_serial_regardless_of_thread_count() {
        let build = || {
            let mut b = Batch::new(37, 0xABCD);
            b.populate(48);
            b
        };
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
            assert_eq!(b.digest(), want, "thread count {threads} changed the result");
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
}
