//! Rust models for retail functions that have **no `don-sim` implementation yet**.
//!
//! Where a mechanic is implemented in `don-sim`, the registry points at *that* function,
//! not at a copy — see `registry.rs`. Testing a transcription that lives only in the
//! oracle proves the transcription is right and says nothing about the code we ship, and
//! before this module existed every case in `main.rs` did exactly that.
//!
//! Everything here is therefore a **gap marker** as much as a model: each item names the
//! `don-sim` symbol it is waiting for. When that symbol lands, the registry entry should
//! be repointed and the copy here deleted.

// ---------------------------------------------------------------------------------
// Anonymous field accessors. These are shape probes, not mechanics: they exist to prove
// the harness reproduces `__thiscall` field reads, so they will never have a don-sim
// counterpart.
// ---------------------------------------------------------------------------------

/// `0x00472400` — `movsx eax, word ptr [ecx+0xA]`.
///
/// Writes the input into the scratch object and returns what retail must produce.
pub fn accessor_movsx_0xa(obj: *mut u8, r: u64) -> u32 {
    let v = r as u16;
    unsafe { std::ptr::write_unaligned(obj.add(0x0A) as *mut u16, v) };
    (v as i16) as i32 as u32
}

/// `0x0048F770` — `movsx [ecx+0x12C]` minus `movsx [ecx+0x12A]`, both `i16`.
pub fn accessor_diff_12c_12a(obj: *mut u8, r: u64) -> u32 {
    let a = r as u16;
    let b = (r >> 16) as u16;
    unsafe {
        std::ptr::write_unaligned(obj.add(0x12C) as *mut u16, a);
        std::ptr::write_unaligned(obj.add(0x12A) as *mut u16, b);
    }
    ((a as i16) as i32).wrapping_sub((b as i16) as i32) as u32
}

// ---------------------------------------------------------------------------------
// `vector_dist` — `0x0046CFF0`, PDB `?vector_dist@@YAHHH@Z`, `__fastcall(ecx, edx)`.
//
// Ledger §2.2: derived, Tier B, **no implementation in this repo**. Its harness lived only
// at `hbox:~/lane-pathfinding` (ledger §7.4 names that as a reproducibility gap); this is
// that harness's model, brought in-tree so the result survives the box.
// ---------------------------------------------------------------------------------

/// `max + min²/(2·max)`, with an unsigned fallback `(min + 2·max) >> 1` above `0xEA60`.
///
/// The traps, all of them real and all exercised by the case: the `hi > lo` compare is
/// **signed** while the `0xEA60` guard is **unsigned**; the divide is unsigned `div`, not
/// `idiv`; the shift is `shr`, not `sar`; `abs` is the wrapping kind, so `i32::MIN` stays
/// `i32::MIN`.
pub fn vector_dist(a: i32, b: i32) -> i32 {
    // edi = abs(a), esi = abs(b) via cdq/xor/sub -> wrapping abs
    let hi = a.wrapping_abs();
    let lo = b.wrapping_abs();
    // cmp edi, esi ; jle  -- SIGNED compare
    if hi > lo {
        if hi == 0 {
            return 0;
        }
        // cmp esi, 0xEA60 ; jb  -- UNSIGNED compare
        if (lo as u32) < 0xEA60 {
            let num = (lo as u32).wrapping_mul(lo as u32);
            let den = (hi as u32).wrapping_mul(2);
            return ((num / den) as i32).wrapping_add(hi);
        }
        return (((lo as u32).wrapping_add((hi as u32).wrapping_mul(2))) >> 1) as i32;
    }
    if lo == 0 {
        return 0;
    }
    if (hi as u32) < 0xEA60 {
        let num = (hi as u32).wrapping_mul(hi as u32);
        let den = (lo as u32).wrapping_mul(2);
        return ((num / den) as i32).wrapping_add(lo);
    }
    (((hi as u32).wrapping_add((lo as u32).wrapping_mul(2))) >> 1) as i32
}

// ---------------------------------------------------------------------------------
// `adler32` — `0x00A46830`, PDB `?adler32@@YAKKPBEK@Z`, `__fastcall(ecx = running sum,
// edx = buffer) + one stack dword length`, **caller cleans** (`ret`, and the call site
// does `add esp, 4`).
//
// Ledger §2.3: derived, Tier B, **no implementation in this repo**. Its harness lived only
// at `hbox:~/don-oracle-checksum` (ledger §7.4). Written from the algorithm rather than
// from zlib, so agreement is evidence about the routine and not about a shared source.
// ---------------------------------------------------------------------------------

// GAP CLOSED 2026-08-08. `don_sim::checksum::adler32` now exists and is the only
// implementation in the workspace, so the registry points at it and the copy that used to
// live here is deleted, per this module's own rule.

