//! Batched integer cost/distance fields.
//!
//! One flat allocation spans **every field in the batch**, with the field index implicit
//! in the linear offset — the Madrona column-store shape (`docs/derivation/gpu-architecture.md`).
//! There is no per-field `Vec`, no pointer chase, and the buffer can be handed to a GPU or
//! to a PyTorch tensor without a copy or a gather.
//!
//! Layout is `[field][y][x]`, row-major within a field, fields contiguous. Element `i` of
//! field `f` at `(x, y)` lives at `f * width * height + y * width + x`.

/// Sentinel entry cost meaning "impassable".
///
/// Distinct from a merely expensive tile: a blocked cell never receives a finite distance
/// and never propagates one.
pub const COST_BLOCKED: u32 = u32::MAX;

/// Distance value meaning "unreached". Chosen below `u32::MAX` so that `INF + small`
/// cannot wrap; every kernel additionally refuses to relax through an `INF` neighbour, so
/// the headroom is belt-and-braces.
pub const INF: u32 = 0x7fff_ffff;

/// Chamfer weight for a cardinal (4-neighbour) step, in units where a unit-cost cardinal
/// step is 2.
pub const W_CARD: u32 = 2;
/// Chamfer weight for a diagonal step. The 2/3 chamfer metric approximates Euclidean
/// distance to within ~5.4% and, unlike a real Euclidean metric, is **exactly
/// representable in integers**, which is what buys determinism (see the crate docs).
pub const W_DIAG: u32 = 3;

/// Neighbour offsets in a fixed order. Every implementation — CPU scalar, CPU row-blocked,
/// Dial's queue, and the WGSL kernel — visits neighbours in this order, so argmin ties in
/// direction extraction break identically everywhere.
pub const NEIGHBOURS: [(i32, i32, u32); 8] = [
    (0, -1, W_CARD),  // 0  N
    (1, -1, W_DIAG),  // 1  NE
    (1, 0, W_CARD),   // 2  E
    (1, 1, W_DIAG),   // 3  SE
    (0, 1, W_CARD),   // 4  S
    (-1, 1, W_DIAG),  // 5  SW
    (-1, 0, W_CARD),  // 6  W
    (-1, -1, W_DIAG), // 7  NW
];

/// Direction byte written for a cell that is a goal or is unreachable.
pub const DIR_NONE: u8 = 255;

/// A batch of same-shaped fields sharing one allocation per column.
#[derive(Clone)]
pub struct FieldBatch {
    pub width: u32,
    pub height: u32,
    pub fields: u32,
    /// Per-cell **entry** cost: the price of stepping *into* this cell, before the chamfer
    /// weight multiplies it. `COST_BLOCKED` marks impassable terrain.
    pub cost: Vec<u32>,
    /// Per-cell distance-to-nearest-goal. Input: `0` at goals, `INF` elsewhere. Output: the
    /// least fixed point of the relaxation.
    pub dist: Vec<u32>,
}

impl FieldBatch {
    pub fn new(width: u32, height: u32, fields: u32) -> FieldBatch {
        let n = (width as usize) * (height as usize) * (fields as usize);
        FieldBatch {
            width,
            height,
            fields,
            cost: vec![1; n],
            dist: vec![INF; n],
        }
    }

    #[inline]
    pub fn cells_per_field(&self) -> usize {
        (self.width as usize) * (self.height as usize)
    }

    #[inline]
    pub fn total_cells(&self) -> usize {
        self.cells_per_field() * self.fields as usize
    }

    #[inline]
    pub fn index(&self, field: u32, x: u32, y: u32) -> usize {
        (field as usize) * self.cells_per_field() + (y as usize) * (self.width as usize) + x as usize
    }

    /// Reset every distance to `INF`. Costs are untouched.
    pub fn clear_dist(&mut self) {
        self.dist.fill(INF);
    }

    pub fn set_goal(&mut self, field: u32, x: u32, y: u32) {
        let i = self.index(field, x, y);
        self.dist[i] = 0;
    }

    /// Deterministic synthetic terrain: scattered rectangular obstacles plus a cheap
    /// "road" lattice, with one goal per field. Purely a benchmark fixture — it makes no
    /// claim about Rise of Nations terrain, which is not derived yet.
    pub fn synth_terrain(width: u32, height: u32, fields: u32, seed: u64) -> FieldBatch {
        let mut b = FieldBatch::new(width, height, fields);
        let mut s = seed | 1;
        let mut rnd = move || {
            s ^= s >> 12;
            s ^= s << 25;
            s ^= s >> 27;
            s.wrapping_mul(0x2545_f491_4f6c_dd1d)
        };
        let obstacles = ((width as u64 * height as u64) / 400).max(4) as u32;
        for f in 0..fields {
            // roads: every 16th row/column costs 1, open ground costs 3, so the solver has
            // a real preference structure rather than a uniform plane.
            for y in 0..height {
                for x in 0..width {
                    let i = b.index(f, x, y);
                    b.cost[i] = if x % 16 == 0 || y % 16 == 0 { 1 } else { 3 };
                }
            }
            for _ in 0..obstacles {
                let r = rnd();
                let ox = (r % width as u64) as u32;
                let oy = ((r >> 20) % height as u64) as u32;
                let ow = 2 + ((r >> 40) % 9) as u32;
                let oh = 2 + ((r >> 48) % 9) as u32;
                for y in oy..(oy + oh).min(height) {
                    for x in ox..(ox + ow).min(width) {
                        let i = b.index(f, x, y);
                        b.cost[i] = COST_BLOCKED;
                    }
                }
            }
            // goal near a corner, guaranteed passable
            let gx = (rnd() % (width as u64 / 4).max(1)) as u32;
            let gy = (rnd() % (height as u64 / 4).max(1)) as u32;
            let gi = b.index(f, gx, gy);
            b.cost[gi] = 1;
            b.dist[gi] = 0;
        }
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexing_is_field_major_and_row_major() {
        let b = FieldBatch::new(7, 5, 3);
        assert_eq!(b.index(0, 0, 0), 0);
        assert_eq!(b.index(0, 6, 4), 34);
        assert_eq!(b.index(1, 0, 0), 35);
        assert_eq!(b.index(2, 3, 2), 70 + 17);
        assert_eq!(b.total_cells(), 105);
    }

    #[test]
    fn synth_terrain_is_reproducible() {
        let a = FieldBatch::synth_terrain(64, 64, 4, 12345);
        let c = FieldBatch::synth_terrain(64, 64, 4, 12345);
        assert_eq!(a.cost, c.cost);
        assert_eq!(a.dist, c.dist);
        let d = FieldBatch::synth_terrain(64, 64, 4, 999);
        assert_ne!(a.cost, d.cost);
    }

    #[test]
    fn every_field_gets_exactly_one_goal() {
        let b = FieldBatch::synth_terrain(48, 48, 5, 7);
        for f in 0..b.fields {
            let base = f as usize * b.cells_per_field();
            let goals = b.dist[base..base + b.cells_per_field()]
                .iter()
                .filter(|&&d| d == 0)
                .count();
            assert_eq!(goals, 1, "field {f}");
        }
    }
}
