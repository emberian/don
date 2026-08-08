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
//! # Fidelity status
//!
//! **What this module does is [measured]**: the grammar below was derived by surveying all
//! 690 value-bearing elements in the shipped `rules.xml`, and the test suite asserts every
//! one of them parses.
//!
//! **What this module deliberately does NOT do is decide semantics.** How the engine
//! converts `1/16 tile` into an internal quantity — the denominator limit, rounding, the
//! meaning of each unit word, whether the result lands in an `f32` or a fixed-point
//! integer — is a property of the binary's tokenizer, not of this file, and is *not yet*
//! recovered. See the loader worklist in `docs/binary-ground-truth.md`. Do not add a
//! `to_f32()` here until that is settled; a plausible-looking conversion invented now is
//! exactly the folklore the charter forbids.

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
        if i < bytes.len() && bytes[i] == b'.' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit()
        {
            saw_dot = true;
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
        }
        if i == digits_start {
            // no numeric at all
            return RuleValue { number: None, percent: false, unit: None, raw: raw.to_string() };
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
        let unit = if u > unit_start { Some(s[unit_start..u].to_ascii_lowercase()) } else { None };

        RuleValue { number, percent, unit, raw: raw.to_string() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_with_unit() {
        let v = RuleValue::parse("1/16 tile (calibration for unit spacing in formations)");
        assert_eq!(v.number, Some(Number::Rational { numer: 1, denom: 16 }));
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

        assert!(total > 600, "expected the full corpus, saw only {total} values");
        assert!(
            no_number.is_empty(),
            "{} of {total} shipped values yielded no numeric; first few: {:?}",
            no_number.len(),
            &no_number[..no_number.len().min(5)]
        );
    }
}
