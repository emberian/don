//! The lockstep checksum primitive. **One implementation, one place.**
//!
//! `adler32` `0x00a46830` is the single arithmetic primitive under all fifteen channels of
//! `CheckSums::check_all` `0x00936560`: `CheckSum::walk_function` `0x00936ff0` is
//! `this->checksum = adler32(this->checksum, begin, end - begin)` and nothing else. Every
//! byte of sim-critical state in the game reaches the wire through this function.
//!
//! Before this module existed the crate carried **ten** transcriptions of it — nine free
//! `adler32` functions in `systems/{ammo, borders_fog, economy, groups_guys, items,
//! movement, production, tech_cities, victory_score}.rs` plus an `Adler32` accumulator in
//! `systems/map_terrain.rs` — and `don-replay`, `don-replay`'s three channel modules and
//! the oracle carried more. They happened to agree, but nothing made them agree; the
//! primitive that defines whether two machines are running the same game must be singular
//! by construction, not by luck. Every one of those sites now calls this function.
//!
//! # Fidelity
//!
//! **Tier B.** `docs/derivation/checksum.md` §2: 500,000 calls into the shipped machine
//! code at `0x00a46830`, 0 mismatches, with lengths straddling every structural boundary
//! (`0, 1, 2, 15, 16, 17, 31, 5551, 5552, 5553, 11104, 11105`, then uniform in
//! `[0, 24576]`) and a uniform random 32-bit incoming `adler`. The `adler32` case in
//! `crates/oracle/src/registry.rs` re-runs it on every `tools/oracle-regress.sh`, and as of
//! this lane its `model` is **this function** rather than a copy transcribed into the
//! oracle — a differential test against a copy proves the copy.
//!
//! That is a claim about the *primitive*, not about our state. What bytes our simulation
//! offers it is what the replay scoreboard measures.

/// `0x00a46882: mov ecx, 0xfff1` — largest prime below 65536.
pub const ADLER_BASE: u32 = 65_521;

/// `0x00a46854: mov edx, 0x15b0` — zlib's `NMAX`, the largest `n` for which
/// `255·n·(n+1)/2 + (n+1)·(BASE−1)` still fits in 32 bits.
///
/// The chunk boundary is not cosmetic: it is *where the modulus is applied*. A version
/// that reduces on a different schedule is a different function on any input long enough
/// to overflow, so the boundary is part of the derivation and is kept.
pub const ADLER_NMAX: usize = 5_552;

/// `adler32(unsigned long adler, const unsigned char* buf, unsigned long len)`
/// `0x00a46830` — `__fastcall(ECX = adler, EDX = buf) + one stack dword len`, caller
/// cleans. PDB: `?adler32@@YAKKPBEK@Z`.
///
/// Structure, transcribed from `re/decomp-all/00a46830.c` and the instruction stream:
///
/// ```text
/// s1 = adler & 0xffff ; s2 = adler >> 16
/// buf == NULL -> return 1                     ; see `adler32_or_null`
/// while len:
///     k = min(len, 0x15b0); len -= k
///     16-at-a-time block for k >> 4 iterations, then the k & 15 remainder
///     s1 %= 0xfff1 ; s2 %= 0xfff1
/// return (s2 << 16) | s1
/// ```
///
/// The 16-way unroll is arithmetically invisible — it is kept because it is the *other*
/// structural boundary the oracle case's boundary lengths probe, and a transcription that
/// silently drops it stops being a transcription of this routine.
///
/// A Rust `&[u8]` is never null, so an empty slice takes the ordinary path and returns
/// `adler` unchanged. That is a different case from the null pointer, and the distinction
/// is load-bearing: it is why an empty channel reads exactly `1` (the walker is never
/// called at all) rather than `1` by accident.
pub fn adler32(adler: u32, buf: &[u8]) -> u32 {
    let mut s1 = adler & 0xFFFF;
    let mut s2 = adler >> 16;
    let mut i = 0usize;
    while i < buf.len() {
        let k = ADLER_NMAX.min(buf.len() - i);
        let chunk = &buf[i..i + k];
        let (blocks, tail) = chunk.split_at(k - (k & 15));
        for b16 in blocks.chunks_exact(16) {
            for &b in b16 {
                s1 = s1.wrapping_add(u32::from(b));
                s2 = s2.wrapping_add(s1);
            }
        }
        for &b in tail {
            s1 = s1.wrapping_add(u32::from(b));
            s2 = s2.wrapping_add(s1);
        }
        s1 %= ADLER_BASE;
        s2 %= ADLER_BASE;
        i += k;
    }
    (s2 << 16) | s1
}

