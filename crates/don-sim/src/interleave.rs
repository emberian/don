//! Cross-world lane-major stepping: one SIMD lane = one world.
//!
//! This is the JUX / Madrona batch layout, and this module exists to **measure** whether
//! it beats per-world loops for this simulation rather than to assume it does. It is an
//! experiment with a real implementation, not a replacement for [`crate::Batch`]: the
//! dense [`World`] columns stay authoritative, and a [`LaneBatch`] is *compiled* from them
//! and written back.
//!
//! # The layout
//!
//! Worlds are taken in groups of `lanes`. Within a group, a column is stored row-major
//! over rows and lane-minor over worlds: element (row, lane) at `row * lanes + lane`. One
//! aligned vector load therefore holds the same unit row of `lanes` different worlds, and
//! a per-world scalar (a tech multiplier, a difficulty setting) would broadcast into a
//! lane rather than forcing a scalar loop.
//!
//! Rows past a world's live count are padded with zeroes. The tick systems map zero to
//! zero (`0 + 0 = 0` needs no wrap; a zero cooldown does not decrement), so padding lanes
//! are processed unmasked and provably contribute nothing — no mask register, no branch.
//! `write_back` copies only each world's live rows, so padding can never leak into state.
//!
//! # What this experiment can and cannot show
//!
//! The systems it steps are the placeholder element-wise ones, and for a purely
//! element-wise system lane-major and world-dense arrays do *the same arithmetic in the
//! same order*; the layout can only move memory-traffic and loop-overhead costs. So this
//! measures exactly one thing honestly — whether flattening 4096 small per-world loops
//! into a few long ones pays — and it does **not** speak to the case the pattern is really
//! for, which is per-world control-flow divergence. Read the numbers in
//! `docs/derivation/simd-batch.md` with that boundary in mind.

use crate::batch::Batch;
use crate::simd;
use crate::world::{World, MAP_SPAN};

/// Worlds per group. Four 32-bit lanes is one 128-bit vector, the width both NEON and
/// SSE2 give unconditionally.
pub const LANES: usize = 4;

struct Group {
    /// Rows provisioned per world in this group (the group's max live count).
    rows: usize,
    /// Live row count per lane.
    live: [usize; LANES],
    /// Worlds actually present (the last group may be partial).
    used: usize,
    pos_x: Vec<i32>,
    pos_y: Vec<i32>,
    /// The cached per-frame movement step (`sinx(angle, speed)` / `-cosx(...)`), which
    /// is what `World::step_hot` integrates by. It is constant across a lane-major run
    /// because nothing here changes a unit's facing — that is the point: this experiment
    /// measures the element-wise arithmetic, not the order dispatch.
    step_x: Vec<i32>,
    step_y: Vec<i32>,
    /// `ObjectData::hold_frames`.
    cooldown: Vec<i16>,
}

impl Group {
    #[inline]
    fn step(&mut self) {
        let n = self.rows * LANES;
        simd::integrate_wrap(&mut self.pos_x[..n], &self.step_x[..n], MAP_SPAN);
        simd::integrate_wrap(&mut self.pos_y[..n], &self.step_y[..n], MAP_SPAN);
        simd::tick_down(&mut self.cooldown[..n]);
    }
}

/// A batch compiled into lane-major groups.
pub struct LaneBatch {
    groups: Vec<Group>,
    frames_run: u64,
}

impl LaneBatch {
    /// Transpose a batch's hot columns into lane-major groups.
    pub fn from_batch(b: &Batch) -> LaneBatch {
        LaneBatch::from_worlds(&b.worlds)
    }

    pub fn from_worlds(worlds: &[World]) -> LaneBatch {
        let mut groups = Vec::with_capacity(worlds.len().div_ceil(LANES));
        for chunk in worlds.chunks(LANES) {
            let rows = chunk
                .iter()
                .map(|w| w.live_count() as usize)
                .max()
                .unwrap_or(0);
            let n = rows * LANES;
            let mut g = Group {
                rows,
                live: [0; LANES],
                used: chunk.len(),
                pos_x: vec![0; n],
                pos_y: vec![0; n],
                step_x: vec![0; n],
                step_y: vec![0; n],
                cooldown: vec![0; n],
            };
            for (lane, w) in chunk.iter().enumerate() {
                let live = w.live_count() as usize;
                g.live[lane] = live;
                for row in 0..live {
                    let k = row * LANES + lane;
                    g.pos_x[k] = w.pos_x()[row];
                    g.pos_y[k] = w.pos_y()[row];
                    g.step_x[k] = w.move_step_x()[row];
                    g.step_y[k] = w.move_step_y()[row];
                    g.cooldown[k] = w.cooldown()[row];
                }
            }
            groups.push(g);
        }
        LaneBatch {
            groups,
            frames_run: 0,
        }
    }

