//! CPU reference solvers for the batched flow field.
//!
//! Three independent implementations of the *same* least fixed point:
//!
//! - [`solve_jacobi`] — the naive scalar double-buffered relaxation. This is the direct
//!   analogue of the WGSL kernel and the honest kernel-to-kernel comparison.
//! - [`solve_jacobi_rows`] — the same relaxation restructured into row-shifted slice
//!   passes so LLVM can auto-vectorise it (NEON `umin`/`add` on aarch64, AVX2 on x86).
//! - [`solve_dial`] — a bucket-queue Dijkstra. Different algorithm, `O(cells)` instead of
//!   `O(iterations x cells)`. This is what anyone would actually write on a CPU, so it is
//!   the baseline the GPU has to beat to justify itself.
//!
//! All three must agree bit-for-bit. They do, because the object they compute is the
//! **unique least fixed point of a min-plus relaxation** — see the crate docs on
//! determinism. That agreement is a real cross-check, and it is the reason a GPU result
//! can be validated without trusting the GPU.

use crate::field::{FieldBatch, COST_BLOCKED, DIR_NONE, INF, NEIGHBOURS, W_CARD, W_DIAG};

/// Largest per-cell entry cost the kernels accept. `W_DIAG * MAX_COST + INF` still fits in
/// a `u32`, which is what lets every kernel add without saturating and without wrapping.
pub const MAX_COST: u32 = 0x00ff_ffff;

/// Panics in debug if any cost would break the no-wrap argument above.
#[inline]
pub fn debug_check_costs(cost: &[u32]) {
    debug_assert!(
        cost.iter().all(|&c| c == COST_BLOCKED || c <= MAX_COST),
        "entry costs must be <= MAX_COST or exactly COST_BLOCKED"
    );
}

/// Weighted step costs for a cell, pre-masked so a blocked cell can never be relaxed
/// without a branch: its weights are `INF`, and `INF + INF` still fits a `u32`.
#[inline(always)]
fn step_weights(c: u32) -> (u32, u32) {
    if c == COST_BLOCKED {
        (INF, INF)
    } else {
        (W_CARD * c, W_DIAG * c)
    }
}

// ---------------------------------------------------------------------------------------
// scalar Jacobi — the shape the GPU kernel mirrors
// ---------------------------------------------------------------------------------------

/// One Jacobi sweep over a single field. Returns `true` if any cell changed.
pub fn jacobi_sweep(w: u32, h: u32, cost: &[u32], src: &[u32], dst: &mut [u32]) -> bool {
    let (wi, hi) = (w as i32, h as i32);
    let mut changed = false;
    for y in 0..hi {
        for x in 0..wi {
            let i = (y * wi + x) as usize;
            let (c2, c3) = step_weights(cost[i]);
            let mut best = src[i];
            for &(dx, dy, wt) in NEIGHBOURS.iter() {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= wi || ny >= hi {
                    continue;
                }
                let d = src[(ny * wi + nx) as usize];
                let cand = d + if wt == W_CARD { c2 } else { c3 };
                if cand < best {
                    best = cand;
                }
            }
            if best != src[i] {
                changed = true;
            }
            dst[i] = best;
        }
    }
    changed
}

/// Relax one field to its fixed point with the scalar Jacobi kernel.
/// Returns the number of sweeps performed.
pub fn solve_field_jacobi(w: u32, h: u32, cost: &[u32], dist: &mut [u32], max_sweeps: u32) -> u32 {
    let mut scratch = dist.to_vec();
    let mut sweeps = 0;
    loop {
        if sweeps >= max_sweeps {
            return sweeps;
        }
        let changed = jacobi_sweep(w, h, cost, dist, &mut scratch);
        dist.copy_from_slice(&scratch);
        sweeps += 1;
        if !changed {
            return sweeps;
        }
    }
}

// ---------------------------------------------------------------------------------------
// row-blocked Jacobi — same arithmetic, shaped for the vector unit
// ---------------------------------------------------------------------------------------

