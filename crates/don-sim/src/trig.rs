//! The engine's integer trigonometry, from `basic/arith.cpp`.
//!
//! This matters more than it looks. `Unit::move_step` `0x005FAF30` calls `sin_table`
//! twice and `find_angle` once and contains **no other trig**; `PathFinder` is entirely
//! integer. So the whole locomotion path is reachable without touching a float, and
//! these three functions are the reason.
//!
//! # Angles are binary
//!
//! A full turn is 2^32. `0x40000000` is 90 degrees, `0x80000000` is 180. Angle 0 points
//! at **-y** and `0x40000000` at **+x** [measured, from `find_angle`'s degenerate cases
//! at `0x0092D140` and `0x0092D155`], i.e. a clockwise compass with north at zero.
//!
//! # Provenance
//!
//! | symbol | VA | what |
//! |---|---|---|
//! | `sin_table(angle, mag)` | `0x00A46A00` | `mag * sin(angle)`, table + lerp |
//! | `cos_table(angle, mag)` | `0x00A469F0` | `sin_table(angle + 0x3FFFFFFF, mag)` |
//! | `find_angle(dx, dy)` | `0x0092D130` | integer `atan2` |
//! | `trig_init()` | `0x00A46980` | fills `sine_table` |
//! | `int sine_table[256]` | `0x00E32F40` | the lookup, 1024 bytes |
//!
//! The table is **zero in the image** — `0x00E32F40` sits past `.data`'s raw size — and
//! is built at startup by `trig_init`, whose body is a two-at-a-time SSE loop computing
//! `trunc(sin(i * 1.570796327 / 255.0) * 65535.0)`, with all three doubles read out of
//! `.rdata` (`0xB69BB0`, `0xB69BD0`, `0xB69BE0`). See
//! [`crate::generated::state::SINE_TABLE`] for the values and their caveat.
//!
//! Note `255.0`, not `256.0`: index 255 is exactly 90 degrees, so the table's last entry
//! is the maximum and index 256 wraps to entry 0. That wrap is visible in the output —
//! see the note on [`sin_table`].
//!
//! # Fidelity
//!
//! **Tier C.** Instruction-by-instruction transcriptions, tested here for internal
//! consistency and against `f64` trig within the approximations' own error bounds. No
//! oracle execution has compared them against retail.

use crate::generated::state::SINE_TABLE;

/// Quarter turn. `find_angle` returns this for a due-east vector.
pub const QUARTER_TURN: i32 = 0x4000_0000;
/// Half turn.
pub const HALF_TURN: i32 = -0x8000_0000; // 0x80000000 as i32

