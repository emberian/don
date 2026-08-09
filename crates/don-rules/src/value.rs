//! Tokenizer for the value strings in Rise of Nations rule data.
//!
//! Rule values are prose. The shipped `rules.xml` contains entries like:
//!
//! ```text
//! <UNIT_FORMATION_SPACING value="1/16 tile (calibration for unit spacing in formations)"/>
//! <FLANK_BONUS           value="50% per level of flank (max bonus is twice this number)"/>
//! <FORCED_MARCH_SPEED    value="42m"/>
//! ```
//!
//! so a leading numeric — possibly a rational — is followed by an optional unit word and
//! then arbitrary trailing English. This module recovers the numeric and the unit token
//! exactly as written, and preserves the raw string.
//!
//! # Two layers, and they are not the same thing
//!
//! [`RuleValue::parse`] describes the **text**: which characters form the number, whether a
//! `%` follows, what the unit word says. It exists for documentation and linting.
//!
//! [`as_scaled`] and [`as_int`] are the **engine**. They are ports of
//! `RString::AsScaled` (`0x00A1D110`) and `_wtoi`, which is all the loader ever calls, and
//! they are what a value must go through before it may be used as a number. The engine
//! never looks at a `%` and never reads a unit word — `RuleValue`'s `percent` and `unit`
//! fields describe prose the engine discards, so **no computed value may depend on them**.
//!
//! # Fidelity status
//!
//! `RuleValue::parse` **[measured]**: the grammar was derived by surveying all value-bearing
//! elements in the shipped `rules.xml`, and the test suite asserts every one parses.
//!
//! [`as_scaled`] / [`as_int`] **[measured], Tier B**: `0x00A1D110` is
//! `(_wtoi(s) * scale) / _wtoi(after_first_slash)`, 32-bit wrapping multiply and truncating
//! `idiv`, with a null string or a zero denominator returning 0. Differentially tested
//! against the retail instructions on 202,643 calls with 0 mismatches
//! (`docs/derivation/economy.md` §6), and reproducing all 832 recovered shipped slots here
//! (`engine_tokenizer::reproduces_the_whole_shipped_corpus`). The Tier-B claim is **conditional on
//! the two substituted CRT leaves** `_wtoi` and `wcschr`: the harness patched those IAT
//! slots, so what was tested is the surrounding logic, not MSVC's `_wtoi` itself. Testing,
//! not verification.
//!
//! Do **not** add a general `to_f32()`. The engine's rule values are `i32`; the scale is a
//! property of the *field*, fixed at compile time in the loader (100 / 192 / 256), and it
//! lives in [`crate::rules`], never in the value string.

/// A numeric literal exactly as it appeared: either a plain decimal or a rational `a/b`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Number {
    /// A decimal literal. Stored as written; no unit conversion applied.
    Decimal(f64),
    /// A rational literal `numer/denom`, kept unevaluated so the engine's own division
    /// semantics can be applied once they are recovered from the binary.
    Rational { numer: i64, denom: i64 },
}

/// A parsed rule value.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleValue {
    /// The leading numeric, if the value began with one.
    pub number: Option<Number>,
    /// Whether a `%` immediately followed the number.
    pub percent: bool,
    /// The unit token following the number (`tile`, `tiles`, `frames`, `m`, `x`, …),
    /// lowercased. `None` when the number stands alone or is followed only by prose.
    pub unit: Option<String>,
    /// The original string, verbatim.
    pub raw: String,
}

