//! Vector kernels for the element-wise tick systems.
//!
//! # The contract
//!
//! Every kernel ships in three forms (table below), one of which is a scalar reference
//! that *defines* the semantics. Every other form must be **bit-identical** to it on every
//! input — not close, identical. Determinism is the point of this project, and a kernel
//! that is faster but differs in the low bit is worthless. `world.rs` carries a test that
//! steps three identical worlds, one through each path, and compares columns and digest at
//! sixteen population sizes chosen to straddle the vector widths and leave awkward tails.
//!
//! # Why bit-identity is currently cheap, and when it stops being
//!
//! Every column a tick touches today is an **integer**. Integer SIMD is the scalar
//! computation done four or eight at a time: `vaddq_s32` is `wrapping_add` per lane, and
//! comparisons produce exact all-ones/all-zeros masks, so the equivalence is structural
//! rather than lucky.
//!
//! That stops the moment a derived system integrates in `f32` — and it will, because the
//! binary is SSE single-precision throughout (`docs/binary-ground-truth.md`). At that
//! point the hazards are contraction (an `a*b+c` fused into one rounding on AArch64 but
//! two on baseline SSE), reassociation in a vectorised reduction, and any horizontal sum
//! whose tree shape depends on the lane count. The discipline for float kernels will be:
//! no FMA contraction, no reassociation, lane count must not change the arithmetic tree.
//! Adding a float kernel here without honouring that would break cross-platform
//! determinism silently, which is the worst way for it to break.
//!
//! # Which path actually ships
//!
//! Three forms of each kernel exist and all three are asserted bit-identical:
//!
//! | form | what it is |
//! |---|---|
//! | `*_scalar` | branchy reference; defines the semantics |
//! | `*_portable` | branchless, no unsafe, shaped for an autovectoriser |
//! | [`hand`] | explicit NEON / SSE2 intrinsics, 4 vectors per iteration |
//!
//! `integrate_wrap` and `tick_down` — what [`crate::World::step`] calls — dispatch to the
//! **portable** form on every target, because that is what measured fastest: decisively on
//! x86-64 (+11 to +15%) and within noise on aarch64. The hand-written path is compiled and
//! tested everywhere regardless, and `don-bench` carries a row that re-runs the comparison
//! on any new machine. See [`integrate_wrap`] for the numbers and for how to measure this
//! without fooling yourself.
//!
//! Selection is by `cfg` at compile time, so no hot loop carries a runtime branch. There
//! is no AVX2 path: it would need runtime detection, and adding an unmeasured one on a
//! machine that cannot run it would be exactly the kind of plausible-but-unchecked work
//! this project forbids.

/// PLACEHOLDER integrator, scalar reference.
///
/// `pos[i] += vel[i]`, then wrapped into `0..span` by a single add or subtract — the
/// engine's real integrator, its speed derivation and its collision behaviour are all
/// underived, so this defines only the *shape* of the work.
///
/// The add is `wrapping_add`: for the values the world holds (`|pos| < span`, `|vel| <=
/// 128`) it cannot overflow, and specifying the wrap makes the vector path exactly equal
/// on every input rather than only on in-range ones.
#[inline]
pub fn integrate_wrap_scalar(pos: &mut [i32], vel: &[i32], span: i32) {
    debug_assert_eq!(pos.len(), vel.len());
    debug_assert!(span > 0);
    for (p, v) in pos.iter_mut().zip(vel.iter()) {
        let mut x = p.wrapping_add(*v);
        if x < 0 {
            x = x.wrapping_add(span);
        } else if x >= span {
            x = x.wrapping_sub(span);
        }
        *p = x;
    }
}

/// PLACEHOLDER cooldown decay, scalar reference: `if c > 0 { c -= 1 }`.
///
/// Values at or below zero are left exactly alone (they cannot arise from the spawn path,
/// but the semantics are defined for them so the vector path can be compared on them).
#[inline]
pub fn tick_down_scalar(counters: &mut [i16]) {
    for c in counters.iter_mut() {
        if *c > 0 {
            *c -= 1;
        }
    }
}

/// Branchless portable form of [`integrate_wrap_scalar`], for targets with no hand-written
/// path and as the honest control for "did the intrinsics earn their keep".
///
/// `x >> 31` is an arithmetic shift, so it is all-ones exactly when `x` is negative; the
/// `>=` comparison is widened to a mask the same way. This is the identical computation
/// the vector paths do, written so that any autovectoriser can see it.
#[inline]
pub fn integrate_wrap_portable(pos: &mut [i32], vel: &[i32], span: i32) {
    debug_assert_eq!(pos.len(), vel.len());
    debug_assert!(span > 0);
    for (p, v) in pos.iter_mut().zip(vel.iter()) {
        let x = p.wrapping_add(*v);
        let lt = x >> 31; // -1 when x < 0
        let ge = -((x >= span) as i32); // -1 when x >= span
        *p = x.wrapping_add(span & lt).wrapping_sub(span & ge);
    }
}

