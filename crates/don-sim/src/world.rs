//! Fixed-capacity structure-of-arrays world state.

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

/// Per-world unit capacity. Fixed so the whole world is one allocation and batch stepping
/// never allocates in steady state. The real population cap is a rule value we have not
/// derived yet; this is a provisioning decision, not a claim about the game.
pub const MAX_UNITS: usize = 4096;

/// One simulation world in structure-of-arrays form.
///
/// Arrays are parallel and indexed by unit slot. `alive` is the occupancy mask; freed
/// slots are recycled rather than compacted, so indices are stable across ticks.
#[derive(Clone)]
pub struct World {
    // ---- spatial ----
    pub pos_x: Vec<i32>,
    pub pos_y: Vec<i32>,
    pub vel_x: Vec<i32>,
    pub vel_y: Vec<i32>,

    // ---- combat-adjacent (names mirror the binary's own attribute names) ----
    pub hits: Vec<i32>,
    pub armor: Vec<i16>,
    pub attack: Vec<i16>,
    pub recharge: Vec<i16>,
    /// Frames remaining before this unit may attack again.
    pub cooldown: Vec<i16>,

    // ---- bookkeeping ----
    pub owner: Vec<u8>,
    pub alive: Vec<bool>,
    free_slots: Vec<u32>,
    live_count: u32,

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
    pub fn new(seed: u64) -> World {
        let n = MAX_UNITS;
        World {
            pos_x: vec![0; n],
            pos_y: vec![0; n],
            vel_x: vec![0; n],
            vel_y: vec![0; n],
            hits: vec![0; n],
            armor: vec![0; n],
            attack: vec![0; n],
            recharge: vec![0; n],
            cooldown: vec![0; n],
            owner: vec![0; n],
            alive: vec![false; n],
            free_slots: (0..n as u32).rev().collect(),
            live_count: 0,
            frame: 0,
            rng: seed | 1,
        }
    }

    #[inline]
    pub fn live_count(&self) -> u32 {
        self.live_count
    }

    #[inline]
    fn next_rand(&mut self) -> u64 {
        // xorshift64*: deterministic and cheap. PLACEHOLDER, see field docs.
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn spawn(&mut self, owner: u8) -> Option<u32> {
        let slot = self.free_slots.pop()?;
        let i = slot as usize;
        let r = self.next_rand();
        self.pos_x[i] = (r as u32 % (256 * SUBTILE as u32)) as i32;
        self.pos_y[i] = ((r >> 32) as u32 % (256 * SUBTILE as u32)) as i32;
        self.vel_x[i] = ((r >> 8) as i8) as i32;
        self.vel_y[i] = ((r >> 16) as i8) as i32;
        self.hits[i] = 100;
        self.armor[i] = 2;
        self.attack[i] = 10;
        self.recharge[i] = 15;
        self.cooldown[i] = 0;
        self.owner[i] = owner;
        self.alive[i] = true;
        self.live_count += 1;
        Some(slot)
    }

    pub fn despawn(&mut self, slot: u32) {
        let i = slot as usize;
        if self.alive[i] {
            self.alive[i] = false;
            self.free_slots.push(slot);
            self.live_count -= 1;
        }
    }

    /// Advance one frame.
    ///
    /// PLACEHOLDER mechanics throughout — see the crate docs. The purpose is the memory
    /// access pattern, so throughput of this layout can be measured before real systems
    /// land.
    pub fn step(&mut self) {
        self.sys_movement();
        self.sys_cooldown();
        self.frame += 1;
    }

    /// PLACEHOLDER: integrate position at 1/192-tile granularity, wrapping at the map edge.
    /// The real integrator, its speed derivation and its collision behaviour are underived.
    #[inline]
    fn sys_movement(&mut self) {
        const LIMIT: i32 = 256 * SUBTILE;
        let n = MAX_UNITS;
        // Straight-line passes over parallel arrays: the shape a vectorised system wants.
        for i in 0..n {
            if !self.alive[i] {
                continue;
            }
            let mut x = self.pos_x[i] + self.vel_x[i];
            let mut y = self.pos_y[i] + self.vel_y[i];
            if x < 0 {
                x += LIMIT;
            } else if x >= LIMIT {
                x -= LIMIT;
            }
            if y < 0 {
                y += LIMIT;
            } else if y >= LIMIT {
                y -= LIMIT;
            }
            self.pos_x[i] = x;
            self.pos_y[i] = y;
        }
    }

    /// PLACEHOLDER: decrement attack cooldowns. Real recharge semantics are underived.
    #[inline]
    fn sys_cooldown(&mut self) {
        for i in 0..MAX_UNITS {
            if self.alive[i] && self.cooldown[i] > 0 {
                self.cooldown[i] -= 1;
            }
        }
    }

    /// Order-independent digest of sim-critical state.
    ///
    /// Deliberately *not* modelled on the engine's own checksum: measurement confirms a
    /// `CheckSum` / `DataWalk` visitor exists in the binary, and once its field set is
    /// recovered this should be replaced by that definition, since the engine's notion of
    /// "sim-critical" is the authoritative one.
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for i in 0..MAX_UNITS {
            if !self.alive[i] {
                continue;
            }
            for v in [self.pos_x[i] as u64, self.pos_y[i] as u64, self.hits[i] as u64] {
                h ^= v;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
        h ^ self.frame
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
        w.despawn(a);
        assert_eq!(w.live_count(), 1);
        w.despawn(a); // idempotent
        assert_eq!(w.live_count(), 1);
        w.despawn(b);
        assert_eq!(w.live_count(), 0);
    }

    #[test]
    fn capacity_is_bounded_and_slots_recycle() {
        let mut w = World::new(1);
        for _ in 0..MAX_UNITS {
            assert!(w.spawn(0).is_some());
        }
        assert!(w.spawn(0).is_none(), "must not exceed fixed capacity");
        w.despawn(0);
        assert!(w.spawn(0).is_some(), "freed slot must be reusable");
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
        for i in 0..MAX_UNITS {
            if w.alive[i] {
                assert!((0..256 * SUBTILE).contains(&w.pos_x[i]));
                assert!((0..256 * SUBTILE).contains(&w.pos_y[i]));
            }
        }
    }
}