/// `sin_table` `0x00A46A00` — `mag * sin(angle)`, integer throughout.
///
/// Structure, in retail's order:
///
/// ```text
/// esi  = angle >= 0 ? mag : -mag         ; cmovns on the sign of the ANGLE
/// edi  = angle & 0x3FFFFFFF              ; position inside the half-turn
/// eax  = angle & 0x40000000              ; which quarter
/// idx  = edi >> 22                       ; 0..255
/// frac = edi & 0x3FFFFF                  ; 22 fractional bits
/// ; then one of three shift regimes chosen by |esi|, to keep the final imul in range
/// v    = lerp(T[idx], T[idx+1], frac)    ; quarter 0
///      = 0xFFFF - T[idx] + delta         ; quarter 1
/// ret  = (v * esi) >> shift
/// ```
///
/// Three details that are faithful rather than accidental:
///
/// * **The magnitude is signed by the angle**, not by itself: a negative angle negates
///   the magnitude before anything else happens (`cmovns esi, edx` at `0x00A46A0C`).
/// * **The second-quarter branch is not a mirror of the first.** It computes
///   `0xFFFF - T[i] + delta`, where a mirror would be `0xFFFF - (T[i] + delta)`. The sign
///   of the interpolation term does not flip. That is what the machine code does
///   (`sub eax, ecx; add eax, 0xffff` at `0x00A46A50`), so it is what this does.
/// * **Index 255 interpolates toward index 0**, because `inc dl` wraps in a byte. At
///   exactly 90 degrees the delta is `-65535`, and `-65535 * frac` overflows `i32`. The
///   overflow is retail's; it is reproduced with `wrapping_mul` rather than widened away.
#[inline]
pub fn sin_table(angle: i32, mag: i32) -> i32 {
    // 00a46a06 neg esi / 00a46a0c cmovns esi, edx
    let mut m = if angle >= 0 { mag } else { mag.wrapping_neg() };
    let a = angle & 0x3FFF_FFFF;
    let quarter = angle & 0x4000_0000;
    let idx = ((a as u32) >> 22) as u8;
    let frac = a & 0x003F_FFFF;

    // 00a46a25 / 00a46a7e: pick a shift so the final imul does not overflow.
    let shift = if m < 0xFFFF {
        16
    } else if m < 0x00FF_FFFF {
        m >>= 8;
        8
    } else {
        m >>= 16;
        0
    };

    let t0 = SINE_TABLE[idx as usize];
    let t1 = SINE_TABLE[idx.wrapping_add(1) as usize];
    let delta = t1.wrapping_sub(t0).wrapping_mul(frac) >> 22;
    let v = if quarter == 0 {
        delta.wrapping_add(t0)
    } else {
        delta.wrapping_sub(t0).wrapping_add(0xFFFF)
    };
    v.wrapping_mul(m) >> shift
}

/// `cos_table` `0x00A469F0` — literally `add ecx, 0x3FFFFFFF; jmp sin_table`.
///
/// Note the constant is `0x3FFFFFFF`, one short of a true quarter turn. That is what the
/// two-instruction body contains. `Unit::move_step` does **not** use this function: it
/// inlines its own cosine with `lea edx, [eax + 0x40000000]` (`0x005FB5FA`), i.e. the
/// exact quarter. Both are reproduced — [`cos_table`] here, [`cos_move_step`] below —
/// because they are not the same function and picking one would silently pick a bug.
#[inline]
pub fn cos_table(angle: i32, mag: i32) -> i32 {
    sin_table(angle.wrapping_add(0x3FFF_FFFF), mag)
}

/// The folded sine and cosine — **use these, not [`sin_table`] directly.**
///
/// `sinx` `0x0092D100` and `cosx` `0x0092D0C0` pre-fold the angle into
/// `[0, 0x3FFFFFFF]` before calling `sin_table`, so the raw function's second-quarter
/// arm — the one that is not a mirror — is never reached through them. Every simulation
/// caller (`Guy::move`, `Unit::move_step`, `PathFinder::find_upath`) goes through the
/// fold, inlined or not. Calling [`sin_table`] where retail calls `sinx` gives a
/// different number in two of the four quadrants.
///
/// The implementations live in [`crate::systems::groups_guys`], which derived them; they
/// are re-exported here so there is exactly one of each in the crate.
pub use crate::systems::groups_guys::{angle_diff, cosx, sinx};

