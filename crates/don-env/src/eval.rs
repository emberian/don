//! Deterministic building blocks for policy evaluation.
//!
//! `VecEnv` already makes simulation output independent of its worker count.  Policy
//! evaluation needs the same property one level up: changing the number of match workers
//! must change only wall-clock time, never case order, seeds, or the report checksum.
//!
//! This module deliberately knows nothing about a particular world or policy.  It supplies
//! an ordered parallel map and a stable report digest; `don-ai::arena::eval` owns the arena
//! cases and their honest readiness labels.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

/// Run independent cases in parallel and return results in input order.
///
/// The callback receives the immutable global case index.  Seeds must be derived from that
/// index (or carried by the item), never from a worker id: worker assignment is intentionally
/// unspecified and may differ between runs.  `threads == 0` is treated as one worker.
pub fn ordered_parallel_map<T, R, E, F>(items: &[T], threads: usize, f: F) -> Vec<Result<R, E>>
where
    T: Sync,
    R: Send,
    E: Send,
    F: Fn(usize, &T) -> Result<R, E> + Sync,
{
    if items.is_empty() {
        return Vec::new();
    }
    let workers = threads.max(1).min(items.len());
    if workers == 1 {
        return items
            .iter()
            .enumerate()
            .map(|(i, item)| f(i, item))
            .collect();
    }

    let next = AtomicUsize::new(0);
    let (send, recv) = mpsc::channel();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let send = send.clone();
            let f = &f;
            let next = &next;
            scope.spawn(move || loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(item) = items.get(i) else { break };
                if send.send((i, f(i, item))).is_err() {
                    break;
                }
            });
        }
        drop(send);
        let mut completed: Vec<_> = recv.into_iter().collect();
        completed.sort_unstable_by_key(|(i, _)| *i);
        completed.into_iter().map(|(_, result)| result).collect()
    })
}

/// Stable, dependency-free FNV-1a digest for evaluation reports.
///
/// This is an identity for comparing two runs of *our* evaluator.  It is not the retail
/// `CheckSum::walk_data` channel and must never be presented as one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvalDigest(u64);

impl Default for EvalDigest {
    fn default() -> Self {
        Self::new()
    }
}

impl EvalDigest {
    pub const OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
    pub const PRIME: u64 = 0x0000_0100_0000_01B3;

    pub const fn new() -> Self {
        EvalDigest(Self::OFFSET)
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    pub fn write_u8(&mut self, value: u8) {
        self.write_bytes(&[value]);
    }

    pub fn write_u32(&mut self, value: u32) {
        self.write_bytes(&value.to_le_bytes());
    }

    pub fn write_u64(&mut self, value: u64) {
        self.write_bytes(&value.to_le_bytes());
    }

    pub fn write_i64(&mut self, value: i64) {
        self.write_bytes(&value.to_le_bytes());
    }

    pub fn write_str(&mut self, value: &str) {
        self.write_u64(value.len() as u64);
        self.write_bytes(value.as_bytes());
    }

    pub const fn finish(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_map_is_thread_count_independent() {
        let input: Vec<u64> = (0..257).collect();
        let run = |threads| {
            ordered_parallel_map(&input, threads, |i, value| {
                // Make completion order differ from input order without sleeping.
                let rounds = (i % 11) * 37;
                let mut out = value.wrapping_mul(0x9E37_79B9);
                for _ in 0..rounds {
                    out = out.rotate_left(7) ^ 0xA5A5_A5A5;
                }
                Ok::<_, ()>(out)
            })
        };
        assert_eq!(run(1), run(2));
        assert_eq!(run(1), run(8));
        assert_eq!(run(1), run(0));
    }

    #[test]
    fn digest_has_a_frozen_cross_platform_image() {
        let mut h = EvalDigest::new();
        h.write_str("RoNEval");
        h.write_u8(6);
        h.write_u32(0x5EED_0001);
        h.write_u64(17_573_073);
        h.write_i64(-15);
        assert_eq!(h.finish(), 0xBF6A_C3AA_53C9_6019);
    }
}