/// The 8-neighbour min factors into two 4-way mins because the chamfer weight depends only
/// on cardinal-vs-diagonal, not on which neighbour:
///
/// ```text
/// best = min(cur, min4(N, W, E, S) + 2c, min4(NW, NE, SW, SE) + 3c)
/// ```
///
/// Each `min4` is four shifted reads of contiguous rows, which is exactly a vector `umin`
/// chain. Boundary columns and rows fall back to the scalar path.
pub fn jacobi_sweep_rows(w: u32, h: u32, cost: &[u32], src: &[u32], dst: &mut [u32]) -> bool {
    let (wu, hu) = (w as usize, h as usize);
    if wu < 3 || hu < 3 {
        return jacobi_sweep(w, h, cost, src, dst);
    }
    let mut changed = false;

    // top and bottom rows: scalar
    for y in [0usize, hu - 1] {
        changed |= scalar_row(w, h, cost, src, dst, y);
    }
    for y in 1..hu - 1 {
        let up = &src[(y - 1) * wu..(y - 1) * wu + wu];
        let mid = &src[y * wu..y * wu + wu];
        let dn = &src[(y + 1) * wu..(y + 1) * wu + wu];
        let crow = &cost[y * wu..y * wu + wu];
        let out = &mut dst[y * wu..y * wu + wu];

        // interior columns 1..w-1, four aligned-by-shift slices per min4
        let n = wu - 2;
        let (u_l, u_c, u_r) = (&up[0..n], &up[1..n + 1], &up[2..n + 2]);
        let (m_l, m_r) = (&mid[0..n], &mid[2..n + 2]);
        let (d_l, d_c, d_r) = (&dn[0..n], &dn[1..n + 1], &dn[2..n + 2]);
        let cur = &mid[1..n + 1];
        let cc = &crow[1..n + 1];
        let o = &mut out[1..n + 1];

        for k in 0..n {
            let card = u_c[k].min(m_l[k]).min(m_r[k]).min(d_c[k]);
            let diag = u_l[k].min(u_r[k]).min(d_l[k]).min(d_r[k]);
            let c = cc[k];
            let (c2, c3) = if c == COST_BLOCKED {
                (INF, INF)
            } else {
                (W_CARD * c, W_DIAG * c)
            };
            let best = cur[k].min(card + c2).min(diag + c3);
            o[k] = best;
            changed |= best != cur[k];
        }

        // boundary columns of this row
        for x in [0usize, wu - 1] {
            changed |= scalar_cell(w, h, cost, src, dst, x, y);
        }
    }
    changed
}

#[inline]
fn scalar_row(w: u32, h: u32, cost: &[u32], src: &[u32], dst: &mut [u32], y: usize) -> bool {
    let mut changed = false;
    for x in 0..w as usize {
        changed |= scalar_cell(w, h, cost, src, dst, x, y);
    }
    changed
}

#[inline]
fn scalar_cell(
    w: u32,
    h: u32,
    cost: &[u32],
    src: &[u32],
    dst: &mut [u32],
    x: usize,
    y: usize,
) -> bool {
    let (wi, hi) = (w as i32, h as i32);
    let i = y * w as usize + x;
    let (c2, c3) = step_weights(cost[i]);
    let mut best = src[i];
    for &(dx, dy, wt) in NEIGHBOURS.iter() {
        let (nx, ny) = (x as i32 + dx, y as i32 + dy);
        if nx < 0 || ny < 0 || nx >= wi || ny >= hi {
            continue;
        }
        let d = src[(ny * wi + nx) as usize];
        let cand = d + if wt == W_CARD { c2 } else { c3 };
        if cand < best {
            best = cand;
        }
    }
    dst[i] = best;
    best != src[i]
}