/// `find_angle` `0x0092D130` — integer `atan2`, returning a binary angle.
///
/// Degenerate cases first, exactly as retail orders them:
/// `dx == 0` returns `0` (north) or `0x80000000` (south); `dy == 0` returns
/// `0x40000000` (east) or `0xC0000000` (west).
///
/// Otherwise it forms `r = min(|dx|,|dy|) / max(|dx|,|dy|)` in Q14 with a real `idiv`,
/// evaluates the cheap rational
/// `base = ((0x2800 - (|0x1333 - r| * 0xB00 >> 14)) * r) & 0xFFFFC000`, shifts it left
/// two, and assembles the quadrant from the signs plus which of `|dx|`/`|dy|` won.
/// Peak error against a true `atan2` is about a third of a degree.
#[inline]
pub fn find_angle(dx: i32, dy: i32) -> i32 {
    // The function works in a y-up frame: it negates dy on entry (0x0092D13A).
    let ny = dy.wrapping_neg();
    if dx == 0 {
        return if ny > 0 { 0 } else { HALF_TURN };
    }
    if ny == 0 {
        return if dx > 0 { QUARTER_TURN } else { -QUARTER_TURN };
    }
    let adx = dx.wrapping_abs();
    let any = ny.wrapping_abs();
    // 0x0092D178: whichever axis is larger becomes the denominator.
    let (num, den, horizontal) = if adx > any {
        (any << 14, adx, true)
    } else {
        (adx << 14, any, false)
    };
    let r = num / den; // idiv; den != 0 because both degenerate cases returned
    let err = (0x1333i32).wrapping_sub(r).wrapping_abs();
    let c = err.wrapping_mul(0xB00) >> 14;
    let base = ((0x2800i32).wrapping_sub(c).wrapping_mul(r) & 0xFFFF_C000u32 as i32) << 2;

    if dx > 0 {
        if ny > 0 {
            // NE quadrant: 0 .. 90
            if horizontal {
                QUARTER_TURN.wrapping_sub(base)
            } else {
                base
            }
        } else {
            // SE quadrant: 90 .. 180
            if horizontal {
                base.wrapping_add(QUARTER_TURN)
            } else {
                HALF_TURN.wrapping_sub(base)
            }
        }
    } else if ny > 0 {
        // NW quadrant: 270 .. 360
        if horizontal {
            base.wrapping_add(0xC000_0000u32 as i32)
        } else {
            base.wrapping_neg()
        }
    } else {
        // SW quadrant: 180 .. 270
        if horizontal {
            (0xC000_0000u32 as i32).wrapping_sub(base)
        } else {
            base.wrapping_sub(HALF_TURN)
        }
    }
}

