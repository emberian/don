//! Deterministic many-to-one accumulation, without atomics.
//!
//! Two conflict shapes show up everywhere in an RTS tick, and they are **not** the same
//! problem:
//!
//! 1. **Accumulation** — many attackers add damage into one defender's HP; many gatherers add
//!    income into one player's stockpile. The result must not depend on who got there first.
//! 2. **Draw from an exhaustible pool** — many workers take from a resource that can run out;
//!    many builders draw from a treasury. Here the result *does* depend on the order, because
//!    whoever draws first may leave nothing for the rest.
//!
//! The usual GPU answer to both is an atomic. That is wrong for a lockstep simulation twice
//! over: atomics fix neither problem's determinism (the *order* they resolve in is scheduler
//! state), and for case 2 they are not even well-defined without a compare-and-swap loop.
//!
//! # Case 1: segmented reduce, and the exact algebraic reason it is safe
//!
//! [`accumulate_i32`] sorts the `(target, value)` contributions by target with a stable
//! counting sort and reduces each run. Reduction order does not matter because the values are
//! **integers under `wrapping_add`**, which is the abelian group `Z/2^32` — associative,
//! commutative, exactly representable, no rounding. So every schedule gives the same bits.
//! The sim is integers (`Constants` has 722 members, all `int`/`int[N]`; `TypeData`,
//! `ObjectTypeData`, `UnitTypeData`, `BuildTypeData` and `TechTypeData` hold no floats), so
//! this is not a convenient special case, it is the normal case.
//!
//! **Where it does not hold — state this every time, because it is easy to lose:**
//!
//! - **Saturating or clamped addition is not associative.** `sat(sat(120, 100), -100) = 27`
//!   for `i8` but `sat(120, sat(100, -100)) = 120`. If HP clamps at zero *during*
//!   accumulation, the sum is order-dependent and this machinery does not apply. Accumulate
//!   raw and clamp **once, after** the reduce — that is what [`accumulate_i32`] forces by
//!   only ever handing you a total.
//! - **Anything with a division or a rescale mid-chain.** `ObjectData::get_damage`
//!   (`0x00644130`) subtracts armour at step 22 of 31 and rescales by 10 — those happen
//!   *per strike*, before the value reaches this reduce. Feed this the per-strike results,
//!   never the raw attack values.
//! - **`min`/`max` accumulation is fine** (idempotent commutative monoid) but `argmin`/
//!   `argmax` is not, unless the tie-break is part of the key. Sort by `(value, entity_id)`
//!   and the tie-break becomes total.
//! - **Floating point is never fine.** `f32` addition is not associative and no amount of
//!   care fixes it.
//!
//! # Case 2: segmented exclusive scan, which is order-*faithful*, not order-free
//!
//! [`draw_from_pools`] does not need commutativity; it reproduces a specific sequential
//! order in parallel. Let the draws against one pool be `w_0, w_1, …` in canonical order and
//! the pool hold `P`. The sequential loop is `take_i = min(w_i, remaining_i)`. By induction
//! `remaining_i = max(0, P − Σ_{j<i} w_j)`, so
//!
//! ```text
//! take_i = clamp(P − Σ_{j<i} w_j, 0, w_i)
//! ```
//!
//! which is an **exclusive prefix sum followed by a clamp** — a scan, which parallelises,
//! and which reproduces the sequential answer *exactly* rather than approximately. The
//! prefix sums are taken in `i64` so a long segment cannot overflow.
//!
//! The canonical order here is `(pool, item index)`, and item index is the arena slot, which
//! is `world * capacity + row`. It is a real modelling choice: when the engine's own
//! resolution order is derived it may be different (owner-slot rotation, `(frame + i) % 10`,
//! is exactly the kind of thing that would change it), and then only the sort key changes.

use crate::partition::KeySort;

/// Reusable scratch so a per-tick reduction allocates nothing.
#[derive(Clone, Debug, Default)]
pub struct ReduceScratch {
    sort: KeySort,
    segs: Vec<u32>,
}

impl ReduceScratch {
    pub fn new() -> ReduceScratch {
        ReduceScratch::default()
    }
    /// Segment boundaries of the last sort, exposed so a caller can parallelise over them.
    pub fn segments(&self) -> &[u32] {
        &self.segs
    }
}

/// The reference: accumulate in item order, one at a time.
///
/// This *defines* the answer. Everything else in this module must reproduce it bit for bit,
/// and the tests assert that. Kept simple on purpose.
pub fn accumulate_i32_reference(targets: &[u32], vals: &[i32], out: &mut [i32]) {
    assert_eq!(targets.len(), vals.len());
    for (&t, &v) in targets.iter().zip(vals) {
        out[t as usize] = out[t as usize].wrapping_add(v);
    }
}

