//! Multiplayer payload obfuscation.
//!
//! `CommandPackage::process_all` (`?process_all@CommandPackage@@QAEXXZ`, VA
//! 0x0094c500) applies two transforms to a received package, both gated on the
//! multiplayer flag `PTR_DAT_00c061ec[0x820] & 4`:
//!
//! 1. **XOR.** Every `u16` of the payload is XORed with `(G >> 8) as u16`.
//!    The 18-byte header is untouched.
//! 2. **Padding.** After each command the reader skips `Random::get(0, 2)`
//!    bytes, i.e. 0 or 1.
//!
//! Both are driven by the same 32-bit game global
//! `G = *(u32*)(*(u8**)0x00C061EC + 0x10)`. The XOR key is `G >> 8`; the padding
//! `Random` is a **stack local seeded with `G` on entry to `process_all`**
//! (`0094c6b2: mov ecx,[eax+0x10]` … `0094c6b7: mov [ebp-0x18],eax`, then
//! `0094c6fe: lea ecx,[ebp-0x18]` immediately before `call 0xa39d70`).
//!
//! That last detail matters more than it looks. Because the generator is a
//! fresh stack local rather than the simulation `Random`, **the pad sequence
//! restarts at the same value for every package** and does not depend on
//! simulation history. A multiplayer command stream is therefore statically
//! decodable, which contradicts the earlier reading in
//! `docs/derivation/replay-stream.md` §5 that a reader must co-simulate the
//! engine's RNG. Measured evidence is in `tests/roundtrip.rs`.
//!
//! `Random::get(int, int)` (`?get@Random@@QAEHHH@Z`, VA 0x00a39d70), read off
//! the retail instructions at 0x00a39ea5:
//!
//! ```text
//! imul ecx, [eax], 0x19660d   ; s *= 1664525
//! add  ecx, 0x3c6ef35f        ; s += 1013904223
//! mov  [eax], ecx
//! movzx eax, cx               ; low 16 bits of the new state
//! imul eax, edi               ; * (hi - lo)
//! shr  eax, 0x10              ; >> 16
//! add  eax, ebx               ; + lo
//! ```

/// LCG multiplier used by every `Random` in the engine.
pub const LCG_A: u32 = 0x0019_660D;
/// LCG increment.
pub const LCG_C: u32 = 0x3C6E_F35F;

/// The engine's `Random`, which is exactly one `u32` (`sizeof(Random) == 4`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PadRandom {
    pub seed: u32,
}

impl PadRandom {
    pub fn new(seed: u32) -> Self {
        PadRandom { seed }
    }

    /// `Random::get(lo, hi)`. Faithful to the retail instruction sequence,
    /// including the `lo == hi` short-circuit and the `lo > hi` swap.
    pub fn in_range(&mut self, lo: i32, hi: i32) -> i32 {
        if lo == hi {
            return lo;
        }
        let (lo, hi) = if lo > hi { (hi, lo) } else { (lo, hi) };
        self.seed = self.seed.wrapping_mul(LCG_A).wrapping_add(LCG_C);
        let low16 = (self.seed & 0xFFFF) as u64;
        lo + ((low16 * (hi - lo) as u64) >> 16) as i32
    }

    /// The inter-command pad the engine draws: `in_range(0, 2)` ∈ {0, 1}.
    pub fn next_pad(&mut self) -> usize {
        self.in_range(0, 2) as usize
    }
}

/// Payload obfuscation state for one `CommandPackage`.
///
/// Construct with [`Obfuscation::none`] for a single-player stream and
/// [`Obfuscation::multiplayer`] for a networked one. The generator must be
/// reset for every package, which [`Obfuscation::multiplayer`] does by
/// construction.
#[derive(Debug, Clone, Copy)]
pub struct Obfuscation {
    rng: Option<PadRandom>,
}

impl Obfuscation {
    /// Single-player / recorded solo game: `process_all` takes the
    /// `iVar11 = 0` branch, so there is no padding and no XOR.
    pub fn none() -> Self {
        Obfuscation { rng: None }
    }

    /// Multiplayer, seeded from the game key `G`.
    pub fn multiplayer(g: u32) -> Self {
        Obfuscation {
            rng: Some(PadRandom::new(g)),
        }
    }

