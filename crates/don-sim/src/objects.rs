//! `Objects` — the per-owner object registry, and the traversal order of
//! `Objects::process_all`.
//!
//! # Why this file is not an implementation detail
//!
//! `Objects::process_all` `0x0065DCE0` decides the order in which every unit, building
//! and wall in the game is updated, and that order **rotates every frame**. Because
//! object updates consume `game_random` draws, a fixed-order scheduler diverges from
//! retail inside a single tick. Reproducing the rotation is not an optimisation, it is
//! the difference between a port and a different game.
//!
//! # The banded index space
//!
//! Each owner slot holds one pointer array with three bands, and the band bounds come
//! straight out of the `Objects` layout in the PDB [measured]:
//!
//! | field | offset | band | index range |
//! |---|---|---|---|
//! | `lists : ObjectsArray[10]` | `+0x004` | — | the arrays themselves, stride 0x1C |
//! | `unit_mark : int[10]` | `+0x15C` | units | `[0, unit_mark[s])` |
//! | `build_mark : int[10]` | `+0x184` | buildings | `[2000, build_mark[s])` |
//! | `wall_mark : int[10]` | `+0x1AC` | walls | `[3000, wall_mark[s])` |
//!
//! The bases 2000 and 3000 are the literals the retail loops start at
//! (`0x0065DD5D`, `0x0065DDD1`), and the marks are the `int[10]` arrays those loops
//! bound themselves with. So `UnitData::o` — the `short` object index every unit carries
//! — is band-encoded, and `(who, o)` is the engine's object address.
//!
//! # The traversal, transcribed
//!
//! ```text
//! for i in 0..10:                      # ten owner slots
//!     s = (game.frame + i) % 10        # <-- ROTATES EVERY FRAME
//!     if leaders[s].active:
//!         for k in 0 .. unit_mark[s]:
//!             o = lists[s][k]
//!             if o.flags & 1:  o->vt[+0x9C]()      # ::process
//!             elif o.hold_frames != 0: o.hold_frames -= 1
//! for s in 0..8:                       # <-- EIGHT, not ten. See below.
//!     if leaders[s].active:
//!         for k in 2000 .. build_mark[s]: ...      # same active/hold rule
//!         for k in 3000 .. wall_mark[s]:  ...
//! if game.frame % 32 == 0: spawn wildlife            (draws game_random)
//! if game.frame % 64 == 0: Herd::process, round-robin
//! ```
//!
//! **The 8-vs-10 asymmetry is real.** The second loop is a pointer walk
//! `for (p = 0x00E3A390; p < 0x00E71AF0; p += 0x6EEC)`, and
//! `(0x00E71AF0 - 0x00E3A390) / 0x6EEC = 8` exactly [measured]. `Leaders` is
//! `sizeof 283,980` = ten `LeaderData` at stride `0x6EEC` plus 20 bytes, so slots 8 and
//! 9 exist and the *first* loop reaches them — but the building and wall bands are only
//! walked for slots 0..7. Slots 8 and 9 therefore hold units only, consistent with them
//! being nature/unowned.
//!
//! `flags & 1` is `SubObjectData::flags` bit 0 at +8, and `hold_frames` is
//! `ObjectData::hold_frames`, an `unsigned short` at **+50 = 0x32** — the exact offset
//! the decompiled loop decrements. That the PDB layout and the machine code agree here
//! is a useful independent check on both.

/// Owner slots the first band iterates. Ten, and the rotation is modulo this.
pub const OWNER_SLOTS: usize = 10;
/// Owner slots the building and wall bands iterate. Eight [measured].
pub const BANDED_SLOTS: usize = 8;
/// First index of the building band.
pub const BUILD_BAND_BASE: u32 = 2000;
/// First index of the wall band.
pub const WALL_BAND_BASE: u32 = 3000;
/// `frame % 32 == 0` gates the wildlife spawn (`0x0065DE7C`).
pub const WILDLIFE_PERIOD: i32 = 32;
/// `frame % 64 == 0` gates the round-robin herd step.
pub const HERD_PERIOD: i32 = 64;

/// Which band an object index falls in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Band {
    Unit,
    Build,
    Wall,
}

impl Band {
    #[inline]
    pub fn base(self) -> u32 {
        match self {
            Band::Unit => 0,
            Band::Build => BUILD_BAND_BASE,
            Band::Wall => WALL_BAND_BASE,
        }
    }
}

/// One owner's object list, stored per band.
///
/// Retail keeps a single sparse array with gaps between `unit_mark` and 2000 and between
/// `build_mark` and 3000. Storing the bands separately holds the same information
/// without reserving the gaps — a batch of thousands of small worlds cannot afford
/// 3,000 empty pointer slots per owner — and the engine's `o` index is recovered by
/// adding the band base.
#[derive(Clone, Default)]
pub struct Slot {
    units: Vec<u32>,
    builds: Vec<u32>,
    walls: Vec<u32>,
}