/// Branchless portable form of [`tick_down_scalar`].
#[inline]
pub fn tick_down_portable(counters: &mut [i16]) {
    for c in counters.iter_mut() {
        *c -= (*c > 0) as i16;
    }
}

/// The production integrator: the branchless portable kernel.
///
/// **This is a measured choice and the measurement went against the hand-written code.**
/// All three forms are bit-identical, so the choice is only about speed. Comparing the two
/// vector forms *properly* means building twice and changing only the line this function
/// dispatches on — comparing them through two different `World` entry points is worthless,
/// because on hbox two entry points into bit-identical SSE2 code measured 3302 vs 3713
/// Ms/s from code layout alone, which is larger than the effect being measured. That
/// artifact briefly made the hand path look like the x86 winner; it is not.
///
/// Two builds, same entry point, median of 3 (Ms/s unit-steps):
///
/// | machine | row | portable | hand |
/// |---|---|---|---|
/// | i9-12900, SSE2, 1t | 1 world x 512u | **3723** | 3336 |
/// | i9-12900, SSE2, 4t | 256w x 256u, run | **7494** | 6723 |
/// | i9-12900, SSE2, 4t | 4096w x 64u, run | **7324** | 6422 |
/// | M2 Max, NEON, 1t | 1 world x 512u | **3396** | 3303 |
/// | M2 Max, NEON, 12t | 256w x 256u, run | 18045 | **18748** |
///
/// x86-64 favours the portable form by 11-15%, decisively; aarch64 is a wash — single
/// thread favours portable by 2.8%, the contended 12-thread rows favour hand by ~3%, and
/// both are inside the run-to-run spread of a shared laptop. So: portable everywhere.
///
/// The large win was never the intrinsics. Against the branchy scalar reference the
/// branchless form is **1.47x on M2 and 2.58x on x86-64**, and that came from removing the
/// branch and iterating a dense row region — not from writing vectors by hand.
///
/// [`hand`] stays exported and tested on every target anyway, because the situation
/// reverses for the float systems this crate is waiting on: LLVM may only vectorise float
/// code by reassociating it, which changes results, so a deterministic float kernel must be
/// written by hand. The scaffolding — kernel pairs, bit-identity tests over adversarial
/// inputs, a benchmark row that compares them — is what carries over.
#[inline]
pub fn integrate_wrap(pos: &mut [i32], vel: &[i32], span: i32) {
    integrate_wrap_portable(pos, vel, span)
}

/// The production cooldown decay. Same measured choice as [`integrate_wrap`].
#[inline]
pub fn tick_down(counters: &mut [i16]) {
    tick_down_portable(counters)
}

/// Which kernel form [`integrate_wrap`] dispatches to on this target.
pub const fn shipped_path() -> &'static str {
    "portable branchless"
}

/// Explicitly vectorised kernels, selected by `cfg` at compile time.
///
/// Bit-identical to the scalar reference on every input — integer SIMD is the scalar
/// computation done four or eight at a time, and the tests compare them on adversarial
/// inputs at every length from 0 to 39. Where no hand-written path exists for the target,
/// these forward to the portable kernels, so callers never have to `cfg` themselves.
pub mod hand {
    /// Hand-written integrator for this target.
    #[inline]
    pub fn integrate_wrap(pos: &mut [i32], vel: &[i32], span: i32) {
        debug_assert_eq!(pos.len(), vel.len());
        debug_assert!(span > 0);

        #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
        // SAFETY: NEON is a compile-time-guaranteed target feature here, and the callee
        // reads and writes only within the two slices it is given.
        unsafe {
            super::neon::integrate_wrap(pos, vel, span)
        }

        #[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
        // SAFETY: SSE2 is a compile-time-guaranteed target feature here (it is baseline
        // for x86-64), and the callee stays inside the two slices it is given.
        unsafe {
            super::sse2::integrate_wrap(pos, vel, span)
        }

        #[cfg(not(any(
            all(target_arch = "aarch64", target_feature = "neon"),
            all(target_arch = "x86_64", target_feature = "sse2")
        )))]
        super::integrate_wrap_portable(pos, vel, span)
    }

    /// Hand-written cooldown decay for this target.
    #[inline]
    pub fn tick_down(counters: &mut [i16]) {
        #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
        // SAFETY: as above — NEON is guaranteed by cfg, accesses stay in bounds.
        unsafe {
            super::neon::tick_down(counters)
        }

        #[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
        // SAFETY: as above — SSE2 is guaranteed by cfg, accesses stay in bounds.
        unsafe {
            super::sse2::tick_down(counters)
        }

        #[cfg(not(any(
            all(target_arch = "aarch64", target_feature = "neon"),
            all(target_arch = "x86_64", target_feature = "sse2")
        )))]
        super::tick_down_portable(counters)
    }
}

