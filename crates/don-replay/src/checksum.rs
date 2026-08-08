//! The lockstep checksum, in our shape.
//!
//! `CheckSum`, `SaveGame` and `LoadGame` are the only three concrete
//! implementations of the engine's two-method pure-virtual `DataWalk` visitor
//! (`docs/derivation/checksum.md` §3, RTTI-derived), so the traversal in this
//! module is simultaneously the checksum, the save format and the `.rcx` header
//! format. That is why it is worth mirroring exactly rather than approximating.
//!
//! Nothing here is a fidelity claim about *our simulation state*. It is a claim
//! about the **visitor**: given the same bytes in the same order, this produces
//! the same 32-bit values the engine produces. What bytes our state offers is
//! the open question the harness measures.

/// Largest prime below 65536. `0xa4691f: mov eax,0x80078071 … imul eax,edx,0xffff000f`
/// is the reciprocal-multiply form of `% 65521`.
pub const ADLER_BASE: u32 = 65521;

/// `0xa46854: mov edx, 0x15b0`. zlib's `NMAX`: the largest `n` for which
/// `255n(n+1)/2 + (n+1)(BASE-1)` still fits in 32 bits.
pub const NMAX: usize = 5552;

/// `adler32(adler, buf, len)` — the retail primitive at VA `0x00a46830`,
/// `__fastcall(ECX = adler, EDX = buf, [esp+4] = len)`.
///
/// Tier B against retail: 500,000 calls into the shipped machine code with
/// 0 mismatches, lengths straddling every structural boundary
/// (`0,1,2,15,16,17,31,5551,5552,5553,11104,11105` then uniform in `[0,24576]`).
/// See `docs/derivation/checksum.md` §2 and the `adler` case in the oracle
/// regression suite.
///
/// The `NMAX` chunking is not cosmetic: it is where the modulus is applied, and
/// a version that reduces on a different schedule is a *different function* on
/// inputs long enough to overflow. Keep the chunk boundary.
pub fn adler32(adler: u32, buf: &[u8]) -> u32 {
    // `0xa4683d test edi,edi` -> `lea eax,[edx+1]`: a null buffer returns 1
    // regardless of the incoming adler. An empty slice is not a null pointer,
    // so it takes the ordinary path and returns `adler` unchanged; that
    // distinction is why the engine's empty channels read exactly 1 (the
    // walker is never called at all) rather than "1 by accident".
    let mut s1 = adler & 0xFFFF;
    let mut s2 = (adler >> 16) & 0xFFFF;
    let mut i = 0usize;
    while i < buf.len() {
        let n = NMAX.min(buf.len() - i);
        for &b in &buf[i..i + n] {
            s1 += b as u32;
            s2 += s1;
        }
        s1 %= ADLER_BASE;
        s2 %= ADLER_BASE;
        i += n;
    }
    (s2 << 16) | s1
}

/// The engine's `DataWalk` interface: exactly two virtuals.
///
/// | slot | signature | `CheckSum`'s implementation |
/// |---|---|---|
/// | 0 | `walk_function(void* begin, void* end)` | `0x00936ff0` — adler-32 over `[begin,end)` |
/// | 1 | `walk_test(const String& tag)` | `0x0041bfe0` — a bare `ret 4`, a no-op |
///
/// `walk_test` emits one byte (`String::module_id`) for `SaveGame`/`LoadGame`
/// and **nothing** for `CheckSum`. Modelling it as a separate method rather
/// than folding it into `walk` is what keeps one traversal usable for both.
pub trait DataWalk {
    fn walk(&mut self, bytes: &[u8]);
    fn walk_tag(&mut self, tag: u8);

    /// `DataWalk+0x0c`, the section mask that gates optional sub-walks.
    /// `check_units` passes `-1`.
    fn mask(&self) -> u32 {
        u32::MAX
    }

    /// `DataWalk+0x08`: non-zero for `CheckSum`. Several `walk_data` overrides
    /// branch on it to skip fields that are saved but not checksummed.
    fn is_checksum(&self) -> bool {
        true
    }
}

/// `CheckSum : DataWalk`, vftable `0x00b3f920`.
///
/// Layout mirrors the object the engine constructs on `check_all`'s stack:
/// `+0x10` running adler (initialised to **1**), `+0x14` bytes-walked counter,
/// `+0x0c` section mask.
#[derive(Debug, Clone, Copy)]
pub struct CheckSum {
    pub checksum: u32,
    pub bytes: u64,
    pub mask: u32,
}

