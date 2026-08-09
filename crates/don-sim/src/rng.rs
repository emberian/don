//! The simulation RNG, ported from `Random` in `basic/random.cpp`.
//!
//! # Provenance
//!
//! `Random` is one `int` of state. Three entry points matter to the simulation:
//!
//! | function | VA | note |
//! |---|---|---|
//! | `int Random::get(int, int)` | `0x00A39D70` | the one the sim calls |
//! | `Random::reseed(int)` | `0x00A39D30` | an XOR swap: installs the new seed, **returns the old** |
//! | free `random(int, int)` | `0x00A39D40` | thunk: `this = internal_random 0x00EB697C` |
//!
//! The generator is the Numerical-Recipes LCG, `s <- s*0x0019660D + 0x3C6EF35F`
//! (1664525 / 1013904223) [measured at `0x00A39EA5`].
//!
//! `Random::get(int,int)`'s body, transcribed from `0x00A39E83..0x00A39EC0`:
//!
//! ```text
//! 00a39e83  cmp  ebx, edi          ; lo == hi -> return lo
//! 00a39e9d  jle  ...               ; lo > hi  -> xor-swap them
//! 00a39ea5  imul ecx, [eax], 0x19660d
//! 00a39eab  sub  edi, ebx          ; span = hi - lo
//! 00a39ead  add  ecx, 0x3c6ef35f
//! 00a39eb3  mov  [eax], ecx        ; state <- s
//! 00a39eb5  movzx eax, cx          ; ONLY THE LOW 16 BITS ARE USED
//! 00a39eb8  imul eax, edi
//! 00a39ebb  shr  eax, 0x10         ; logical
//! 00a39ebe  add  eax, ebx
//! ```
//!
//! Two consequences that a naive `lo + s % span` gets wrong, and both are load-bearing
//! for lockstep:
//!
//! * The range is **half-open, `[lo, hi)`**. `(0..=0xFFFF) * span >> 16` never reaches
//!   `span`, so `hi` is unreachable. Call sites like the wildlife spawn's
//!   `Random::get(0, 0xFFFF) % n` are drawing from `[0, 0xFFFF)`.
//! * Only the **low 16 bits** of the LCG state feed the result — the high bits, which is
//!   where an LCG's good bits live, are discarded. The stream's quality is irrelevant;
//!   reproducing it exactly is the entire point.
//!
//! Entry does a range check (`lo > 0xFFFF || hi > 0xFFFF`) that branches into a
//! diagnostic path gated on the byte at `0x00EE13A8` and then rejoins this same
//! arithmetic. We do not model the diagnostic.
//!
//! # Fidelity
//!
//! **Tier C.** Transcribed from the disassembly and self-tested here; *no* oracle
//! execution has compared it against retail. It is a good candidate for the next
//! `crates/oracle/src/registry.rs` case — the function is leaf-ish and takes two ints.

/// One `Random` object: a single `int` of state, as in the binary.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Random {
    state: i32,
}

impl Random {
    /// `0x00A39EA5`.
    pub const MUL: i32 = 0x0019_660D;
    /// `0x00A39EAD`.
    pub const ADD: i32 = 0x3C6E_F35F;

    pub const fn new(seed: i32) -> Random {
        Random { state: seed }
    }

    #[inline]
    pub const fn state(&self) -> i32 {
        self.state
    }

    /// `Random::reseed` `0x00A39D30` — an XOR swap, so it **returns the previous seed**.
    #[inline]
    pub fn reseed(&mut self, seed: i32) -> i32 {
        let old = self.state;
        self.state = seed;
        old
    }

    /// Advance the LCG and return the new state. Every draw consumes exactly one of
    /// these, which is what makes call-site *order* a lockstep concern.
    #[inline]
    pub fn advance(&mut self) -> i32 {
        self.state = self.state.wrapping_mul(Self::MUL).wrapping_add(Self::ADD);
        self.state
    }

    /// `int Random::get(int lo, int hi)` `0x00A39D70`. Half-open: `hi` is unreachable.
    #[inline]
    pub fn get(&mut self, lo: i32, hi: i32) -> i32 {
        let (mut lo, mut hi) = (lo, hi);
        if lo == hi {
            return lo;
        }
        if lo > hi {
            std::mem::swap(&mut lo, &mut hi);
        }
        let s = self.advance();
        let span = hi.wrapping_sub(lo);
        // movzx eax, cx  -- low 16 bits, zero extended.
        let r = (s as u32 & 0xFFFF) as i32;
        let scaled = ((r.wrapping_mul(span) as u32) >> 16) as i32;
        scaled.wrapping_add(lo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two constants are the whole claim; freeze them.
    #[test]
    fn lcg_constants_are_the_measured_ones() {
        assert_eq!(Random::MUL, 1_664_525);
        assert_eq!(Random::ADD, 1_013_904_223);
    }

    #[test]
    fn reseed_returns_the_old_seed() {
        let mut r = Random::new(7);
        assert_eq!(r.reseed(99), 7);
        assert_eq!(r.state(), 99);
    }

    /// Half-open, not inclusive. This is the property a `% span` port gets wrong.
    #[test]
    fn range_is_half_open() {
        let mut r = Random::new(0x1234_5678);
        let mut saw_lo = false;
        for _ in 0..200_000 {
            let v = r.get(10, 20);
            assert!((10..20).contains(&v), "{v} escaped [10, 20)");
            saw_lo |= v == 10;
        }
        assert!(saw_lo, "test is vacuous if the low end is never hit");
    }

    #[test]
    fn equal_bounds_consume_no_draw() {
        let mut r = Random::new(5);
        let before = r.state();
        assert_eq!(r.get(3, 3), 3);
        assert_eq!(
            r.state(),
            before,
            "lo == hi must return before touching the state"
        );
    }

    #[test]
    fn inverted_bounds_are_swapped() {
        let mut a = Random::new(11);
        let mut b = Random::new(11);
        assert_eq!(a.get(50, 10), b.get(10, 50));
    }

    /// A draw is one LCG step, always. Order of call sites is therefore observable.
    #[test]
    fn one_draw_is_one_step() {
        let mut a = Random::new(1);
        let mut b = Random::new(1);
        a.get(0, 0xFFFF);
        b.advance();
        assert_eq!(a.state(), b.state());
    }
}