/// Which hand-written path this build compiled, for benchmark and report honesty.
pub const fn hand_path() -> &'static str {
    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    {
        "neon-128 (4x unrolled)"
    }
    #[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
    {
        "sse2-128 (4x unrolled)"
    }
    #[cfg(not(any(
        all(target_arch = "aarch64", target_feature = "neon"),
        all(target_arch = "x86_64", target_feature = "sse2")
    )))]
    {
        "none for this target (falls back to portable)"
    }
}

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
mod neon {
    use core::arch::aarch64::*;

    /// # Safety
    /// Caller guarantees NEON (guaranteed by `cfg` at every call site) and `pos.len() ==
    /// vel.len()`.
    /// One 128-bit lane group: the whole kernel, so the unrolled body is four calls to
    /// this and cannot drift from the tail's version of the arithmetic.
    ///
    /// # Safety
    /// `p` and `v` must each be valid for 4 `i32`.
    #[inline(always)]
    unsafe fn quad(p: *mut i32, v: *const i32, vspan: int32x4_t, vzero: int32x4_t) {
        // SAFETY: caller guarantees four readable elements at each pointer.
        let x = vaddq_s32(unsafe { vld1q_s32(p) }, unsafe { vld1q_s32(v) }); // wrapping
        // Exactly the scalar branches: the two conditions are mutually exclusive for
        // span > 0, so adding one masked span and subtracting the other is the same as
        // the if/else-if chain.
        let lt = vreinterpretq_s32_u32(vcltq_s32(x, vzero)); // all-ones where x < 0
        let ge = vreinterpretq_s32_u32(vcgeq_s32(x, vspan)); // all-ones where x >= span
        let x = vaddq_s32(x, vandq_s32(vspan, lt));
        let x = vsubq_s32(x, vandq_s32(vspan, ge));
        // SAFETY: caller guarantees four writable elements at `p`.
        unsafe { vst1q_s32(p, x) };
    }

    /// # Safety
    /// Caller guarantees NEON (guaranteed by `cfg` at every call site) and `pos.len() ==
    /// vel.len()`.
    ///
    /// Unrolled to four vectors per iteration. That is not decoration: measured against
    /// the autovectorised portable kernel, a one-vector-per-iteration loop *loses* (2962
    /// vs 3337 Ms/s unit-steps on an M2 Max), because LLVM interleaves four vectors and
    /// keeps the load/store ports busy while a single dependent chain does not.
    #[inline]
    pub unsafe fn integrate_wrap(pos: &mut [i32], vel: &[i32], span: i32) {
        let n = pos.len();
        let vspan = vdupq_n_s32(span);
        let vzero = vdupq_n_s32(0);
        let p0 = pos.as_mut_ptr();
        let v0 = vel.as_ptr();
        let wide = n & !15;
        let mut i = 0;
        while i < wide {
            // SAFETY: `i + 16 <= wide <= n`, and both slices are `n` long.
            unsafe {
                quad(p0.add(i), v0.add(i), vspan, vzero);
                quad(p0.add(i + 4), v0.add(i + 4), vspan, vzero);
                quad(p0.add(i + 8), v0.add(i + 8), vspan, vzero);
                quad(p0.add(i + 12), v0.add(i + 12), vspan, vzero);
            }
            i += 16;
        }
        let body = n & !3;
        while i < body {
            // SAFETY: `i + 4 <= body <= n`.
            unsafe { quad(p0.add(i), v0.add(i), vspan, vzero) };
            i += 4;
        }
        super::integrate_wrap_scalar(&mut pos[body..], &vel[body..], span);
    }