/// Segmented reduce: sort by target, sum each run, apply once per target.
///
/// Touches `out` exactly once per *distinct* target instead of once per contribution, which
/// is the point on a machine with any kind of write-combining — and the reason this beats the
/// naive loop even single-threaded when contributions cluster.
pub fn accumulate_i32(targets: &[u32], vals: &[i32], out: &mut [i32], s: &mut ReduceScratch) {
    assert_eq!(targets.len(), vals.len());
    if targets.is_empty() {
        return;
    }
    s.sort.sort(targets, out.len());
    s.sort.segments(&mut s.segs);
    for w in s.segs.windows(2) {
        let (a, b) = (w[0] as usize, w[1] as usize);
        let key = s.sort.keys[a] as usize;
        let mut acc = 0i32;
        for &item in &s.sort.items[a..b] {
            acc = acc.wrapping_add(vals[item as usize]);
        }
        out[key] = out[key].wrapping_add(acc);
    }
}

/// The same reduce with the segments split across `threads` workers.
///
/// Segments never straddle a worker, and no two workers touch the same `out` element, so
/// this needs no atomics and no locks — and the answer cannot depend on the thread count.
/// The tests assert it at 1, 2, 3, 8 and 16 threads.
pub fn accumulate_i32_parallel(
    targets: &[u32],
    vals: &[i32],
    out: &mut [i32],
    s: &mut ReduceScratch,
    threads: usize,
) {
    assert_eq!(targets.len(), vals.len());
    if targets.is_empty() {
        return;
    }
    let threads = threads.max(1);
    s.sort.sort(targets, out.len());
    s.sort.segments(&mut s.segs);
    let nseg = s.segs.len() - 1;
    if threads == 1 || nseg < 2 {
        for w in s.segs.windows(2) {
            let (a, b) = (w[0] as usize, w[1] as usize);
            let key = s.sort.keys[a] as usize;
            let mut acc = 0i32;
            for &item in &s.sort.items[a..b] {
                acc = acc.wrapping_add(vals[item as usize]);
            }
            out[key] = out[key].wrapping_add(acc);
        }
        return;
    }

    // Each worker produces (key, total) pairs for its own segment range; the apply step is
    // then a disjoint scatter, so `out` needs no synchronisation at all.
    let per = nseg.div_ceil(threads);
    let keys = &s.sort.keys;
    let items = &s.sort.items;
    let segs = &s.segs;
    let mut parts: Vec<Vec<(u32, i32)>> = Vec::new();
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(threads);
        for t in 0..threads {
            let lo = (t * per).min(nseg);
            let hi = ((t + 1) * per).min(nseg);
            handles.push(scope.spawn(move || {
                let mut local = Vec::with_capacity(hi - lo);
                for sgi in lo..hi {
                    let (a, b) = (segs[sgi] as usize, segs[sgi + 1] as usize);
                    let key = keys[a];
                    let mut acc = 0i32;
                    for &item in &items[a..b] {
                        acc = acc.wrapping_add(vals[item as usize]);
                    }
                    local.push((key, acc));
                }
                local
            }));
        }
        for h in handles {
            parts.push(h.join().expect("reduce worker panicked"));
        }
    });
    for part in &parts {
        for &(k, v) in part {
            out[k as usize] = out[k as usize].wrapping_add(v);
        }
    }
}

/// The reference draw: walk items in order, take what is left.
///
/// This *defines* the answer for [`draw_from_pools`]. Note it is deliberately written as the
/// obvious sequential loop, because that is the semantics we are claiming to reproduce.
pub fn draw_from_pools_reference(keys: &[u32], want: &[i32], pool: &mut [i32], got: &mut [i32]) {
    assert_eq!(keys.len(), want.len());
    assert_eq!(keys.len(), got.len());
    for i in 0..keys.len() {
        let k = keys[i] as usize;
        debug_assert!(want[i] >= 0 && pool[k] >= 0, "draws and pools must be non-negative");
        let take = want[i].min(pool[k]).max(0);
        pool[k] -= take;
        got[i] = take;
    }
}

