//! Turning a branch into a partition.
//!
//! A per-entity `match order { … }` costs a mispredict per entity and blocks vectorisation.
//! The same computation, expressed as *"group the entities by order once, then run one
//! branch-free kernel per group"*, costs one counting-sort pass over the entity list and
//! nothing per entity. This module is that pass.
//!
//! # Why a counting sort, and why one pass is enough
//!
//! The key we want is `(order_type, world_id)`: orders major, so a bucket is one homogeneous
//! kernel launch spanning the whole batch; worlds minor, so a bucket's memory access stays
//! world-ordered. An LSD radix sort on that pair is two stable passes — world first, then
//! order.
//!
//! **The world pass is free**, because the input list is already in world order: the arena
//! stores worlds contiguously and enumerates live slots world-major. A stable sort on the
//! remaining digit therefore lands exactly on `(order, world, row)` in one pass. That is the
//! whole trick, and it is why partitioning costs O(n) with a 28-entry histogram rather than a
//! real sort.
//!
//! # Determinism
//!
//! Counting sort here is **stable** — the prefix-sum cursors are walked in increasing item
//! order — so the output permutation is a pure function of the key array. No tie-break, no
//! hash, no thread-order dependence. Every downstream kernel therefore sees the same entity
//! order on every run and on every machine, which is what makes the bucketed path
//! bit-identical to the branchy one rather than merely equivalent in expectation.

/// A partition of an item list into `nkeys` dense buckets.
///
/// Reused across frames: [`Partition::build`] never allocates once the capacities are large
/// enough, which matters when this runs every tick of every world.
#[derive(Clone, Debug, Default)]
pub struct Partition {
    /// Bucket boundaries: bucket `k` is `slots[starts[k]..starts[k + 1]]`. Length `nkeys+1`.
    starts: Vec<u32>,
    /// Item ids grouped by key, stable within a key.
    slots: Vec<u32>,
    /// Write cursors during a build; also the per-key counts before the prefix sum.
    cursor: Vec<u32>,
}

impl Partition {
    pub fn new(nkeys: usize) -> Partition {
        Partition {
            starts: vec![0; nkeys + 1],
            slots: Vec::new(),
            cursor: vec![0; nkeys],
        }
    }

    /// Number of buckets.
    #[inline]
    pub fn buckets(&self) -> usize {
        self.starts.len() - 1
    }

    /// Total items in the partition.
    #[inline]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Items with key `k`, in increasing item order.
    #[inline]
    pub fn bucket(&self, k: usize) -> &[u32] {
        let a = self.starts[k] as usize;
        let b = self.starts[k + 1] as usize;
        &self.slots[a..b]
    }

    /// The full grouped item list, buckets back to back.
    #[inline]
    pub fn items(&self) -> &[u32] {
        &self.slots
    }

    /// Group `items` by `key_of(item)`.
    ///
    /// Two passes: histogram, then scatter. Stable, so a bucket keeps the relative order the
    /// items had in `items` — which is world-major, hence the `(order, world, row)` claim in
    /// the module docs.
    pub fn build(&mut self, items: &[u32], key_of: impl Fn(u32) -> u8) {
        let nkeys = self.cursor.len();
        self.cursor.iter_mut().for_each(|c| *c = 0);
        for &it in items {
            let k = key_of(it) as usize;
            debug_assert!(k < nkeys, "key {k} out of range");
            self.cursor[k] += 1;
        }
        let mut acc = 0u32;
        for k in 0..nkeys {
            self.starts[k] = acc;
            acc += self.cursor[k];
            self.cursor[k] = self.starts[k];
        }
        self.starts[nkeys] = acc;

        self.slots.clear();
        self.slots.resize(items.len(), 0);
        for &it in items {
            let k = key_of(it) as usize;
            let c = &mut self.cursor[k];
            self.slots[*c as usize] = it;
            *c += 1;
        }
    }

    /// Group `0..keys.len()` by `keys[i]` — the common case where the item id *is* the index.
    pub fn build_from_keys(&mut self, keys: &[u8]) {
        let nkeys = self.cursor.len();
        self.cursor.iter_mut().for_each(|c| *c = 0);
        for &k in keys {
            debug_assert!((k as usize) < nkeys);
            self.cursor[k as usize] += 1;
        }
        let mut acc = 0u32;
        for k in 0..nkeys {
            self.starts[k] = acc;
            acc += self.cursor[k];
            self.cursor[k] = self.starts[k];
        }
        self.starts[nkeys] = acc;
        self.slots.clear();
        self.slots.resize(keys.len(), 0);
        for (i, &k) in keys.iter().enumerate() {
            let c = &mut self.cursor[k as usize];
            self.slots[*c as usize] = i as u32;
            *c += 1;
        }
    }

    /// Bucket sizes, for reporting how divergent a frame actually was.
    pub fn sizes(&self) -> Vec<u32> {
        (0..self.buckets())
            .map(|k| self.starts[k + 1] - self.starts[k])
            .collect()
    }
}