    pub fn groups(&self) -> usize {
        self.groups.len()
    }

    /// Advance every world in the batch by one frame.
    pub fn step(&mut self) {
        for g in &mut self.groups {
            g.step();
        }
        self.frames_run += 1;
    }

    pub fn run_serial(&mut self, frames: usize) {
        for g in &mut self.groups {
            for _ in 0..frames {
                g.step();
            }
        }
        self.frames_run += frames as u64;
    }

    /// Same dynamic-pull scheduling as [`Batch::run_parallel`], one group per work item.
    pub fn run_parallel(&mut self, frames: usize, threads: usize) {
        let threads = threads.max(1);
        if frames == 0 {
            return;
        }
        if threads == 1 || self.groups.len() < 2 {
            return self.run_serial(frames);
        }
        let grab = (self.groups.len() / (threads * 4)).max(1);
        let queue = std::sync::Mutex::new(self.groups.chunks_mut(grab));
        std::thread::scope(|scope| {
            for _ in 0..threads {
                let queue = &queue;
                scope.spawn(move || loop {
                    let next = queue.lock().expect("worker panicked").next();
                    let Some(part) = next else { break };
                    for g in part {
                        for _ in 0..frames {
                            g.step();
                        }
                    }
                });
            }
        });
        self.frames_run += frames as u64;
    }

    /// Copy the stepped hot columns back into the authoritative dense worlds.
    ///
    /// Only live rows are copied, so lane padding cannot become state. The worlds' frame
    /// counters are advanced by the number of frames this batch ran, keeping the two
    /// representations telling the same story about time.
    pub fn write_back(&self, b: &mut Batch) {
        for (gi, g) in self.groups.iter().enumerate() {
            for lane in 0..g.used {
                let w = &mut b.worlds[gi * LANES + lane];
                let live = g.live[lane];
                assert_eq!(
                    live,
                    w.live_count() as usize,
                    "world population changed under a LaneBatch"
                );
                for row in 0..live {
                    let k = row * LANES + lane;
                    w.set_pos(row, g.pos_x[k], g.pos_y[k]);
                    w.cooldown_mut()[row] = g.cooldown[k];
                }
                w.advance_frames(self.frames_run as i32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The experiment is only worth measuring if it computes the same thing. Bit-identical
    /// or it is not an alternative layout, it is a different simulation.
    ///
    /// The reference is [`Batch::run_hot_serial`], **not** `run_serial`: a `LaneBatch`
    /// mirrors `World::step_hot`, the element-wise subset, not `Game::do_frame`.
    /// `energise` is not optional — the kernels map zero to zero, so a batch of resting
    /// units would make this pass without computing anything.
    #[test]
    fn lane_major_stepping_matches_per_world_stepping() {
        for (worlds, units, frames) in [
            (4usize, 16usize, 33usize),
            (9, 1, 5),
            (17, 64, 101),
            (1, 7, 9),
        ] {
            let mut reference = Batch::with_capacity(worlds, 0xC0FFEE, units.max(1));
            reference.populate(units);
            reference.energise();
            let mut target = Batch::with_capacity(worlds, 0xC0FFEE, units.max(1));
            target.populate(units);
            target.energise();
            let before = target.digest();

            reference.run_hot_serial(frames);
            let mut lanes = LaneBatch::from_batch(&target);
            lanes.run_serial(frames);
            lanes.write_back(&mut target);

            if units > 0 {
                assert_ne!(
                    target.digest(),
                    before,
                    "the run must actually change state"
                );
            }
            assert_eq!(
                target.digest(),
                reference.digest(),
                "lane-major diverged at {worlds} worlds x {units} units over {frames} frames"
            );
            for (a, b) in target.worlds.iter().zip(reference.worlds.iter()) {
                assert_eq!(a.pos_x(), b.pos_x());
                assert_eq!(a.pos_y(), b.pos_y());
                assert_eq!(a.cooldown(), b.cooldown());
                assert_eq!(a.frame, b.frame);
            }
        }
    }

    #[test]
    fn ragged_populations_match_per_world_stepping() {
        let build = || {
            let mut b = Batch::with_capacity(11, 0x1234, 128);
            for (i, w) in b.worlds.iter_mut().enumerate() {
                for k in 0..(i * 13) % 128 {
                    w.spawn((k % 3) as u8).unwrap();
                }
            }
            b.energise();
            b
        };
        let mut reference = build();
        let before = reference.digest();
        reference.run_hot_serial(50);
        assert_ne!(
            reference.digest(),
            before,
            "the reference run must move state"
        );
        let mut target = build();
        let mut lanes = LaneBatch::from_batch(&target);
        lanes.run_parallel(50, 3);
        lanes.write_back(&mut target);
        assert_eq!(target.digest(), reference.digest());
    }
}