    /// # Safety
    /// Caller guarantees NEON (guaranteed by `cfg` at every call site).
    /// # Safety
    /// `c` must be valid for 8 `i16`.
    #[inline(always)]
    unsafe fn octet(c: *mut i16, vzero: int16x8_t) {
        // SAFETY: caller guarantees eight readable elements.
        let v = unsafe { vld1q_s16(c) };
        // mask is -1 where v > 0, so v + mask is exactly `v - 1` there and `v` elsewhere;
        // v > 0 means the decrement cannot underflow.
        let gt = vreinterpretq_s16_u16(vcgtq_s16(v, vzero));
        // SAFETY: caller guarantees eight writable elements.
        unsafe { vst1q_s16(c, vaddq_s16(v, gt)) };
    }

    /// # Safety
    /// Caller guarantees NEON (guaranteed by `cfg` at every call site).
    #[inline]
    pub unsafe fn tick_down(counters: &mut [i16]) {
        let n = counters.len();
        let c0 = counters.as_mut_ptr();
        let vzero = vdupq_n_s16(0);
        let wide = n & !31;
        let mut i = 0;
        while i < wide {
            // SAFETY: `i + 32 <= wide <= n`.
            unsafe {
                octet(c0.add(i), vzero);
                octet(c0.add(i + 8), vzero);
                octet(c0.add(i + 16), vzero);
                octet(c0.add(i + 24), vzero);
            }
            i += 32;
        }
        let body = n & !7;
        while i < body {
            // SAFETY: `i + 8 <= body <= n`.
            unsafe { octet(c0.add(i), vzero) };
            i += 8;
        }
        super::tick_down_scalar(&mut counters[body..]);
    }
}

#[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
mod sse2 {
    use core::arch::x86_64::*;

    /// # Safety
    /// Caller guarantees SSE2 (baseline for x86-64, and asserted by `cfg` at every call
    /// site) and `pos.len() == vel.len()`.
    #[inline]
    pub unsafe fn integrate_wrap(pos: &mut [i32], vel: &[i32], span: i32) {
        let n = pos.len();
        let body = n & !3;
        let vspan = _mm_set1_epi32(span);
        let vzero = _mm_setzero_si128();
        // SSE2 has no unsigned/`>=` compare for i32, so `x >= span` is written
        // `x > span - 1`. span > 0 keeps `span - 1` from underflowing.
        let vspan_m1 = _mm_set1_epi32(span - 1);
        let mut i = 0;
        while i < body {
            // SAFETY: `i + 4 <= body <= n`; loadu has no alignment requirement.
            let p = unsafe { _mm_loadu_si128(pos.as_ptr().add(i) as *const __m128i) };
            let v = unsafe { _mm_loadu_si128(vel.as_ptr().add(i) as *const __m128i) };
            let x = _mm_add_epi32(p, v); // wrapping, lane-wise
            let lt = _mm_cmplt_epi32(x, vzero);
            let ge = _mm_cmpgt_epi32(x, vspan_m1);
            let x = _mm_add_epi32(x, _mm_and_si128(vspan, lt));
            let x = _mm_sub_epi32(x, _mm_and_si128(vspan, ge));
            // SAFETY: same bounds as the load.
            unsafe { _mm_storeu_si128(pos.as_mut_ptr().add(i) as *mut __m128i, x) };
            i += 4;
        }
        super::integrate_wrap_scalar(&mut pos[body..], &vel[body..], span);
    }