/// Stable counting sort of `(key, payload)` pairs by a **wide** (`u32`) key.
///
/// Used by [`crate::reduce`], where the key is a target entity slot rather than a 28-way
/// order id, so the histogram is the size of the entity population. Same stability argument
/// as [`Partition`].
#[derive(Clone, Debug, Default)]
pub struct KeySort {
    counts: Vec<u32>,
    /// Sorted key column.
    pub keys: Vec<u32>,
    /// Sorted payload column (the original item index).
    pub items: Vec<u32>,
}

impl KeySort {
    pub fn new() -> KeySort {
        KeySort::default()
    }

    /// Sort `0..keys.len()` by `keys[i]`, with `nkeys` possible key values.
    ///
    /// Stable: equal keys come out in increasing item order. That is load-bearing for
    /// [`crate::reduce::draw_from_pools`], where the draw order is part of the semantics.
    pub fn sort(&mut self, keys: &[u32], nkeys: usize) {
        self.counts.clear();
        self.counts.resize(nkeys + 1, 0);
        for &k in keys {
            debug_assert!((k as usize) < nkeys, "key {k} >= nkeys {nkeys}");
            self.counts[k as usize] += 1;
        }
        let mut acc = 0u32;
        for c in self.counts.iter_mut() {
            let n = *c;
            *c = acc;
            acc += n;
        }
        self.keys.clear();
        self.keys.resize(keys.len(), 0);
        self.items.clear();
        self.items.resize(keys.len(), 0);
        for (i, &k) in keys.iter().enumerate() {
            let c = &mut self.counts[k as usize];
            self.keys[*c as usize] = k;
            self.items[*c as usize] = i as u32;
            *c += 1;
        }
    }

    /// Boundaries of the maximal runs of equal keys in the sorted order, as
    /// `[start0, start1, …, len]`. This is the segment structure the reductions walk.
    pub fn segments(&self, out: &mut Vec<u32>) {
        out.clear();
        if self.keys.is_empty() {
            out.push(0);
            return;
        }
        out.push(0);
        for i in 1..self.keys.len() {
            if self.keys[i] != self.keys[i - 1] {
                out.push(i as u32);
            }
        }
        out.push(self.keys.len() as u32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_are_dense_stable_and_complete() {
        let keys: Vec<u8> = (0..1000u32)
            .map(|i| ((i * 7 + i / 13) % 28) as u8)
            .collect();
        let mut p = Partition::new(28);
        p.build_from_keys(&keys);
        assert_eq!(p.len(), keys.len());
        let mut seen = vec![false; keys.len()];
        for k in 0..28 {
            let b = p.bucket(k);
            for w in b.windows(2) {
                assert!(w[0] < w[1], "bucket {k} is not stable/increasing");
            }
            for &i in b {
                assert_eq!(keys[i as usize] as usize, k, "item {i} in the wrong bucket");
                assert!(!seen[i as usize], "item {i} appears twice");
                seen[i as usize] = true;
            }
        }
        assert!(seen.iter().all(|&s| s), "an item was dropped");
    }

    #[test]
    fn an_empty_input_partitions_into_empty_buckets() {
        let mut p = Partition::new(28);
        p.build_from_keys(&[]);
        assert_eq!(p.len(), 0);
        for k in 0..28 {
            assert!(p.bucket(k).is_empty());
        }
    }

    /// The order-major/world-minor claim: with a world-major input list, a single stable
    /// pass on the order digit leaves each bucket sorted by world.
    #[test]
    fn one_stable_pass_gives_order_major_world_minor() {
        const WORLDS: u32 = 7;
        const PER: u32 = 40;
        let items: Vec<u32> = (0..WORLDS * PER).collect(); // slot = world * PER + row
        let key_of = |slot: u32| ((slot * 5 + slot / 3) % 28) as u8;
        let mut p = Partition::new(28);
        p.build(&items, key_of);
        for k in 0..28 {
            let b = p.bucket(k);
            let worlds: Vec<u32> = b.iter().map(|&s| s / PER).collect();
            assert!(
                worlds.windows(2).all(|w| w[0] <= w[1]),
                "bucket {k} not world-sorted"
            );
        }
    }

    #[test]
    fn key_sort_is_stable_and_segments_line_up() {
        let keys: Vec<u32> = vec![5, 1, 5, 0, 3, 1, 5, 3, 3, 0];
        let mut ks = KeySort::new();
        ks.sort(&keys, 6);
        assert_eq!(ks.keys, vec![0, 0, 1, 1, 3, 3, 3, 5, 5, 5]);
        assert_eq!(ks.items, vec![3, 9, 1, 5, 4, 7, 8, 0, 2, 6]);
        let mut seg = Vec::new();
        ks.segments(&mut seg);
        assert_eq!(seg, vec![0, 2, 4, 7, 10]);
    }

    #[test]
    fn key_sort_handles_an_empty_input() {
        let mut ks = KeySort::new();
        ks.sort(&[], 4);
        let mut seg = Vec::new();
        ks.segments(&mut seg);
        assert_eq!(seg, vec![0]);
    }
}
