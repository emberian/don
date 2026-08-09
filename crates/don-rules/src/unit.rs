//! Closed text primitives used by `UnitType::init` pass 2.
//!
//! The bit positions are direct instruction recoveries from `UnitType::init`
//! (`0x0061_AF3B..0x0061_AFBC` for lower-case unit flags and
//! `0x0061_B2B0..0x0061_B365` for object masks). Invalid characters are rejected here;
//! reproducing x86's masked shift count for malformed content would turn a typo into an
//! unrelated capability bit.

/// Parse `OBJ_MASK`: `A..Z` map to bits 0..25 and `1..6` to bits 26..31.
pub fn object_mask(raw: &str) -> Option<u32> {
    parse_mask(raw, b'A', b'Z')
}

/// Parse `FLAGS`: retail lowercases the string first, then maps `a..z` to bits 0..25 and
/// `1..6` to bits 26..31.
pub fn unit_flags(raw: &str) -> Option<u32> {
    if !raw.is_ascii() {
        return None;
    }
    parse_mask(&raw.to_ascii_lowercase(), b'a', b'z')
}

fn parse_mask(raw: &str, first: u8, last: u8) -> Option<u32> {
    let mut out = 0_u32;
    for byte in raw.bytes() {
        let bit = if (first..=last).contains(&byte) {
            u32::from(byte - first)
        } else if (b'1'..=b'6').contains(&byte) {
            u32::from(byte - b'1') + 26
        } else {
            return None;
        };
        out |= 1_u32 << bit;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_and_object_masks_use_the_two_exact_alphabets() {
        assert_eq!(object_mask("AZ16"), Some(0x8600_0001));
        assert_eq!(unit_flags("az16"), Some(0x8600_0001));
        assert_eq!(unit_flags("Az16"), Some(0x8600_0001));
        assert_eq!(object_mask("a"), None);
        assert_eq!(unit_flags("!"), None);
    }
}