    /// # Safety
    /// Caller guarantees SSE2 (baseline for x86-64, asserted by `cfg` at every call site).
    #[inline]
    pub unsafe fn tick_down(counters: &mut [i16]) {
        let n = counters.len();
        let body = n & !7;
        let vzero = _mm_setzero_si128();
        let mut i = 0;
        while i < body {
            // SAFETY: `i + 8 <= body <= n`.
            let c = unsafe { _mm_loadu_si128(counters.as_ptr().add(i) as *const __m128i) };
            let gt = _mm_cmpgt_epi16(c, vzero); // -1 where c > 0
            let out = _mm_add_epi16(c, gt);
            // SAFETY: same bounds as the load.
            unsafe { _mm_storeu_si128(counters.as_mut_ptr().add(i) as *mut __m128i, out) };
            i += 8;
        }
        super::tick_down_scalar(&mut counters[body..]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random inputs; no dependency, and the same stream on every
    /// platform so a failure on one target is reproducible on another.
    struct Lcg(u64);
    impl Lcg {
        fn next_u32(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (self.0 >> 32) as u32
        }
    }

    #[test]
    fn integrate_wrap_paths_agree_bit_for_bit() {
        let span = crate::world::MAP_SPAN;
        let mut rng = Lcg(0xF00D_BEEF);
        // Every length from 0..40 covers all tail shapes for 4- and 8-wide bodies.
        for len in 0..40usize {
            let pos: Vec<i32> = (0..len).map(|_| (rng.next_u32() % span as u32) as i32).collect();
            let vel: Vec<i32> = (0..len).map(|_| (rng.next_u32() as i8) as i32).collect();
            let mut a = pos.clone();
            let mut b = pos.clone();
            let mut c = pos.clone();
            let mut d = pos.clone();
            hand::integrate_wrap(&mut a, &vel, span);
            integrate_wrap_scalar(&mut b, &vel, span);
            integrate_wrap_portable(&mut c, &vel, span);
            integrate_wrap(&mut d, &vel, span);
            assert_eq!(a, b, "hand vs scalar, len {len}");
            assert_eq!(a, c, "hand vs portable, len {len}");
            assert_eq!(a, d, "hand vs shipped default, len {len}");
        }
    }

    #[test]
    fn integrate_wrap_agrees_on_adversarial_inputs() {
        // Positions and velocities well outside the range the world produces, including
        // values that make the add wrap, so the paths are compared where they are most
        // likely to differ rather than only where the sim happens to live.
        let span = 49152i32;
        let pos: Vec<i32> = vec![
            0,
            -1,
            1,
            span - 1,
            span,
            span + 1,
            -span,
            i32::MIN,
            i32::MAX,
            i32::MIN + 1,
            i32::MAX - 1,
            -span / 2,
            span / 2,
            123456789,
            -123456789,
            0,
            7,
        ];
        let vel: Vec<i32> = vec![
            0,
            1,
            -1,
            1,
            -1,
            span,
            -span,
            -1,
            1,
            i32::MIN,
            i32::MAX,
            i32::MIN,
            i32::MAX,
            -1,
            1,
            i32::MAX,
            i32::MIN,
        ];
        for s in [span, 1, 2, i32::MAX] {
            let mut a = pos.clone();
            let mut b = pos.clone();
            let mut c = pos.clone();
            let mut d = pos.clone();
            hand::integrate_wrap(&mut a, &vel, s);
            integrate_wrap_scalar(&mut b, &vel, s);
            integrate_wrap_portable(&mut c, &vel, s);
            integrate_wrap(&mut d, &vel, s);
            assert_eq!(a, b, "hand vs scalar at span {s}");
            assert_eq!(a, c, "hand vs portable at span {s}");
            assert_eq!(a, d, "hand vs shipped default at span {s}");
        }
    }

    #[test]
    fn tick_down_paths_agree_bit_for_bit() {
        let mut rng = Lcg(0x1234_5678);
        for len in 0..40usize {
            let c: Vec<i16> = (0..len).map(|_| (rng.next_u32() % 9) as i16 - 3).collect();
            let mut a = c.clone();
            let mut b = c.clone();
            let mut d = c.clone();
            let mut e = c.clone();
            hand::tick_down(&mut a);
            tick_down_scalar(&mut b);
            tick_down_portable(&mut d);
            tick_down(&mut e);
            assert_eq!(a, b, "hand vs scalar, len {len}");
            assert_eq!(a, d, "hand vs portable, len {len}");
            assert_eq!(a, e, "hand vs shipped default, len {len}");
        }
    }

    #[test]
    fn tick_down_agrees_on_extremes() {
        let c: Vec<i16> = vec![i16::MIN, i16::MIN + 1, -2, -1, 0, 1, 2, i16::MAX - 1, i16::MAX, 0];
        let mut a = c.clone();
        let mut b = c.clone();
        let mut d = c.clone();
        hand::tick_down(&mut a);
        tick_down_scalar(&mut b);
        tick_down_portable(&mut d);
        assert_eq!(a, b);
        assert_eq!(a, d);
        // And the semantics themselves, spelled out: nothing at or below zero moves.
        assert_eq!(a[0], i16::MIN);
        assert_eq!(a[4], 0);
        assert_eq!(a[5], 0);
        assert_eq!(a[8], i16::MAX - 1);
    }

    #[test]
    fn integrate_wrap_keeps_in_range_inputs_in_range() {
        let span = 1000i32;
        let pos: Vec<i32> = (0..span).collect();
        let vel: Vec<i32> = (0..span).map(|i| (i % 257) - 128).collect();
        let mut a = pos.clone();
        integrate_wrap(&mut a, &vel, span);
        assert!(a.iter().all(|&x| (0..span).contains(&x)));
        let mut b = pos.clone();
        hand::integrate_wrap(&mut b, &vel, span);
        assert_eq!(a, b);
    }
}