impl Slot {
    #[inline]
    pub fn band(&self, b: Band) -> &[u32] {
        match b {
            Band::Unit => &self.units,
            Band::Build => &self.builds,
            Band::Wall => &self.walls,
        }
    }

    /// `unit_mark[s]` / `build_mark[s]` / `wall_mark[s]` as retail stores them: the
    /// band base plus the band's length.
    #[inline]
    pub fn mark(&self, b: Band) -> u32 {
        b.base() + self.band(b).len() as u32
    }
}

/// The `Objects` singleton's traversal state.
#[derive(Clone)]
pub struct ObjectRegistry {
    slots: [Slot; OWNER_SLOTS],
    /// `leaders[s].flags & 1` — whether the slot participates at all.
    active: [bool; OWNER_SLOTS],
}

impl Default for ObjectRegistry {
    fn default() -> Self {
        ObjectRegistry::new()
    }
}

impl ObjectRegistry {
    pub fn new() -> ObjectRegistry {
        ObjectRegistry {
            slots: Default::default(),
            active: [false; OWNER_SLOTS],
        }
    }

    #[inline]
    pub fn slot(&self, s: usize) -> &Slot {
        &self.slots[s]
    }

    #[inline]
    pub fn is_active(&self, s: usize) -> bool {
        self.active[s]
    }

    pub fn set_active(&mut self, s: usize, on: bool) {
        self.active[s] = on;
    }