/// The same routine including its **null-pointer arm**: `0x00a4683d test edx, edx` →
/// `mov eax, 1; ret`. A null buffer returns `1` regardless of the incoming `adler`, which
/// is not the same as an empty buffer returning `adler`.
///
/// Rust cannot express a null slice, so callers that model a possibly-unallocated engine
/// array — `LeaderData`'s optional blocks, an unallocated `World::wdata` — pass `None`.
#[inline]
pub fn adler32_or_null(adler: u32, buf: Option<&[u8]>) -> u32 {
    match buf {
        None => 1,
        Some(b) => adler32(adler, b),
    }
}

/// The engine's `DataWalk` visitor, as `don-sim` needs it.
///
/// `DataWalk` is a two-method pure virtual: slot 0 `walk_function(void* begin, void* end)`
/// and slot 1 `walk_test(const String& tag)`. `CheckSum`, `SaveGame` and `LoadGame` are its
/// only three concrete implementations (RTTI-derived, `docs/derivation/checksum.md` §3),
/// which is why one traversal is simultaneously the checksum, the save format and the
/// `.rcx` header format.
///
/// `CheckSum::walk_test` `0x0041bfe0` is a bare `ret 4`, so the tag contributes nothing to
/// a checksum and is defaulted to a no-op here; a save/load sink overrides it.
pub trait DataWalk {
    /// Slot 0 — hand the visitor the bytes of `[begin, end)`.
    fn walk(&mut self, bytes: &[u8]);

    /// Slot 1 — `walk_test`. One `String::module_id` byte for `SaveGame`/`LoadGame`,
    /// nothing for `CheckSum`.
    fn walk_tag(&mut self, _tag: u8) {}

    /// `DataWalk+0x08` — non-zero for `CheckSum`. Several `walk_data` overrides branch on
    /// it to skip fields that are saved but not checksummed; `World::walk_data`'s section 9
    /// is one (it reallocates `CollBlock`s on the load path and only *reads* them here).
    fn is_checksum(&self) -> bool {
        true
    }

    /// `DataWalk+0x0c` — the section mask gating optional sub-walks. `check_all` passes
    /// `-1`.
    fn mask(&self) -> u32 {
        u32::MAX
    }
}

/// `CheckSum : DataWalk`, vftable `0x00b3f920` — the accumulator `check_all` builds on its
/// own stack, one per channel.
///
/// Layout mirrors the engine object: `+0x10` running adler, **initialised to 1 before every
/// channel** (`mov dword ptr [ebp-0x18], 1`, sixteen times in `check_all`), and `+0x14` a
/// running byte count. Keeping the byte count is what lets a mismatched channel be reported
/// as "diverged after N bytes" instead of one opaque 32-bit difference.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Adler32 {
    checksum: u32,
    /// `CheckSum+0x14` — bytes walked so far.
    pub bytes: u64,
}

impl Default for Adler32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Adler32 {
    /// A fresh channel accumulator, seeded to `1`.
    #[inline]
    pub const fn new() -> Self {
        Adler32 {
            checksum: 1,
            bytes: 0,
        }
    }

    /// Resume from an existing running value — used when a channel is walked in pieces.
    #[inline]
    pub const fn with_seed(seed: u32) -> Self {
        Adler32 {
            checksum: seed,
            bytes: 0,
        }
    }

    /// `CheckSum::walk_function` `0x00936ff0`.
    #[inline]
    pub fn update(&mut self, bytes: &[u8]) {
        self.checksum = adler32(self.checksum, bytes);
        self.bytes += bytes.len() as u64;
    }

    /// The running value at `CheckSum+0x10`.
    #[inline]
    pub const fn finish(&self) -> u32 {
        self.checksum
    }
}

impl DataWalk for Adler32 {
    #[inline]
    fn walk(&mut self, bytes: &[u8]) {
        self.update(bytes);
    }
}