/// Deterministic parallel-shaped draw from exhaustible pools.
///
/// Reproduces [`draw_from_pools_reference`] exactly when the canonical order is `(pool,
/// item index)` — i.e. when the sequential reference visits items in increasing index order,
/// which it does. Implemented as a stable sort by pool plus a segmented exclusive scan and a
/// clamp; see the module docs for the derivation.
///
/// Requires `want[i] >= 0` and `pool[k] >= 0`; both are asserted in debug.
pub fn draw_from_pools(
    keys: &[u32],
    want: &[i32],
    pool: &mut [i32],
    got: &mut [i32],
    s: &mut ReduceScratch,
) {
    assert_eq!(keys.len(), want.len());
    assert_eq!(keys.len(), got.len());
    if keys.is_empty() {
        return;
    }
    s.sort.sort(keys, pool.len());
    s.sort.segments(&mut s.segs);
    for w in s.segs.windows(2) {
        let (a, b) = (w[0] as usize, w[1] as usize);
        let k = s.sort.keys[a] as usize;
        let p = pool[k] as i64;
        debug_assert!(p >= 0, "pool {k} is negative");
        // Exclusive prefix sum of `want` over this segment, in i64 so a long segment cannot
        // overflow; then the clamp that turns the scan into the sequential answer.
        let mut prefix: i64 = 0;
        let mut taken: i64 = 0;
        for &item in &s.sort.items[a..b] {
            let w_i = want[item as usize] as i64;
            debug_assert!(w_i >= 0, "negative draw request at item {item}");
            let take = (p - prefix).clamp(0, w_i);
            got[item as usize] = take as i32;
            taken += take;
            prefix += w_i;
        }
        pool[k] = (p - taken) as i32;
    }
}