pub fn solve_field_jacobi_rows(
    w: u32,
    h: u32,
    cost: &[u32],
    dist: &mut [u32],
    max_sweeps: u32,
) -> u32 {
    let mut scratch = dist.to_vec();
    let mut sweeps = 0;
    loop {
        if sweeps >= max_sweeps {
            return sweeps;
        }
        let changed = jacobi_sweep_rows(w, h, cost, dist, &mut scratch);
        dist.copy_from_slice(&scratch);
        sweeps += 1;
        if !changed {
            return sweeps;
        }
    }
}

// ---------------------------------------------------------------------------------------
// Dial's bucket-queue Dijkstra — the strong CPU baseline
// ---------------------------------------------------------------------------------------

/// Bucket-queue Dijkstra over the same graph. Linear in cells plus the distance range,
/// with no repeated sweeps. Computes the identical fixed point.
///
/// `scratch` is reused across fields so a batch run does not reallocate per field.
pub struct DialSolver {
    buckets: Vec<Vec<u32>>,
    span: u32,
}

impl DialSolver {
    /// `max_cost` is the largest finite entry cost that will be seen; the bucket ring needs
    /// `W_DIAG * max_cost + 1` slots.
    pub fn new(max_cost: u32) -> DialSolver {
        let span = W_DIAG * max_cost.max(1) + 1;
        DialSolver {
            buckets: vec![Vec::new(); span as usize],
            span,
        }
    }

    pub fn solve_field(&mut self, w: u32, h: u32, cost: &[u32], dist: &mut [u32]) {
        for b in &mut self.buckets {
            b.clear();
        }
        let (wi, hi) = (w as i32, h as i32);
        let n = cost.len();
        let mut pending = 0usize;
        let mut max_seed = 0u32;
        for i in 0..n {
            if dist[i] < INF {
                let d = dist[i];
                self.buckets[(d % self.span) as usize].push(i as u32);
                pending += 1;
                max_seed = max_seed.max(d);
            }
        }
        if pending == 0 {
            return;
        }
        let mut cur = 0u32;
        // Upper bound on any finite distance: every cell entered at most once at the worst
        // weight. Loop termination never depends on it (we exit when nothing is pending).
        while pending > 0 {
            let b = (cur % self.span) as usize;
            while let Some(i) = self.buckets[b].pop() {
                let iu = i as usize;
                // `pending` counts live queue *entries*, not cells: a cell can be pushed
                // more than once when a shorter path is found, and every push must be
                // matched by a pop or the loop never terminates.
                pending -= 1;
                if dist[iu] != cur {
                    continue; // stale entry, superseded by a shorter path
                }
                let (x, y) = ((iu % w as usize) as i32, (iu / w as usize) as i32);
                for &(dx, dy, wt) in NEIGHBOURS.iter() {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= wi || ny >= hi {
                        continue;
                    }
                    let ni = (ny * wi + nx) as usize;
                    let c = cost[ni];
                    if c == COST_BLOCKED {
                        continue;
                    }
                    let nd = cur + wt * c;
                    if nd < dist[ni] {
                        dist[ni] = nd;
                        self.buckets[(nd % self.span) as usize].push(ni as u32);
                        pending += 1;
                    }
                }
            }
            cur += 1;
        }
    }
}

// ---------------------------------------------------------------------------------------
// flow directions
// ---------------------------------------------------------------------------------------

/// Extract the flow direction for every cell: the index into [`NEIGHBOURS`] of the
/// neighbour that realises this cell's distance, ties broken by lowest index.
/// Goals and unreachable cells get [`DIR_NONE`].
pub fn directions_field(w: u32, h: u32, cost: &[u32], dist: &[u32], out: &mut [u8]) {
    let (wi, hi) = (w as i32, h as i32);
    for y in 0..hi {
        for x in 0..wi {
            let i = (y * wi + x) as usize;
            if dist[i] == 0 || dist[i] >= INF {
                out[i] = DIR_NONE;
                continue;
            }
            let (c2, c3) = step_weights(cost[i]);
            let mut best = u32::MAX;
            let mut arg = DIR_NONE;
            for (k, &(dx, dy, wt)) in NEIGHBOURS.iter().enumerate() {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= wi || ny >= hi {
                    continue;
                }
                let d = dist[(ny * wi + nx) as usize];
                let cand = d + if wt == W_CARD { c2 } else { c3 };
                // strict `<` with a fixed neighbour order: the first neighbour realising
                // the minimum wins, on every implementation.
                if cand < best {
                    best = cand;
                    arg = k as u8;
                }
            }
            out[i] = arg;
        }
    }
}