/// A `DataWalk` that keeps the bytes instead of hashing them.
///
/// Not an engine object — it exists because "the checksums differ" is not a debuggable
/// statement. Walking the same traversal into a `ByteSink` on both sides and diffing gives
/// the first differing offset, which the byte count in [`Adler32`] can then be matched
/// against.
#[derive(Clone, Debug, Default)]
pub struct ByteSink(pub Vec<u8>);

impl ByteSink {
    #[inline]
    pub fn new() -> Self {
        ByteSink(Vec::new())
    }

    #[inline]
    pub fn checksum(&self) -> u32 {
        adler32(1, &self.0)
    }

    /// Byte offset of the first difference against another walk of the same traversal, or
    /// `None` when the two streams are identical.
    pub fn first_difference(&self, other: &ByteSink) -> Option<usize> {
        let n = self.0.len().min(other.0.len());
        (0..n)
            .find(|&i| self.0[i] != other.0[i])
            .or(if self.0.len() == other.0.len() {
                None
            } else {
                Some(n)
            })
    }
}

impl DataWalk for ByteSink {
    #[inline]
    fn walk(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The straight one-byte-at-a-time definition. Independent of the shipped code path,
    /// so agreement is evidence the chunking and unrolling are transparent.
    fn adler32_by_definition(adler: u32, buf: &[u8]) -> u32 {
        let mut s1 = adler & 0xFFFF;
        let mut s2 = adler >> 16;
        for &b in buf {
            s1 = (s1 + u32::from(b)) % ADLER_BASE;
            s2 = (s2 + s1) % ADLER_BASE;
        }
        (s2 << 16) | s1
    }

    #[test]
    fn known_vectors() {
        assert_eq!(adler32(1, b""), 1);
        assert_eq!(adler32(1, b"a"), 0x0062_0062);
        assert_eq!(adler32(1, b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn null_is_one_but_empty_is_the_incoming_value() {
        assert_eq!(adler32_or_null(0xDEAD_BEEF, None), 1);
        assert_eq!(adler32_or_null(0xDEAD_BEEF, Some(b"")), 0xDEAD_BEEF);
        assert_eq!(adler32(0x1234_5678, b""), 0x1234_5678);
    }

    /// The 16-way unroll and the `NMAX` chunk boundary must both be arithmetically
    /// invisible. Lengths chosen to straddle each: `15/16/17` for the unroll,
    /// `5551/5552/5553` and `11104/11105` for `NMAX`.
    #[test]
    fn chunking_and_unrolling_are_transparent() {
        let mut state = 0x1234_5678u32;
        let data: Vec<u8> = (0..12_000u32)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 24) as u8
            })
            .collect();
        for len in [
            0usize, 1, 2, 15, 16, 17, 31, 5551, 5552, 5553, 11104, 11105, 12000,
        ] {
            assert_eq!(
                adler32(1, &data[..len]),
                adler32_by_definition(1, &data[..len]),
                "len {len}"
            );
            assert_eq!(
                adler32(0xFFFF_FFFF, &data[..len]),
                adler32_by_definition(0xFFFF_FFFF, &data[..len]),
                "len {len}, seed ffffffff"
            );
        }
    }

    /// A running seed, not a one-shot: `check_all` feeds one accumulator many ranges.
    #[test]
    fn accumulator_equals_one_pass_over_the_concatenation() {
        let mut a = Adler32::new();
        a.update(b"Wiki");
        a.update(b"pedia");
        assert_eq!(a.finish(), adler32(1, b"Wikipedia"));
        assert_eq!(a.bytes, 9);
    }

    #[test]
    fn byte_sink_locates_the_first_difference() {
        let mut a = ByteSink::new();
        let mut b = ByteSink::new();
        a.walk(b"abcdef");
        b.walk(b"abcXef");
        assert_eq!(a.first_difference(&b), Some(3));
        assert_eq!(a.first_difference(&a.clone()), None);
        assert_eq!(a.checksum(), adler32(1, b"abcdef"));

        let mut short = ByteSink::new();
        short.walk(b"abc");
        assert_eq!(a.first_difference(&short), Some(3));
    }

    #[test]
    fn tag_is_a_no_op_for_the_checksum_sink() {
        let mut a = Adler32::new();
        a.walk_tag(7);
        assert_eq!(a.finish(), 1);
        assert!(a.is_checksum());
        assert_eq!(a.mask(), u32::MAX);
    }
}