/// Segments of the last [`draw_from_pools`] / [`accumulate_i32`] sort, so callers can see how
/// much contention there actually was.
pub fn contention_profile(s: &ReduceScratch) -> (usize, u32) {
    let segs = s.segments();
    if segs.len() < 2 {
        return (0, 0);
    }
    let mut mx = 0;
    for w in segs.windows(2) {
        mx = mx.max(w[1] - w[0]);
    }
    (segs.len() - 1, mx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(n: usize, nkeys: usize, seed: u64) -> (Vec<u32>, Vec<i32>) {
        let mut s = seed | 1;
        let mut rnd = move || {
            s ^= s >> 12;
            s ^= s << 25;
            s ^= s >> 27;
            s.wrapping_mul(0x2545_f491_4f6c_dd1d)
        };
        let mut keys = Vec::with_capacity(n);
        let mut vals = Vec::with_capacity(n);
        for _ in 0..n {
            let r = rnd();
            // Deliberately skewed: a third of the contributions land on 1% of the keys, which
            // is what a real fight looks like and what breaks naive schemes.
            let k = if r & 3 == 0 {
                (r >> 8) % (nkeys as u64 / 100).max(1)
            } else {
                (r >> 8) % nkeys as u64
            };
            keys.push(k as u32);
            vals.push(((r >> 40) as i32) % 1000 - 500);
        }
        (keys, vals)
    }

    #[test]
    fn segmented_reduce_matches_the_sequential_reference() {
        for &(n, nkeys) in &[(0usize, 8usize), (1, 8), (7, 3), (1000, 257), (10_000, 4096)] {
            let (keys, vals) = fixture(n, nkeys, 0xABCD ^ n as u64);
            let mut want = vec![0i32; nkeys];
            accumulate_i32_reference(&keys, &vals, &mut want);
            let mut got = vec![0i32; nkeys];
            let mut s = ReduceScratch::new();
            accumulate_i32(&keys, &vals, &mut got, &mut s);
            assert_eq!(got, want, "n={n} nkeys={nkeys}");
        }
    }

    #[test]
    fn segmented_reduce_is_independent_of_thread_count() {
        let (keys, vals) = fixture(50_000, 2048, 99);
        let mut want = vec![0i32; 2048];
        accumulate_i32_reference(&keys, &vals, &mut want);
        for threads in [1usize, 2, 3, 8, 16] {
            let mut got = vec![0i32; 2048];
            let mut s = ReduceScratch::new();
            accumulate_i32_parallel(&keys, &vals, &mut got, &mut s, threads);
            assert_eq!(got, want, "thread count {threads} changed the sum");
        }
    }

    /// The commutativity claim, stated as a test: shuffling the contributions must not move
    /// a bit. This is what makes the reduce safe to reorder in the first place.
    #[test]
    fn reordering_contributions_does_not_change_the_sum() {
        let (keys, vals) = fixture(20_000, 512, 7);
        let mut want = vec![0i32; 512];
        accumulate_i32_reference(&keys, &vals, &mut want);
        // A fixed permutation, applied to both columns together.
        let mut idx: Vec<usize> = (0..keys.len()).collect();
        idx.sort_by_key(|&i| (keys[i].wrapping_mul(2654435761), i));
        let k2: Vec<u32> = idx.iter().map(|&i| keys[i]).collect();
        let v2: Vec<i32> = idx.iter().map(|&i| vals[i]).collect();
        let mut got = vec![0i32; 512];
        accumulate_i32_reference(&k2, &v2, &mut got);
        assert_eq!(got, want);
    }

    /// Overflow must wrap identically both ways: the abelian-group argument is about
    /// `Z/2^32`, not about "values that happen to be small".
    #[test]
    fn wrapping_overflow_agrees_between_paths() {
        let keys = vec![0u32; 5];
        let vals = vec![i32::MAX, i32::MAX, 3, i32::MIN, -7];
        let mut want = vec![0i32; 1];
        accumulate_i32_reference(&keys, &vals, &mut want);
        let mut got = vec![0i32; 1];
        let mut s = ReduceScratch::new();
        accumulate_i32(&keys, &vals, &mut got, &mut s);
        assert_eq!(got, want);
        let mut par = vec![0i32; 1];
        accumulate_i32_parallel(&keys, &vals, &mut par, &mut s, 8);
        assert_eq!(par, want);
    }

    #[test]
    fn pool_draw_matches_the_sequential_reference() {
        let mut s0 = 12345u64;
        let mut rnd = move || {
            s0 ^= s0 >> 12;
            s0 ^= s0 << 25;
            s0 ^= s0 >> 27;
            s0.wrapping_mul(0x2545_f491_4f6c_dd1d)
        };
        for &(n, npools) in &[(0usize, 4usize), (1, 4), (13, 3), (5000, 64), (20_000, 7)] {
            let keys: Vec<u32> = (0..n).map(|_| (rnd() % npools as u64) as u32).collect();
            let want_v: Vec<i32> = (0..n).map(|_| (rnd() % 97) as i32).collect();
            // Pools deliberately too small: exhaustion is the whole point.
            let pools: Vec<i32> = (0..npools).map(|_| (rnd() % 5000) as i32).collect();

            let mut pa = pools.clone();
            let mut ga = vec![0i32; n];
            draw_from_pools_reference(&keys, &want_v, &mut pa, &mut ga);

            let mut pb = pools.clone();
            let mut gb = vec![0i32; n];
            let mut sc = ReduceScratch::new();
            draw_from_pools(&keys, &want_v, &mut pb, &mut gb, &mut sc);

            assert_eq!(gb, ga, "n={n} npools={npools}: draws differ");
            assert_eq!(pb, pa, "n={n} npools={npools}: pools differ");
            assert!(pb.iter().all(|&p| p >= 0), "a pool went negative");
        }
    }

    /// The interesting boundary: a pool that runs out exactly mid-segment. Everyone before
    /// the boundary is paid in full, one is paid partially, everyone after gets zero — and
    /// *which* one is the partial payee is the part a commutative reduce would get wrong.
    #[test]
    fn exhaustion_pays_in_canonical_order_not_in_bulk() {
        let keys = vec![0u32, 0, 0, 0];
        let want = vec![10i32, 10, 10, 10];
        let mut pool = vec![25i32];
        let mut got = vec![0i32; 4];
        let mut s = ReduceScratch::new();
        draw_from_pools(&keys, &want, &mut pool, &mut got, &mut s);
        assert_eq!(got, vec![10, 10, 5, 0]);
        assert_eq!(pool, vec![0]);

        // and the reference agrees
        let mut pool2 = vec![25i32];
        let mut got2 = vec![0i32; 4];
        draw_from_pools_reference(&keys, &want, &mut pool2, &mut got2);
        assert_eq!(got2, got);
    }

    #[test]
    fn a_long_segment_cannot_overflow_the_prefix_sum() {
        // 100k draws of ~21M each sums past i32::MAX; the scan must still be exact.
        let n = 100_000;
        let keys = vec![0u32; n];
        let want = vec![21_000_000i32; n];
        let mut pool = vec![i32::MAX];
        let mut got = vec![0i32; n];
        let mut s = ReduceScratch::new();
        draw_from_pools(&keys, &want, &mut pool, &mut got, &mut s);
        let mut pool2 = vec![i32::MAX];
        let mut got2 = vec![0i32; n];
        draw_from_pools_reference(&keys, &want, &mut pool2, &mut got2);
        assert_eq!(got, got2);
        assert_eq!(pool, pool2);
        assert_eq!(pool[0], 0);
    }
}
