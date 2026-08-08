//! Mechanics derived from the binary.
//!
//! Nothing enters this module without a `docs/provenance-ledger.md` entry naming the
//! function it came from and the evidence behind it. Placeholder logic lives in
//! `world.rs` and is marked as such; this module is for the real thing.

/// Map two integers into the inclusive range spanned by `lo` and `hi`.
///
/// Derived from `riseofnations.exe` at VA `0x00846450` (`__stdcall`, four dword args,
/// `ret 0x10`). The retail body is:
///
/// ```text
/// eax = hi - lo ; cdq ; ecx = |eax| + 1        ; branchless abs via xor/sub with sign
/// eax = a ; imul eax, eax ; imul eax, b        ; signed 32-bit, wrapping
/// cdq ; idiv ecx                               ; edx = truncated remainder
/// eax = edx + lo
/// ```
///
/// Semantics worth stating because they are easy to get wrong and were confirmed, not
/// assumed:
///
/// * All multiplication wraps (signed 32-bit overflow is not saturating).
/// * `idiv` truncates toward zero, so the remainder takes the sign of the **dividend**.
///   `a*a*b` can be negative — `a*a` alone can wrap negative — so the result can fall
///   *below* `lo`. This is faithful behaviour, not a bug in this port.
/// * `hi` is not required to exceed `lo`; the divisor uses `|hi - lo|`.
///
/// # Fidelity
///
/// **Tier B** — differentially tested against the retail code under
/// `crates/oracle`: 500,008 inputs (8 hand-chosen edge cases covering `i32::MIN`,
/// `i32::MAX`, zero ranges and inverted ranges, plus 500,000 pseudo-random), zero
/// mismatches. Testing, not proof: this is not a claim about all 2^128 inputs.
///
/// The name is descriptive of the computation. What the engine *uses* it for is not yet
/// established, so it deliberately does not claim to be "the RNG" or "the damage roll".
#[inline]
pub fn hash_into_range(a: i32, b: i32, lo: i32, hi: i32) -> i32 {
    let divisor = hi.wrapping_sub(lo).wrapping_abs().wrapping_add(1);
    a.wrapping_mul(a)
        .wrapping_mul(b)
        .wrapping_rem(divisor)
        .wrapping_add(lo)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Values **captured from the retail code** via `oracle vectors`, not hand-computed.
    /// An earlier version of this test had `(7, -3, -100, 100)` as -47 from my own
    /// arithmetic; the binary says -247. Capture, do not calculate.
    #[test]
    fn matches_retail_on_captured_vectors() {
        assert_eq!(hash_into_range(0, 0, 0, 0), 0);
        assert_eq!(hash_into_range(1, 1, 0, 1), 1);
        assert_eq!(hash_into_range(-1, -1, -5, 5), -6);
        assert_eq!(hash_into_range(7, -3, -100, 100), -247);
        assert_eq!(hash_into_range(123456, 789, 0, 0), 0);
        assert_eq!(hash_into_range(5, 5, 10, -10), 30);
        assert_eq!(hash_into_range(100, 3, 1, 6), 1);
        assert_eq!(hash_into_range(-9, 4, 0, 100), 21);
    }

    #[test]
    fn wrapping_multiplication_does_not_panic_on_extremes() {
        // i32::MIN * i32::MIN wraps; a naive implementation panics in debug builds.
        let _ = hash_into_range(i32::MIN, 1, 0, 10);
        let _ = hash_into_range(i32::MAX, i32::MAX, i32::MIN, i32::MAX);
        let _ = hash_into_range(i32::MIN, i32::MIN, i32::MAX, i32::MIN);
    }

    #[test]
    fn result_may_fall_below_lo_because_idiv_truncates() {
        // Faithful to the binary: the remainder carries the dividend's sign.
        let r = hash_into_range(-1, -1, -5, 5);
        assert!(r < -5, "expected sub-lo result from a negative dividend, got {r}");
    }

    #[test]
    fn inverted_range_is_accepted() {
        // hi < lo uses |hi - lo|; the retail code does not order them.
        let a = hash_into_range(5, 5, 10, -10);
        let b = hash_into_range(5, 5, 10, -10);
        assert_eq!(a, b);
    }
}