/// A binary angle as a fraction of a turn, for tests and reporting.
pub fn turns(angle: i32) -> f64 {
    (angle as u32) as f64 / 4_294_967_296.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_endpoints_are_what_trig_init_computes() {
        assert_eq!(SINE_TABLE[0], 0);
        assert_eq!(SINE_TABLE[255], 65535, "index 255 is exactly 90 degrees");
        // Monotone rise over the quarter turn.
        for i in 1..256 {
            assert!(
                SINE_TABLE[i] > SINE_TABLE[i - 1],
                "table not monotone at {i}"
            );
        }
    }

    #[test]
    fn cardinal_directions() {
        assert_eq!(find_angle(0, -10), 0, "north");
        assert_eq!(find_angle(10, 0), QUARTER_TURN, "east");
        assert_eq!(find_angle(0, 10), HALF_TURN, "south");
        assert_eq!(find_angle(-10, 0), -QUARTER_TURN, "west");
    }

    /// The approximation's own error bound. The number is **measured here**, not a
    /// target: `find_angle`'s peak deviation from a true `atan2` over the 129x129 integer
    /// neighbourhood is 0.4485 degrees, and it occurs near the octant boundaries where
    /// the rational fit is worst. The bound exists to catch a transcription regression,
    /// so it is set just above the measured peak.
    #[test]
    fn find_angle_tracks_atan2_within_half_a_degree() {
        let mut worst = 0.0f64;
        for dx in -64..=64i32 {
            for dy in -64..=64i32 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let got = turns(find_angle(dx, dy)) * 360.0;
                // Engine frame: 0 = -y, 90 = +x.
                let want = (dx as f64)
                    .atan2(-(dy as f64))
                    .to_degrees()
                    .rem_euclid(360.0);
                let mut d = (got - want).abs();
                if d > 180.0 {
                    d = 360.0 - d;
                }
                worst = worst.max(d);
            }
        }
        assert!(worst < 0.45, "peak find_angle error {worst} deg");
    }

    /// The **raw** `sin_table` is only a sine on the folded domain `[0, 0x3FFFFFFF]`.
    ///
    /// Peak deviation there is 2.15 parts in 1000, which is the table's own 255-versus-256
    /// scale error plus truncation — not a port bug.
    #[test]
    fn raw_sin_table_is_a_sine_on_the_folded_domain_only() {
        let mag = 1000i32;
        let mut worst = 0.0f64;
        for k in 0..1024u32 {
            let angle = ((k as u64 * 0x3FFF_FFFF) / 1023) as i32;
            let got = sin_table(angle, mag) as f64;
            let want = (turns(angle) * std::f64::consts::TAU).sin() * mag as f64;
            worst = worst.max((got - want).abs());
        }
        assert!(worst < 2.5, "peak raw sin_table error {worst} of {mag}");

        // And off that domain it is *not* a sine: the second-quarter arm computes
        // `0xFFFF - T[i] + delta`, which is not the mirror of `T[i] + delta`. This is the
        // reason `sinx` exists and the reason nothing may call `sin_table` unfolded.
        let second_quarter = 0x5000_0000i32; // 112.5 degrees
        let raw = sin_table(second_quarter, mag);
        let folded = sinx(second_quarter, mag);
        assert_ne!(
            raw, folded,
            "the fold has to change something, or it is pointless"
        );
        assert!(
            (folded as f64 - 923.9).abs() < 5.0,
            "sinx(112.5deg) ~= 0.924, got {folded}"
        );
    }

    /// The folded pair is a real sine and cosine over the whole turn.
    #[test]
    fn sinx_and_cosx_track_real_trig_over_the_whole_turn() {
        let mag = 1000i32;
        let (mut ws, mut wc) = (0.0f64, 0.0f64);
        for k in 0..4096u32 {
            let angle = (k << 20) as i32;
            let t = turns(angle) * std::f64::consts::TAU;
            ws = ws.max((sinx(angle, mag) as f64 - t.sin() * mag as f64).abs());
            wc = wc.max((cosx(angle, mag) as f64 - t.cos() * mag as f64).abs());
        }
        // 3.17 parts in 1000 measured; the budget is the table's 255/256 scale error.
        assert!(ws < 3.5, "peak sinx error {ws}");
        assert!(wc < 3.5, "peak cosx error {wc}");
    }

    /// Retail overflows for magnitudes near `0xFFFF`, and the overflow is reproduced.
    ///
    /// In the first magnitude regime the final `imul` is `unit * mag` with `unit` up to
    /// 65536, so `mag >= ~32768` can wrap `i32`. At `mag = 65534` the answer near 90
    /// degrees comes out **0 and -4** instead of ~65534. A port that widened the multiply
    /// to 64 bits would be "more correct" and would desync.
    #[test]
    fn the_near_full_scale_overflow_is_reproduced() {
        assert_eq!(sin_table(0x3FFF_FF00, 65534), 0);
        assert_eq!(sin_table(0x3FB0_0000, 65534), -4);
        // One regime up, the shift keeps it in range and the answer is sane again.
        assert_eq!(sin_table(0x3FFF_FF00, 65535), 65282);
        // Well inside the first regime nothing overflows.
        assert_eq!(sin_table(0x3FFF_FF00, 1000), 1000);
    }

    /// A unit told to walk along the angle it just measured stays on the ray.
    #[test]
    fn angle_then_step_stays_on_the_ray() {
        for &(dx, dy) in &[(100i32, 0i32), (0, -100), (70, -70), (-40, 90), (-30, -80)] {
            let a = find_angle(dx, dy);
            let sx = sinx(a, 100);
            // Engine frame: angle 0 is -y, so the y step is the negated cosine.
            let sy = -cosx(a, 100);
            // Cross product against the true direction should be small.
            let cross = (sx as f64) * (dy as f64) - (sy as f64) * (dx as f64);
            let scale = ((dx * dx + dy * dy) as f64).sqrt() * 100.0;
            assert!(
                cross.abs() / scale < 0.02,
                "step drifted off the ray: {cross} / {scale}"
            );
        }
    }
}