impl Default for CheckSum {
    fn default() -> Self {
        CheckSum { checksum: 1, bytes: 0, mask: u32::MAX }
    }
}

impl CheckSum {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_mask(mask: u32) -> Self {
        CheckSum { mask, ..Self::default() }
    }
}

impl DataWalk for CheckSum {
    fn walk(&mut self, bytes: &[u8]) {
        // `add [edi+0x14], eax` then the adler tail call, in that order.
        self.bytes += bytes.len() as u64;
        self.checksum = adler32(self.checksum, bytes);
    }
    fn walk_tag(&mut self, _tag: u8) {
        // `0x0041bfe0` is `ret 4`.
    }
    fn mask(&self) -> u32 {
        self.mask
    }
}

// ---------------------------------------------------------------------------
// Channels
// ---------------------------------------------------------------------------

/// The sixteen fields of `CheckSumsCommand` (opcode `0x39`, 65 bytes), in wire
/// order — which is also `check_all`'s call order.
///
/// `All` is not a hash: `check_all` accumulates `add edi, <channel>` after each
/// of the fifteen, so it is a **wrapping 32-bit sum**, not adler-32 over the
/// concatenation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(usize)]
pub enum Channel {
    Units = 0,
    Builds,
    Walls,
    Ammo,
    Deaths,
    Groups,
    Guys,
    Leaders,
    Cities,
    Items,
    Goods,
    World,
    Rules,
    ScenarioData,
    ScriptRunTime,
    All,
}

pub const NUM_CHANNELS: usize = 16;
/// The fifteen walked channels; `All` is the sixteenth and is derived.
pub const NUM_WALKED: usize = 15;

pub const CHANNELS: [Channel; NUM_CHANNELS] = [
    Channel::Units,
    Channel::Builds,
    Channel::Walls,
    Channel::Ammo,
    Channel::Deaths,
    Channel::Groups,
    Channel::Guys,
    Channel::Leaders,
    Channel::Cities,
    Channel::Items,
    Channel::Goods,
    Channel::World,
    Channel::Rules,
    Channel::ScenarioData,
    Channel::ScriptRunTime,
    Channel::All,
];

pub const CHANNEL_NAMES: [&str; NUM_CHANNELS] = [
    "units",
    "builds",
    "walls",
    "ammo",
    "deaths",
    "groups",
    "guys",
    "leaders",
    "cities",
    "items",
    "goods",
    "world",
    "rules",
    "scenario_data",
    "script_run_time",
    "all",
];

/// VA of the `check_all` walker for each channel (`docs/derivation/checksum.md` §4).
/// `leaders` is the send-path helper `0x009375a0`; `check_all` itself inlines an
/// 8-iteration `Leader::walk_data` loop over the same records.
pub const CHANNEL_WALKER_VA: [u32; NUM_WALKED] = [
    0x0093_71d0, // units
    0x0093_7290, // builds
    0x0093_7360, // walls
    0x0093_74e0, // ammo
    0x0093_6bb0, // deaths
    0x0093_7530, // groups
    0x0093_7430, // guys
    0x0093_75a0, // leaders
    0x0093_7600, // cities
    0x0093_7790, // items
    0x0093_7710, // goods
    0x006b_5cf0, // world  (conditional on [[0x00c06188]+0x134] != 0)
    0x0058_9550, // rules
    0x0099_7ad0, // scenario_data
    0x009c_41a0, // script_run_time
];

impl Channel {
    pub fn name(self) -> &'static str {
        CHANNEL_NAMES[self as usize]
    }
    pub fn from_name(s: &str) -> Option<Channel> {
        CHANNEL_NAMES.iter().position(|n| *n == s).map(|i| CHANNELS[i])
    }
    /// True for the channels whose value is a function of mutable simulation
    /// state. `rules` is loaded-once static data; the corpus shows it constant
    /// at `0x12ba3104` across every turn of every multiplayer recording and
    /// across two engine builds.
    pub fn is_mutable(self) -> bool {
        !matches!(self, Channel::Rules | Channel::All)
    }
}

/// One turn's sixteen values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Channels(pub [u32; NUM_CHANNELS]);