impl RuleValue {
    /// Parse a rule value string. Never fails: a value with no leading numeric yields
    /// `number: None` and preserves `raw`, because the shipped data contains such entries
    /// and silently dropping them would hide real content.
    pub fn parse(raw: &str) -> RuleValue {
        let s = raw.trim();
        let bytes = s.as_bytes();
        let mut i = 0;

        // optional sign
        if i < bytes.len() && (bytes[i] == b'-' || bytes[i] == b'+') {
            i += 1;
        }
        let digits_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        // fractional part
        let mut saw_dot = false;
        if i < bytes.len()
            && bytes[i] == b'.'
            && i + 1 < bytes.len()
            && bytes[i + 1].is_ascii_digit()
        {
            saw_dot = true;
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
        }
        if i == digits_start {
            // no numeric at all
            return RuleValue {
                number: None,
                percent: false,
                unit: None,
                raw: raw.to_string(),
            };
        }
        let head = &s[..i];

        // rational: `a/b` with no decimal point on the left
        let number;
        if !saw_dot && i < bytes.len() && bytes[i] == b'/' {
            let denom_start = i + 1;
            let mut j = denom_start;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > denom_start {
                let numer = head.parse::<i64>().unwrap_or(0);
                let denom = s[denom_start..j].parse::<i64>().unwrap_or(0);
                number = Some(Number::Rational { numer, denom });
                i = j;
            } else {
                number = Some(Number::Decimal(head.parse::<f64>().unwrap_or(0.0)));
            }
        } else {
            number = Some(Number::Decimal(head.parse::<f64>().unwrap_or(0.0)));
        }

        // optional percent sign, possibly after spaces
        let mut k = i;
        while k < bytes.len() && bytes[k] == b' ' {
            k += 1;
        }
        let percent = k < bytes.len() && bytes[k] == b'%';
        if percent {
            i = k + 1;
        }

        // unit token: the next alphabetic run, skipping spaces. Stop at '(' so trailing
        // prose in parentheses is never mistaken for a unit.
        let mut u = i;
        while u < bytes.len() && bytes[u] == b' ' {
            u += 1;
        }
        let unit_start = u;
        while u < bytes.len() && (bytes[u].is_ascii_alphabetic() || bytes[u] == b'_') {
            u += 1;
        }
        let unit = if u > unit_start {
            Some(s[unit_start..u].to_ascii_lowercase())
        } else {
            None
        };

        RuleValue {
            number,
            percent,
            unit,
            raw: raw.to_string(),
        }
    }
}

/// `_wtoi` as the engine's CRT provides it: skip whitespace, optional sign, decimal digits,
/// stop at the first non-digit.
///
/// This is the leaf of both loader paths — `0x0057FA60` reaches it through
/// `RString::ToInt` (`0x00A1D210` → `0x00A15FC0`), and `0x00A1D110` calls it twice through
/// IAT slot `0x00AC54AC`. Accumulation wraps at 32 bits.
///
/// **Not itself Tier B.** The differential harness *substituted* this function for the
/// retail one (it patched the IAT), so what was tested is the code around it. The
/// whitespace set and the wrapping accumulate are modelled on MSVC's C-locale `wcstol`,
/// and every one of the 832 recovered shipped slots comes out right — but a value whose
/// digits overflow `i32` has never been compared against the real CRT, and MSVC's clamping
/// behaviour there is **not established**. No shipped value comes close.
///
/// Divergence worth naming: the engine parses UTF-16 and this takes `&str`. Only ASCII
/// digits and the six C whitespace codes are recognised either way, so the two agree on
/// every shipped value; a mod using non-ASCII digits would be a real divergence.
#[inline]
pub fn wtoi(s: &str) -> i32 {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let mut neg = false;
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        neg = b[i] == b'-';
        i += 1;
    }
    let mut acc: i32 = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        acc = acc.wrapping_mul(10).wrapping_add((b[i] - b'0') as i32);
        i += 1;
    }
    if neg {
        acc.wrapping_neg()
    } else {
        acc
    }
}