// ---------------------------------------------------------------------------------
// The engine RNG. `docs/derivation/rng.md` proposes a ledger entry naming
// `don_sim::mechanics::{lcg_step, rand_real, rand_int}` — **those functions do not exist
// in don-sim** (checked 2026-08-08). Until they do, this is the only Rust copy, and the
// Tier-B claim covers this transcription rather than anything the sim will run.
// ---------------------------------------------------------------------------------

pub mod rng {
    /// `imul r, [this], 0x19660D ; add r, 0x3C6EF35F` — Numerical Recipes `ranqd1`.
    pub fn lcg_step(s: u32) -> u32 {
        s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223)
    }

    /// `Random::next_float()` at `0x00a39cf0`.
    ///
    /// The retail code widens to double, subtracts 1.0, narrows back. Both conversions
    /// and the subtraction are exact for a value in [1,2), so a single-precision
    /// subtraction is bit-identical — which is what the differential test checks.
    pub fn next_float(s: &mut u32) -> f32 {
        *s = lcg_step(*s);
        let bits = (*s & 0x007f_ffff) | 0x3f80_0000;
        f32::from_bits(bits) - 1.0f32
    }

    /// `Random::in_range(lo, hi)` at `0x00a39d70`.
    pub fn in_range(s: &mut u32, lo: i32, hi: i32) -> i32 {
        if lo == hi {
            return lo; // early-out at 0x00a39e85: the state is NOT advanced
        }
        let (lo, hi) = if lo > hi { (hi, lo) } else { (lo, hi) }; // xor-swap at 0x00a39e9f
        let range = (hi as u32).wrapping_sub(lo as u32); // sub edi, ebx
        *s = lcg_step(*s);
        let low16 = *s & 0xffff; // movzx eax, cx
        let prod = low16.wrapping_mul(range); // imul eax, edi
        ((prod >> 16) as i32).wrapping_add(lo) // shr eax,0x10 ; add eax, ebx
    }
}

// ---------------------------------------------------------------------------------
// CRT substitutes for the rule-value tokenizer, `RString::AsScaled` at `0x00A1D110`.
//
// The *model* for that case is the shipped `don_rules::as_scaled`, not a copy. What lives
// here is the pair of CRT leaves the retail routine calls through its IAT, which we patch
// so the routine can run outside Windows. The Tier-B claim is conditional on them and the
// case record says so.
// ---------------------------------------------------------------------------------

pub mod tokenizer {
    /// MSVC `_wtoi`: skip leading whitespace, optional sign, then decimal digits; stop at
    /// the first non-digit. Accumulates in 32-bit with wrapping.
    ///
    /// This is a **substitute for the real CRT import** (IAT slot `0x00AC54AC`), patched
    /// in so the retail routine's own two leaf calls are ours. The Tier-B claim is
    /// therefore conditional on it, which the case record states.
    pub extern "C" fn wtoi(p: *const u16) -> i32 {
        if p.is_null() {
            return 0;
        }
        unsafe {
            let mut i = 0usize;
            while matches!(*p.add(i), 0x20 | 0x09 | 0x0a | 0x0b | 0x0c | 0x0d) {
                i += 1;
            }
            let mut neg = false;
            match *p.add(i) {
                0x2d => {
                    neg = true;
                    i += 1;
                }
                0x2b => i += 1,
                _ => {}
            }
            let mut acc: i32 = 0;
            loop {
                let c = *p.add(i);
                if !(0x30..=0x39).contains(&c) {
                    break;
                }
                acc = acc.wrapping_mul(10).wrapping_add((c - 0x30) as i32);
                i += 1;
            }
            if neg {
                acc.wrapping_neg()
            } else {
                acc
            }
        }
    }

    /// Substitute for `wcschr` (IAT slot `0x00AC5434`).
    pub extern "C" fn wcschr(p: *const u16, ch: u32) -> *const u16 {
        if p.is_null() {
            return std::ptr::null();
        }
        let ch = ch as u16;
        unsafe {
            let mut i = 0usize;
            loop {
                let c = *p.add(i);
                if c == ch {
                    return p.add(i);
                }
                if c == 0 {
                    return std::ptr::null();
                }
                i += 1;
            }
        }
    }

    /// Retail's IAT slots for the two leaves above.
    pub const IAT_WTOI: u32 = 0x00AC_54AC;
    pub const IAT_WCSCHR: u32 = 0x00AC_5434;

    pub fn to_utf16z(s: &str) -> Vec<u16> {
        let mut v: Vec<u16> = s.encode_utf16().collect();
        v.push(0);
        v
    }
}