impl Channels {
    /// A checksum tuple over completely empty state: every walker is called but
    /// walks nothing, so every channel keeps adler-32's initial value of 1, and
    /// `all` is `15 * 1`.
    pub fn empty_state() -> Channels {
        let mut v = [1u32; NUM_CHANNELS];
        v[Channel::All as usize] = NUM_WALKED as u32;
        Channels(v)
    }

    pub fn get(&self, c: Channel) -> u32 {
        self.0[c as usize]
    }
    pub fn set(&mut self, c: Channel, v: u32) {
        self.0[c as usize] = v;
    }

    /// `total` as `check_all` computes it: a wrapping sum of the fifteen.
    pub fn computed_total(&self) -> u32 {
        self.0[..NUM_WALKED].iter().fold(0u32, |a, &b| a.wrapping_add(b))
    }

    /// The self-check that makes a `CheckSumsCommand` findable without framing:
    /// the sixteenth word must equal the wrapping sum of the first fifteen.
    /// False-positive rate ≈ 2⁻³².
    pub fn total_is_consistent(&self) -> bool {
        self.computed_total() == self.0[Channel::All as usize]
    }

    /// Every walked channel must be adler-32-shaped: both halves below 65521.
    pub fn adler_shaped(&self) -> bool {
        self.0[..NUM_WALKED]
            .iter()
            .all(|v| (v & 0xFFFF) < ADLER_BASE && (v >> 16) < ADLER_BASE)
    }

    pub fn from_recorded(v: [u32; NUM_CHANNELS]) -> Channels {
        Channels(v)
    }
}

/// The `rules` channel value produced by the shipped rule set, constant across
/// every turn of every multiplayer recording in the corpus and across both the
/// 00.2017.11.29 and 00.2024.06.20 engine builds
/// (`docs/derivation/replay-checksum.md` §5) [measured].
///
/// A free, exact, single-32-bit-word target: a Rust `Constants` walk that
/// mirrors `0x00589550` must produce this or the rule set is wrong.
pub const SHIPPED_RULES_CHANNEL: u32 = 0x12ba_3104;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adler_matches_the_published_vectors() {
        // zlib's own documented behaviour; the retail routine is zlib's.
        assert_eq!(adler32(1, b""), 1);
        assert_eq!(adler32(1, b"a"), 0x0062_0062);
        assert_eq!(adler32(1, b"abc"), 0x024d_0127);
        assert_eq!(adler32(1, b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn adler_chunking_is_stable_across_the_nmax_boundary() {
        // Feeding the same bytes in different call shapes must agree, which is
        // the property that lets a walk be split at any struct boundary.
        let buf: Vec<u8> = (0..3 * NMAX as u32 + 17).map(|i| (i * 31) as u8).collect();
        let whole = adler32(1, &buf);
        for split in [1usize, NMAX - 1, NMAX, NMAX + 1, 2 * NMAX] {
            let a = adler32(1, &buf[..split]);
            assert_eq!(adler32(a, &buf[split..]), whole, "split at {split}");
        }
    }

    #[test]
    fn checksum_visitor_starts_at_one_and_counts_bytes() {
        let mut c = CheckSum::new();
        assert_eq!(c.checksum, 1);
        c.walk(&[1, 2, 3]);
        c.walk_tag(0x16);
        assert_eq!(c.bytes, 3, "walk_tag emits nothing for CheckSum");
        assert_eq!(c.checksum, adler32(1, &[1, 2, 3]));
    }

    #[test]
    fn total_is_a_wrapping_sum_not_a_hash() {
        let mut ch = Channels([0; NUM_CHANNELS]);
        for (i, c) in CHANNELS[..NUM_WALKED].iter().enumerate() {
            ch.set(*c, 0xF000_0000u32.wrapping_add(i as u32));
        }
        ch.set(Channel::All, ch.computed_total());
        assert!(ch.total_is_consistent());
        // and it really wraps
        assert_eq!(ch.computed_total(), 0xF000_0000u32.wrapping_mul(15).wrapping_add(105));
    }

    #[test]
    fn empty_state_tuple_is_fifteen_ones() {
        let e = Channels::empty_state();
        assert!(e.total_is_consistent());
        assert!(e.adler_shaped());
        assert_eq!(e.get(Channel::All), 15);
    }
}