    /// Multiplayer with an explicitly chosen pad seed. Useful when only the
    /// low bits of `G` have been recovered — see [`Obfuscation::xor_key`].
    pub fn with_seed(seed: u32) -> Self {
        Obfuscation {
            rng: Some(PadRandom::new(seed)),
        }
    }

    pub fn next_pad(&mut self) -> usize {
        match &mut self.rng {
            Some(r) => r.next_pad(),
            None => 0,
        }
    }

    /// The XOR key the engine derives from `G`.
    pub fn xor_key(g: u32) -> u16 {
        (g >> 8) as u16
    }
}

/// XOR a payload in place with `key`, over whole `u16` words.
///
/// A trailing odd byte is left alone: `process_all` computes the word count as
/// `size / 2` and the tail loop also steps by `u16`, so an odd final byte is
/// never touched.
pub fn xor_payload(buf: &mut [u8], key: u16) {
    let k = key.to_le_bytes();
    let n = buf.len() & !1;
    for i in (0..n).step_by(2) {
        buf[i] ^= k[0];
        buf[i + 1] ^= k[1];
    }
}

/// Rank candidate XOR keys by ciphertext word frequency, most likely first.
///
/// Command payloads carry long runs of zero (unused coordinate and flag
/// fields), so `0 ^ key == key` is usually the most common ciphertext word.
/// "Usually" is not "always" — in games dominated by `process_camera` the modal
/// plaintext word can be a repeated coordinate instead. Returning a ranked list
/// lets the caller validate each candidate by whether the stream decodes, which
/// is the only real test.
pub fn rank_xor_keys<'a, I: IntoIterator<Item = &'a [u8]>>(payloads: I, n: usize) -> Vec<u16> {
    let mut counts = std::collections::HashMap::<u16, u32>::new();
    for p in payloads {
        let m = p.len() & !1;
        for i in (0..m).step_by(2) {
            *counts
                .entry(u16::from_le_bytes([p[i], p[i + 1]]))
                .or_insert(0) += 1;
        }
    }
    let mut v: Vec<(u16, u32)> = counts.into_iter().collect();
    v.sort_by_key(|&(w, c)| (std::cmp::Reverse(c), w));
    v.into_iter().take(n).map(|(w, _)| w).collect()
}

/// The single most likely XOR key. Convenience wrapper over [`rank_xor_keys`].
pub fn recover_xor_key<'a, I: IntoIterator<Item = &'a [u8]>>(payloads: I) -> u16 {
    rank_xor_keys(payloads, 1).first().copied().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_range_endpoints() {
        let mut r = PadRandom::new(12345);
        assert_eq!(r.in_range(7, 7), 7);
        for _ in 0..10_000 {
            let v = r.in_range(0, 2);
            assert!(v == 0 || v == 1);
        }
    }

    #[test]
    fn in_range_is_symmetric_under_swapped_bounds() {
        let mut a = PadRandom::new(99);
        let mut b = PadRandom::new(99);
        assert_eq!(a.in_range(3, 11), b.in_range(11, 3));
    }

    #[test]
    fn xor_is_an_involution_and_spares_an_odd_tail() {
        let orig = vec![1u8, 2, 3, 4, 5];
        let mut b = orig.clone();
        xor_payload(&mut b, 0x8EC6);
        assert_ne!(b[..4], orig[..4]);
        assert_eq!(b[4], orig[4], "odd trailing byte must be untouched");
        xor_payload(&mut b, 0x8EC6);
        assert_eq!(b, orig);
    }

    #[test]
    fn only_the_low_sixteen_bits_of_the_seed_can_change_a_pad() {
        // The LCG's low 16 bits are self-contained under multiply-add mod 2^32,
        // and in_range reads only those. This is why recovering the pad
        // sequence is a 256-way search once the XOR key is known.
        for lo in [0u32, 1, 0x1234, 0xFFFF] {
            let a: Vec<usize> = {
                let mut r = PadRandom::new(lo);
                (0..40).map(|_| r.next_pad()).collect()
            };
            let b: Vec<usize> = {
                let mut r = PadRandom::new(lo | 0xDEAD_0000);
                (0..40).map(|_| r.next_pad()).collect()
            };
            assert_eq!(a, b);
        }
    }
}