/// `RString::AsScaled(scale)` — `riseofnations.exe` VA `0x00A1D110`, `__thiscall`, `ret 4`.
///
/// The 40 scaled fields in `rules.xml` go through here; everything else goes through
/// [`as_int`]. `scale` belongs to the **field**, not to the string: the loader passes a
/// compile-time constant at `0x0057F950` (192 for distances in 1/192 tile, 256 for 8.8
/// fixed point, 100 for percent-style ratios). See [`crate::rules`] for which is which.
///
/// The retail body, instruction for instruction:
///
/// ```text
/// 0x00A1D113  mov edx,[ecx]        ; null string data -> return 0
/// 0x00A1D13A  call [0x00AC54AC]    ; num = _wtoi(s)
/// 0x00A1D145  call [0x00AC5434]    ; sl  = wcschr(s, '/')
/// 0x00A1D156  call [0x00AC54AC]    ; den = _wtoi(sl + 1), only if sl != NULL
/// 0x00A1D163  test ecx,ecx / je    ; den == 0 -> return 0, before the multiply
/// 0x00A1D16B  mov ecx,1            ; no slash -> den = 1
/// 0x00A1D170  imul ebx,[ebp+8]     ; num * scale, signed 32-bit, low half kept
/// 0x00A1D178  idiv ecx             ; truncates toward zero
/// ```
///
/// Consequences that are easy to assume wrongly, each of them confirmed rather than
/// supposed:
///
/// * The multiply happens **after** the division guard and **before** the divide, so
///   `1/16` at scale 192 is `192/16 = 12`, not `(1/16)*192` in any rounded sense.
/// * There is **no denominator limit**. The "largest denominator allowed is 192" note in
///   `rules.xml` is an authoring convention that keeps the division exact; the parser will
///   happily evaluate `1/7`.
/// * A `/` **anywhere** in the string opens the denominator — `wcschr` searches the whole
///   string, not just the numeric head. Trailing prose is otherwise ignored entirely.
/// * The result is `i32`. There is no float and no general fixed-point type.
///
/// # Panics
///
/// On `i32::MIN / -1`, where retail's `idiv` raises `#DE`. Unreachable from shipped data.
#[inline]
pub fn as_scaled(raw: &str, scale: i32) -> i32 {
    let num = wtoi(raw);
    let den = match raw.find('/') {
        None => 1,
        Some(i) => {
            let d = wtoi(&raw[i + 1..]);
            if d == 0 {
                return 0; // 0x00A1D163, before the multiply
            }
            d
        }
    };
    let n = num.wrapping_mul(scale); // 0x00A1D170, 32-bit imul, low half
    if n == i32::MIN && den == -1 {
        panic!("retail raises #DE here: idiv {n} / {den} at 0x00A1D178");
    }
    n / den // 0x00A1D178, truncates toward zero
}

/// The scale-1 path: `_wtoi`, used by 661 of the loader's 701 scalar sites and by every
/// array entry.
///
/// `0x0057FA60` reads the attribute and calls `RString::ToInt` (`0x00A1D210`); array loops
/// call `0x0042D9E0`, which is the same shape. A `/` in one of these fields is **not** a
/// division — eight shipped values contain one inside their prose (`"25 resources / age"`)
/// and none of them is mis-parsed.
#[inline]
pub fn as_int(raw: &str) -> i32 {
    wtoi(raw)
}

#[cfg(test)]
mod engine_tokenizer {
    use super::*;
    use crate::rules::{Parser, FIELDS, SLOTS};