    pub fn active_slots(&self) -> impl Iterator<Item = usize> + '_ {
        (0..OWNER_SLOTS).filter(move |&s| self.active[s])
    }

    /// Register `row` in owner `who`'s band and return the engine-visible `o` index.
    ///
    /// Activating the slot on first insert mirrors the fact that a leader with objects
    /// is a leader that is playing.
    pub fn insert(&mut self, who: usize, band: Band, row: u32) -> u32 {
        let base = band.base();
        let s = &mut self.slots[who];
        let v = match band {
            Band::Unit => &mut s.units,
            Band::Build => &mut s.builds,
            Band::Wall => &mut s.walls,
        };
        v.push(row);
        self.active[who] = true;
        base + (v.len() as u32 - 1)
    }

    /// Remove the entry at `o`, swapping the band's last entry into the hole.
    ///
    /// Returns `(moved_row, new_o)` for the entry that moved, so the caller can repair
    /// the moved object's own `o` field. Retail compacts differently (it keeps the array
    /// dense by moving the tail down and rewriting `o`), but the observable invariant —
    /// a dense band with every member's `o` matching its position — is the same, and
    /// **the traversal order of a band after a removal is the one thing that differs**.
    /// It is recorded in `docs/tracks/sim-core.md` as an open divergence.
    pub fn remove(&mut self, who: usize, band: Band, o: u32) -> Option<(u32, u32)> {
        let base = band.base();
        let s = &mut self.slots[who];
        let v = match band {
            Band::Unit => &mut s.units,
            Band::Build => &mut s.builds,
            Band::Wall => &mut s.walls,
        };
        let idx = o.checked_sub(base)? as usize;
        if idx >= v.len() {
            return None;
        }
        let last = v.len() - 1;
        v.swap(idx, last);
        v.pop();
        if idx != last {
            Some((v[idx], base + idx as u32))
        } else {
            None
        }
    }

    /// Repoint the entry at `o` (used when a column row moves under a swap-remove).
    pub fn repoint(&mut self, who: usize, band: Band, o: u32, row: u32) {
        let base = band.base();
        let s = &mut self.slots[who];
        let v = match band {
            Band::Unit => &mut s.units,
            Band::Build => &mut s.builds,
            Band::Wall => &mut s.walls,
        };
        if let Some(i) = o.checked_sub(base) {
            if let Some(e) = v.get_mut(i as usize) {
                *e = row;
            }
        }
    }

    pub fn band_len(&self, who: usize, band: Band) -> usize {
        self.slots[who].band(band).len()
    }

    /// The owner-slot visiting order for `frame`: `(frame + i) % 10` for `i` in `0..10`.
    ///
    /// Split out from the traversal so it can be tested on its own — it is the single
    /// most divergence-prone line in the scheduler.
    #[inline]
    pub fn rotation(frame: i32) -> [usize; OWNER_SLOTS] {
        let mut out = [0usize; OWNER_SLOTS];
        for (i, o) in out.iter_mut().enumerate() {
            *o = (frame.wrapping_add(i as i32)).rem_euclid(OWNER_SLOTS as i32) as usize;
        }
        out
    }

    /// Every `(slot, band, position, row)` this frame visits, in retail's order.
    ///
    /// Materialising the order as a list is deliberate: the caller mutates world state
    /// while walking it, and a borrow-checker-friendly iterator over `&self` would
    /// forbid exactly that. It also makes the order directly assertable in a test.
    pub fn traversal(&self, frame: i32) -> Vec<(usize, Band, u32, u32)> {
        let mut out = Vec::with_capacity(
            (0..OWNER_SLOTS).map(|s| self.slots[s].units.len()).sum::<usize>() + 16,
        );
        // Band 0: ten slots, rotating.
        for s in ObjectRegistry::rotation(frame) {
            if !self.active[s] {
                continue;
            }
            for (k, &row) in self.slots[s].units.iter().enumerate() {
                out.push((s, Band::Unit, k as u32, row));
            }
        }
        // Bands 2000 and 3000: eight slots, fixed order, buildings before walls.
        for (s, slot) in self.slots.iter().enumerate().take(BANDED_SLOTS) {
            if !self.active[s] {
                continue;
            }
            for (k, &row) in slot.builds.iter().enumerate() {
                out.push((s, Band::Build, BUILD_BAND_BASE + k as u32, row));
            }
            for (k, &row) in slot.walls.iter().enumerate() {
                out.push((s, Band::Wall, WALL_BAND_BASE + k as u32, row));
            }
        }
        out
    }

    pub fn total_objects(&self) -> usize {
        self.slots
            .iter()
            .map(|s| s.units.len() + s.builds.len() + s.walls.len())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_advances_by_one_every_frame() {
        assert_eq!(ObjectRegistry::rotation(0), [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(ObjectRegistry::rotation(1), [1, 2, 3, 4, 5, 6, 7, 8, 9, 0]);
        assert_eq!(ObjectRegistry::rotation(7), [7, 8, 9, 0, 1, 2, 3, 4, 5, 6]);
        assert_eq!(ObjectRegistry::rotation(10), ObjectRegistry::rotation(0));
    }

    /// Negative frames cannot happen in a game, but `%` on a negative int in Rust is not
    /// what `idiv` does either; pin the behaviour so a frame counter that ever wraps
    /// fails loudly rather than indexing out of bounds.
    #[test]
    fn rotation_is_in_range_for_any_frame() {
        for f in [-1, -9, -10, -11, i32::MIN + 1, i32::MAX] {
            for s in ObjectRegistry::rotation(f) {
                assert!(s < OWNER_SLOTS);
            }
        }
    }

    #[test]
    fn bands_encode_the_engines_object_index() {
        let mut r = ObjectRegistry::new();
        assert_eq!(r.insert(3, Band::Unit, 100), 0);
        assert_eq!(r.insert(3, Band::Unit, 101), 1);
        assert_eq!(r.insert(3, Band::Build, 7), BUILD_BAND_BASE);
        assert_eq!(r.insert(3, Band::Wall, 9), WALL_BAND_BASE);
        assert_eq!(r.slot(3).mark(Band::Unit), 2);
        assert_eq!(r.slot(3).mark(Band::Build), BUILD_BAND_BASE + 1);
        assert_eq!(r.slot(3).mark(Band::Wall), WALL_BAND_BASE + 1);
    }

    /// The order the traversal produces is the whole point; assert it literally.
    #[test]
    fn traversal_visits_units_by_rotation_then_the_fixed_bands() {
        let mut r = ObjectRegistry::new();
        for s in 0..OWNER_SLOTS {
            r.insert(s, Band::Unit, s as u32 * 10);
        }
        let f0: Vec<usize> = r.traversal(0).iter().map(|e| e.0).collect();
        assert_eq!(f0, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        let f3: Vec<usize> = r.traversal(3).iter().map(|e| e.0).collect();
        assert_eq!(f3, vec![3, 4, 5, 6, 7, 8, 9, 0, 1, 2]);
    }

    /// Slots 8 and 9 get their units walked but never their buildings or walls.
    #[test]
    fn the_last_two_slots_are_units_only() {
        let mut r = ObjectRegistry::new();
        for s in 0..OWNER_SLOTS {
            r.insert(s, Band::Unit, 1);
            r.insert(s, Band::Build, 2);
            r.insert(s, Band::Wall, 3);
        }
        let t = r.traversal(0);
        let builds_for_8 = t.iter().filter(|e| e.0 >= BANDED_SLOTS && e.1 != Band::Unit).count();
        assert_eq!(builds_for_8, 0, "slots 8 and 9 must not have their build/wall bands walked");
        let builds_total = t.iter().filter(|e| e.1 == Band::Build).count();
        assert_eq!(builds_total, BANDED_SLOTS);
        let units_total = t.iter().filter(|e| e.1 == Band::Unit).count();
        assert_eq!(units_total, OWNER_SLOTS);
    }

    #[test]
    fn removal_reports_the_entry_that_moved() {
        let mut r = ObjectRegistry::new();
        r.insert(0, Band::Unit, 10);
        r.insert(0, Band::Unit, 11);
        r.insert(0, Band::Unit, 12);
        assert_eq!(r.remove(0, Band::Unit, 0), Some((12, 0)));
        assert_eq!(r.band_len(0, Band::Unit), 2);
        assert_eq!(r.remove(0, Band::Unit, 1), None, "removing the tail moves nothing");
        assert_eq!(r.remove(0, Band::Unit, 99), None);
    }
}