// ---------------------------------------------------------------------------------------
// batch drivers
// ---------------------------------------------------------------------------------------

/// Which CPU kernel a batch solve should use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuKernel {
    Jacobi,
    JacobiRows,
    Dial,
}

/// Solve every field in the batch on one thread.
pub fn solve_batch_serial(b: &mut FieldBatch, kernel: CpuKernel, max_sweeps: u32) {
    debug_check_costs(&b.cost);
    let per = b.cells_per_field();
    let (w, h) = (b.width, b.height);
    let mut dial = (kernel == CpuKernel::Dial).then(|| DialSolver::new(max_finite_cost(&b.cost)));
    for f in 0..b.fields as usize {
        let r = f * per..(f + 1) * per;
        let cost = &b.cost[r.clone()];
        let dist = &mut b.dist[r];
        match kernel {
            CpuKernel::Jacobi => {
                solve_field_jacobi(w, h, cost, dist, max_sweeps);
            }
            CpuKernel::JacobiRows => {
                solve_field_jacobi_rows(w, h, cost, dist, max_sweeps);
            }
            CpuKernel::Dial => dial.as_mut().unwrap().solve_field(w, h, cost, dist),
        }
    }
}

/// Solve every field across `threads` workers. Fields are independent, so the result is
/// identical to the serial path for any thread count — asserted in the tests, because
/// silent thread-count-dependent divergence would poison every determinism claim
/// downstream.
pub fn solve_batch_parallel(
    b: &mut FieldBatch,
    kernel: CpuKernel,
    max_sweeps: u32,
    threads: usize,
) {
    debug_check_costs(&b.cost);
    let threads = threads.max(1);
    let per = b.cells_per_field();
    let (w, h) = (b.width, b.height);
    if threads == 1 || b.fields < 2 {
        return solve_batch_serial(b, kernel, max_sweeps);
    }
    let max_cost = max_finite_cost(&b.cost);
    let fields_per = (b.fields as usize).div_ceil(threads);
    let chunk_cells = fields_per * per;
    let cost = &b.cost;
    std::thread::scope(|scope| {
        for (ci, dchunk) in b.dist.chunks_mut(chunk_cells).enumerate() {
            let cchunk = &cost[ci * chunk_cells..ci * chunk_cells + dchunk.len()];
            scope.spawn(move || {
                let mut dial = (kernel == CpuKernel::Dial).then(|| DialSolver::new(max_cost));
                for (f, dist) in dchunk.chunks_mut(per).enumerate() {
                    let c = &cchunk[f * per..(f + 1) * per];
                    match kernel {
                        CpuKernel::Jacobi => {
                            solve_field_jacobi(w, h, c, dist, max_sweeps);
                        }
                        CpuKernel::JacobiRows => {
                            solve_field_jacobi_rows(w, h, c, dist, max_sweeps);
                        }
                        CpuKernel::Dial => dial.as_mut().unwrap().solve_field(w, h, c, dist),
                    }
                }
            });
        }
    });
}