    /// Captured from retail `0x00A1D110` by the differential harness on hbox
    /// (`docs/derivation/economy.md` §2.2) — **not** computed here. The project has already
    /// enshrined one hand-computed expectation that the binary disagreed with.
    #[test]
    fn matches_retail_on_captured_vectors() {
        assert_eq!(
            as_scaled(
                "1/16 tile (calibration for unit spacing in formations)",
                192
            ),
            12
        );
        assert_eq!(
            as_scaled("1/192 tile (granularity for unit movement speeds)", 192),
            1
        );
        assert_eq!(
            as_scaled("1/2 tile (calibration for target sizes)", 192),
            96
        );
        assert_eq!(as_scaled("3/2 tile", 192), 288);
        assert_eq!(as_scaled("8 tile", 192), 1536);
        assert_eq!(
            as_scaled("1/1 rate (master control for unit turn speed)", 256),
            256
        );
        assert_eq!(as_scaled("2/3 (light infantry in rocks)", 256), 170);
        assert_eq!(
            as_scaled("2/1 (units take more damage in rivers)", 256),
            512
        );
        assert_eq!(as_scaled("1/3", 256), 85);
        assert_eq!(as_scaled("1/100 -percent per # tiles", 256), 2);
        assert_eq!(as_scaled("10 resources", 256), 2560);
        assert_eq!(as_scaled("35 oil", 256), 8960);
        assert_eq!(as_scaled("12/10", 256), 307);
        assert_eq!(as_scaled("80/100", 256), 204);
        assert_eq!(
            as_scaled("6/5 base rate (See BR before adjusting)", 100),
            120
        );
        assert_eq!(
            as_scaled("3/4 progression (See BR before adjusting)", 100),
            75
        );
    }

    /// Every recovered slot of the shipped `rules.xml`, parsed at its field's scale, must
    /// yield the integer the engine stores.
    ///
    /// The expectations are `docs/derivation/rules-constants.json`, which is itself
    /// **live-validated**: 828 of 834 extracted constants were read back byte-for-byte out
    /// of a running match's `RULES` object (`docs/provenance-ledger.md`; the 834 counts two
    /// fields twice, from two binder sites, which is why there are 832 distinct slots). So
    /// this is not the parser grading its own homework — it is the parser against the
    /// running game's memory.
    #[test]
    fn reproduces_the_whole_shipped_corpus() {
        assert_eq!(SLOTS.len(), 832, "the recovered corpus changed size");
        let mut scaled = 0;
        for s in SLOTS.iter() {
            let got = if s.scale == 1 {
                as_int(s.xml_value)
            } else {
                as_scaled(s.xml_value, s.scale)
            };
            assert_eq!(
                got, s.stored,
                "{}[{}] at offset {}: parsed {:?} at scale {} as {}, engine stores {}",
                s.name, s.index, s.offset, s.xml_value, s.scale, got, s.stored
            );
            if s.scale != 1 {
                scaled += 1;
            }
        }
        // 40 scaled call sites at 0x0057F950; every one is a scalar field.
        assert_eq!(scaled, 40, "expected exactly the 40 AsScaled fields");
    }

    /// Only 100, 192 and 256 ever reach `AsScaled`; the scale is the field's property.
    #[test]
    fn the_only_scales_are_the_three_the_loader_passes() {
        for f in FIELDS.iter() {
            if let Parser::Scaled(s) = f.parser {
                assert!(matches!(s, 100 | 192 | 256), "{} has scale {}", f.name, s);
            }
        }
    }

    /// `rules.xml`'s own header says a `0` denominator "specifies no distance/speed"; the
    /// code implements that by returning before the multiply (`0x00A1D163`). No arithmetic
    /// is involved in these, which is why they are safe to assert without a capture.
    #[test]
    fn zero_denominator_returns_zero_before_the_multiply() {
        assert_eq!(as_scaled("1/0", 192), 0);
        assert_eq!(as_scaled("0/0 tile", 256), 0);
        assert_eq!(as_scaled("100/0 anything at all", 100), 0);
        // A slash with no digits after it parses as denominator 0, so also zero.
        assert_eq!(as_scaled("5/", 192), 0);
        assert_eq!(as_scaled("5/tiles", 192), 0);
    }

    /// `_wtoi` returns 0 when it finds no leading digits; `AsScaled` inherits that.
    #[test]
    fn no_leading_number_is_zero() {
        assert_eq!(as_int(""), 0);
        assert_eq!(as_int("(unused)"), 0);
        assert_eq!(as_scaled("", 192), 0);
        assert_eq!(as_scaled("tile", 192), 0);
    }

    /// The engine has no decimal point: `_wtoi` stops at the `.`. Our text-level
    /// [`RuleValue`] parser deliberately *does* read `1.5`, which is exactly why a value
    /// must never be taken from it. No shipped value has a decimal point; a mod could.
    #[test]
    fn the_engine_truncates_at_a_decimal_point_and_rulevalue_does_not() {
        assert_eq!(as_int("1.5"), 1);
        assert_eq!(RuleValue::parse("1.5").number, Some(Number::Decimal(1.5)));
    }

    /// Eight shipped `_wtoi` fields carry a `/` inside their prose. Reading one of them
    /// through the scaled path would silently divide — a real hazard for mod support, so
    /// pin that the two paths genuinely differ.
    #[test]
    fn a_slash_in_prose_is_inert_for_wtoi_fields_and_not_for_scaled_ones() {
        let v = "8 tiles (BR 1/16/2003 -- figured this should be smaller than 12)";
        assert_eq!(as_int(v), 8);
        assert_ne!(as_scaled(v, 192), as_int(v) * 192);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_with_unit() {
        let v = RuleValue::parse("1/16 tile (calibration for unit spacing in formations)");
        assert_eq!(
            v.number,
            Some(Number::Rational {
                numer: 1,
                denom: 16
            })
        );
        assert_eq!(v.unit.as_deref(), Some("tile"));
        assert!(!v.percent);
    }

    #[test]
    fn percent_with_trailing_prose() {
        let v = RuleValue::parse("50% per level of flank (max bonus is twice this number)");
        assert_eq!(v.number, Some(Number::Decimal(50.0)));
        assert!(v.percent);
        assert_eq!(v.unit.as_deref(), Some("per"));
    }

    #[test]
    fn bare_number() {
        let v = RuleValue::parse("192");
        assert_eq!(v.number, Some(Number::Decimal(192.0)));
        assert_eq!(v.unit, None);
        assert!(!v.percent);
    }

    #[test]
    fn number_with_suffix_unit_no_space() {
        let v = RuleValue::parse("42m");
        assert_eq!(v.number, Some(Number::Decimal(42.0)));
        assert_eq!(v.unit.as_deref(), Some("m"));
    }

    #[test]
    fn denominator_zero_is_preserved_not_normalised() {
        // rules.xml documents that a 0 denominator means "no distance/speed". We keep it
        // unevaluated rather than dividing, because the engine's handling is not yet known.
        let v = RuleValue::parse("0/0 tile");
        assert_eq!(v.number, Some(Number::Rational { numer: 0, denom: 0 }));
    }

    #[test]
    fn non_numeric_is_preserved() {
        let v = RuleValue::parse("(unused)");
        assert_eq!(v.number, None);
        assert_eq!(v.raw, "(unused)");
    }

    #[test]
    fn negative_number() {
        let v = RuleValue::parse("-25% per age");
        assert_eq!(v.number, Some(Number::Decimal(-25.0)));
        assert!(v.percent);
    }

    /// Every value-bearing element in the shipped `rules.xml` must yield a numeric.
    ///
    /// Skipped when `ron-data/` is absent — it holds copyrighted game content and is not
    /// committed. Re-extract it per the Provenance section of
    /// `docs/binary-ground-truth.md`.
    #[test]
    fn parses_the_whole_shipped_corpus() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../ron-data/rules.xml");
        let Ok(xml) = std::fs::read_to_string(path) else {
            eprintln!("skipping: {path} not present");
            return;
        };

        let mut total = 0usize;
        let mut no_number = Vec::new();
        for chunk in xml.split("value=\"").skip(1) {
            let Some(end) = chunk.find('"') else { continue };
            let raw = &chunk[..end];
            total += 1;
            let v = RuleValue::parse(raw);
            if v.number.is_none() {
                no_number.push(raw.to_string());
            }
        }

        assert!(
            total > 600,
            "expected the full corpus, saw only {total} values"
        );
        assert!(
            no_number.is_empty(),
            "{} of {total} shipped values yielded no numeric; first few: {:?}",
            no_number.len(),
            &no_number[..no_number.len().min(5)]
        );
    }
}