pub fn max_finite_cost(cost: &[u32]) -> u32 {
    cost.iter()
        .copied()
        .filter(|&c| c != COST_BLOCKED)
        .max()
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solved(kernel: CpuKernel, w: u32, h: u32, fields: u32, seed: u64) -> FieldBatch {
        let mut b = FieldBatch::synth_terrain(w, h, fields, seed);
        solve_batch_serial(&mut b, kernel, 100_000);
        b
    }

    #[test]
    fn three_independent_kernels_agree_bit_for_bit() {
        for seed in [1u64, 2, 3] {
            let a = solved(CpuKernel::Jacobi, 37, 29, 3, seed);
            let r = solved(CpuKernel::JacobiRows, 37, 29, 3, seed);
            let d = solved(CpuKernel::Dial, 37, 29, 3, seed);
            assert_eq!(a.dist, r.dist, "jacobi vs rows, seed {seed}");
            assert_eq!(a.dist, d.dist, "jacobi vs dial, seed {seed}");
        }
    }

    #[test]
    fn open_plane_gives_the_chamfer_metric() {
        // Uniform cost 1, single goal at the centre, no obstacles: the fixed point is the
        // 2/3 chamfer distance, which has a closed form. This checks the kernel against
        // the metric it claims to implement, not against a game value.
        let (w, h) = (21u32, 21u32);
        let mut b = FieldBatch::new(w, h, 1);
        b.set_goal(0, 10, 10);
        solve_batch_serial(&mut b, CpuKernel::Jacobi, 10_000);
        for y in 0..h {
            for x in 0..w {
                let dx = (x as i32 - 10).unsigned_abs();
                let dy = (y as i32 - 10).unsigned_abs();
                let (lo, hi) = (dx.min(dy), dx.max(dy));
                let want = W_DIAG * lo + W_CARD * (hi - lo);
                assert_eq!(b.dist[b.index(0, x, y)], want, "at ({x},{y})");
            }
        }
    }

    #[test]
    fn blocked_cells_stay_unreached_and_do_not_conduct() {
        // A wall across the middle with no gap: nothing below it is reachable.
        let (w, h) = (16u32, 9u32);
        let mut b = FieldBatch::new(w, h, 1);
        for x in 0..w {
            let i = b.index(0, x, 4);
            b.cost[i] = COST_BLOCKED;
        }
        b.set_goal(0, 8, 0);
        solve_batch_serial(&mut b, CpuKernel::Dial, 10_000);
        for x in 0..w {
            assert_eq!(
                b.dist[b.index(0, x, 4)],
                INF,
                "wall cell ({x},4) got a distance"
            );
            for y in 5..h {
                assert_eq!(
                    b.dist[b.index(0, x, y)],
                    INF,
                    "({x},{y}) leaked through the wall"
                );
            }
        }
        assert!(
            b.dist[b.index(0, 0, 3)] < INF,
            "same side of the wall must be reached"
        );
    }

    #[test]
    fn parallel_matches_serial_for_every_thread_count() {
        for kernel in [CpuKernel::Jacobi, CpuKernel::JacobiRows, CpuKernel::Dial] {
            let want = solved(kernel, 41, 33, 11, 4242).dist;
            for threads in [2usize, 3, 5, 8] {
                let mut b = FieldBatch::synth_terrain(41, 33, 11, 4242);
                solve_batch_parallel(&mut b, kernel, 100_000, threads);
                assert_eq!(b.dist, want, "{kernel:?} with {threads} threads diverged");
            }
        }
    }

    #[test]
    fn directions_point_downhill() {
        let mut b = FieldBatch::synth_terrain(48, 48, 1, 5);
        solve_batch_serial(&mut b, CpuKernel::Dial, 100_000);
        let mut dir = vec![DIR_NONE; b.cells_per_field()];
        directions_field(b.width, b.height, &b.cost, &b.dist, &mut dir);
        let w = b.width as i32;
        for y in 0..b.height as i32 {
            for x in 0..w {
                let i = (y * w + x) as usize;
                if dir[i] == DIR_NONE {
                    assert!(b.dist[i] == 0 || b.dist[i] >= INF);
                    continue;
                }
                let (dx, dy, _) = NEIGHBOURS[dir[i] as usize];
                let ni = ((y + dy) * w + (x + dx)) as usize;
                assert!(
                    b.dist[ni] < b.dist[i],
                    "cell ({x},{y}) d={} points at d={}",
                    b.dist[i],
                    b.dist[ni]
                );
            }
        }
    }
}
