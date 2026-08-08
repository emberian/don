//! Static `rules` checksum channel (`Game::walk_rules_data`, `0x00589550`).
//!
//! This module is intentionally independent of the rest of `don-replay` and can also be
//! exercised directly with
//!
//! ```text
//! rustc --edition 2021 --test crates/don-replay/src/rules_channel.rs -o /tmp/rules-channel-test
//! /tmp/rules-channel-test
//! ```
//!
//! The traversal is instruction-derived, not inferred from the shape of the data.  It
//! refuses incomplete inputs, so the retail target cannot be reached by silently hashing
//! empty type or tribe state.

#![forbid(unsafe_code)]

use std::fmt;

pub const SHIPPED_RULES_CHANNEL: u32 = 0x12ba_3104;

pub const TYPE_SLOTS: usize = 806;
pub const RULES_BLOCK_BYTES: usize = 0x0d40;
pub const RULES_DUPLICATE_OFFSET: usize = 0x0804;
pub const BALANCE_SIDE: usize = 493;
pub const BALANCE_BYTES: usize = BALANCE_SIDE * BALANCE_SIDE * 2;
pub const TRIBE_COUNT: usize = 24;
pub const TRIBE_SIZE: usize = 0x05f0;

/// Cumulative checkpoints from a read-only walk of retail PID 5236 on 2026-08-08.
///
/// The module base was `0x00d60000` (preferred-base delta `+0x00960000`).  The three
/// root pointers were read before and after the walk and were unchanged.  These are
/// cumulative adler-32 values, not hashes of the named section in isolation.
pub const RETAIL_AFTER_TYPES: u32 = 0x72e0_c3b6;
pub const RETAIL_AFTER_CONSTANTS: u32 = 0x5062_5668;
pub const RETAIL_AFTER_BALANCE: u32 = 0x56da_abc1;
pub const RETAIL_AFTER_TRIBES: u32 = SHIPPED_RULES_CHANNEL;
pub const RETAIL_TYPE_WALKED_BYTES: u64 = 473_984;
pub const RETAIL_CONSTANTS_WALKED_BYTES: u64 = 3_396;
pub const RETAIL_BALANCE_WALKED_BYTES: u64 = 486_098;
pub const RETAIL_TRIBES_WALKED_BYTES: u64 = 34_368;
pub const RETAIL_WALKED_BYTES: u64 = 997_846;

const ADLER_BASE: u32 = 65_521;
const ADLER_NMAX: usize = 5_552;

/// The seven implementations reached by `Types::walk_rules_data`'s 806 virtual calls.
///
/// `ItemType` inherits `ObjectType::walk_rules_data`, and `BonusType` inherits
/// `Type::walk_rules_data`, hence the base-class names in this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeRuleKind {
    Unit,
    Build,
    Tech,
    Object,
    Spell,
    Type,
    Good,
}

impl TypeRuleKind {
    const fn shipped_count(self) -> usize {
        match self {
            TypeRuleKind::Unit => 364,
            TypeRuleKind::Build => 129,
            TypeRuleKind::Tech => 85,
            TypeRuleKind::Object => 1,
            TypeRuleKind::Spell => 55,
            TypeRuleKind::Type => 122,
            TypeRuleKind::Good => 50,
        }
    }

    const fn index(self) -> usize {
        match self {
            TypeRuleKind::Unit => 0,
            TypeRuleKind::Build => 1,
            TypeRuleKind::Tech => 2,
            TypeRuleKind::Object => 3,
            TypeRuleKind::Spell => 4,
            TypeRuleKind::Type => 5,
            TypeRuleKind::Good => 6,
        }
    }

    const fn minimum_image_bytes(self) -> usize {
        match self {
            TypeRuleKind::Unit => 1_492,
            TypeRuleKind::Build => 741,
            TypeRuleKind::Tech => 483,
            TypeRuleKind::Object => 636,
            TypeRuleKind::Spell => 504,
            TypeRuleKind::Type => 94,
            TypeRuleKind::Good => 760,
        }
    }

    const fn uses_object_arrays(self) -> bool {
        matches!(
            self,
            TypeRuleKind::Unit | TypeRuleKind::Build | TypeRuleKind::Object | TypeRuleKind::Good
        )
    }
}

/// Checksum-visible state of `SimpleArray<unsigned short>`.
///
/// The backing pointer is deliberately absent: retail hashes `count`; for a non-empty
/// array it then hashes `capacity`, the two-byte growth hint, `flags & 0xbf`, and exactly
/// `count` little-endian elements.  Empty arrays hash only a zero count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct U16ArrayWalk<'a> {
    pub capacity: i32,
    pub grow: u16,
    pub flags: u8,
    pub elements: &'a [u16],
}

impl<'a> U16ArrayWalk<'a> {
    pub const EMPTY: U16ArrayWalk<'static> = U16ArrayWalk {
        capacity: 0,
        grow: 0,
        flags: 0,
        elements: &[],
    };
}

/// One object reached through `Types::list`, already resolved to its retail dynamic type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeRuleRecord<'a> {
    pub kind: TypeRuleKind,
    /// Byte image beginning at the retail object's `this` pointer.
    pub image: &'a [u8],
    /// `ObjectType + 0x27c` and `ObjectType + 0x298` respectively.  Ignored for
    /// base `Type`, `TechType`, and `SpellType` records.
    pub object_arrays: [U16ArrayWalk<'a>; 2],
}

/// One of the 24 contiguous `Tribe` records beginning at `*(0x00e7fa34)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TribeRecord<'a> {
    pub image: &'a [u8],
}

/// Complete inputs for the shipped static channel.
#[derive(Debug, Clone, Copy)]
pub struct StaticRules<'a> {
    pub types: &'a [TypeRuleRecord<'a>],
    /// Exactly `Constants + 0 .. + 0xd40`.
    pub constants: &'a [u8],
    /// Exactly the 493 x 493 `final_balance_table`, row-major little-endian i16.
    pub balance: &'a [u8],
    pub tribes: &'a [TribeRecord<'a>],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RulesCheckpoints {
    pub after_types: u32,
    pub after_constants: u32,
    pub after_balance: u32,
    pub after_tribes: u32,
    pub bytes_walked: u64,
}

impl RulesCheckpoints {
    pub fn matches_retail(self) -> bool {
        self.after_types == RETAIL_AFTER_TYPES
            && self.after_constants == RETAIL_AFTER_CONSTANTS
            && self.after_balance == RETAIL_AFTER_BALANCE
            && self.after_tribes == RETAIL_AFTER_TRIBES
            && self.bytes_walked == RETAIL_WALKED_BYTES
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RulesChannelError {
    TypeCount {
        actual: usize,
    },
    TypeKindCount {
        kind: TypeRuleKind,
        expected: usize,
        actual: usize,
    },
    TypeImage {
        slot: usize,
        kind: TypeRuleKind,
        needed: usize,
        actual: usize,
    },
    TooManyArrayElements {
        slot: usize,
        array: usize,
        actual: usize,
    },
    ConstantsLength {
        actual: usize,
    },
    BalanceLength {
        actual: usize,
    },
    TribeCount {
        actual: usize,
    },
    TribeImage {
        tribe: usize,
        actual: usize,
    },
    CaptureMissingBlock,
    InvalidBase64 {
        offset: usize,
    },
    CapturedConstantsTooShort {
        actual: usize,
    },
}

impl fmt::Display for RulesChannelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for RulesChannelError {}

#[derive(Debug, Clone, Copy)]
struct Adler32 {
    s1: u32,
    s2: u32,
    bytes: u64,
}

impl Adler32 {
    fn new() -> Self {
        Adler32 {
            s1: 1,
            s2: 0,
            bytes: 0,
        }
    }

    fn update(&mut self, bytes: &[u8]) {
        self.bytes += bytes.len() as u64;
        let mut at = 0;
        while at < bytes.len() {
            let n = ADLER_NMAX.min(bytes.len() - at);
            for &b in &bytes[at..at + n] {
                self.s1 += u32::from(b);
                self.s2 += self.s1;
            }
            self.s1 %= ADLER_BASE;
            self.s2 %= ADLER_BASE;
            at += n;
        }
    }

    fn value(self) -> u32 {
        (self.s2 << 16) | self.s1
    }
}

impl StaticRules<'_> {
    /// Execute the complete shipped traversal, returning the retail section checkpoints.
    pub fn checksum(&self) -> Result<RulesCheckpoints, RulesChannelError> {
        validate(self)?;
        let mut adler = Adler32::new();

        for (slot, ty) in self.types.iter().enumerate() {
            walk_type(slot, ty, &mut adler)?;
        }
        let after_types = adler.value();

        // Constants::walk_data, inlined by Game::walk_rules_data.  The second range is
        // intentionally inside the first: retail hashes this dword twice.
        adler.update(self.constants);
        adler.update(&self.constants[RULES_DUPLICATE_OFFSET..RULES_DUPLICATE_OFFSET + 4]);
        let after_constants = adler.value();

        // Balance::walk_rules_data makes 243,049 two-byte visitor calls.  Adler-32 is
        // compositional, so one update over the identical byte sequence is equivalent.
        adler.update(self.balance);
        let after_balance = adler.value();

        for tribe in self.tribes {
            // The preceding walk_test emits nothing for CheckSum.
            adler.update(&tribe.image[0x54..0x6c]);
            adler.update(&tribe.image[0x70..0x5f0]);
        }

        Ok(RulesCheckpoints {
            after_types,
            after_constants,
            after_balance,
            after_tribes: adler.value(),
            bytes_walked: adler.bytes,
        })
    }
}

fn validate(rules: &StaticRules<'_>) -> Result<(), RulesChannelError> {
    if rules.types.len() != TYPE_SLOTS {
        return Err(RulesChannelError::TypeCount {
            actual: rules.types.len(),
        });
    }
    let mut kinds = [0usize; 7];
    for ty in rules.types {
        kinds[ty.kind.index()] += 1;
    }
    for kind in [
        TypeRuleKind::Unit,
        TypeRuleKind::Build,
        TypeRuleKind::Tech,
        TypeRuleKind::Object,
        TypeRuleKind::Spell,
        TypeRuleKind::Type,
        TypeRuleKind::Good,
    ] {
        let actual = kinds[kind.index()];
        let expected = kind.shipped_count();
        if actual != expected {
            return Err(RulesChannelError::TypeKindCount {
                kind,
                expected,
                actual,
            });
        }
    }
    if rules.constants.len() != RULES_BLOCK_BYTES {
        return Err(RulesChannelError::ConstantsLength {
            actual: rules.constants.len(),
        });
    }
    if rules.balance.len() != BALANCE_BYTES {
        return Err(RulesChannelError::BalanceLength {
            actual: rules.balance.len(),
        });
    }
    if rules.tribes.len() != TRIBE_COUNT {
        return Err(RulesChannelError::TribeCount {
            actual: rules.tribes.len(),
        });
    }
    for (tribe, image) in rules.tribes.iter().enumerate() {
        if image.image.len() < TRIBE_SIZE {
            return Err(RulesChannelError::TribeImage {
                tribe,
                actual: image.image.len(),
            });
        }
    }
    Ok(())
}

fn walk_type(
    slot: usize,
    ty: &TypeRuleRecord<'_>,
    adler: &mut Adler32,
) -> Result<(), RulesChannelError> {
    let needed = ty.kind.minimum_image_bytes();
    if ty.image.len() < needed {
        return Err(RulesChannelError::TypeImage {
            slot,
            kind: ty.kind,
            needed,
            actual: ty.image.len(),
        });
    }

    match ty.kind {
        TypeRuleKind::Unit => {
            walk_object(slot, ty, adler)?;
            adler.update(&ty.image[692..716]);
            adler.update(&ty.image[724..732]);
            adler.update(&ty.image[732..736]);
            adler.update(&ty.image[736..1_492]);
        }
        TypeRuleKind::Build => {
            walk_object(slot, ty, adler)?;
            adler.update(&ty.image[692..741]);
        }
        TypeRuleKind::Tech => {
            walk_type_base(ty, adler);
            adler.update(&ty.image[456..483]);
        }
        TypeRuleKind::Object => walk_object(slot, ty, adler)?,
        TypeRuleKind::Spell => {
            walk_type_base(ty, adler);
            adler.update(&ty.image[456..504]);
        }
        TypeRuleKind::Type => walk_type_base(ty, adler),
        TypeRuleKind::Good => {
            walk_object(slot, ty, adler)?;
            adler.update(&ty.image[692..760]);
        }
    }
    Ok(())
}

fn walk_type_base(ty: &TypeRuleRecord<'_>, adler: &mut Adler32) {
    adler.update(&ty.image[4..94]);
    // String at +0x74 is gated off when DataWalk::is_checksum is non-zero.
}

fn walk_object(
    slot: usize,
    ty: &TypeRuleRecord<'_>,
    adler: &mut Adler32,
) -> Result<(), RulesChannelError> {
    debug_assert!(ty.kind.uses_object_arrays());
    walk_type_base(ty, adler);
    adler.update(&ty.image[484..636]);
    walk_u16_array(slot, 0, &ty.object_arrays[0], adler)?;
    walk_u16_array(slot, 1, &ty.object_arrays[1], adler)
}

fn walk_u16_array(
    slot: usize,
    array: usize,
    value: &U16ArrayWalk<'_>,
    adler: &mut Adler32,
) -> Result<(), RulesChannelError> {
    let count = i32::try_from(value.elements.len()).map_err(|_| {
        RulesChannelError::TooManyArrayElements {
            slot,
            array,
            actual: value.elements.len(),
        }
    })?;
    adler.update(&count.to_le_bytes());
    if count == 0 {
        return Ok(());
    }
    adler.update(&value.capacity.to_le_bytes());
    adler.update(&value.grow.to_le_bytes());
    adler.update(&[value.flags & 0xbf]);
    for element in value.elements {
        adler.update(&element.to_le_bytes());
    }
    Ok(())
}

/// The correct checked-in balance capture (`Balance::final_balance_table + 4`).
pub fn repository_balance() -> &'static [u8] {
    include_bytes!("../../../schema/live/final-balance-runtime.bin")
}

/// Decode the checked-in live Constants capture and return only the checksummed block.
///
/// This is an input helper, not a claim of a complete repository channel.  The generic
/// 806-entry `Types::list` (including the two pointed-to u16 arrays per ObjectType) and
/// the 24 Tribe records still need a shipped-data builder or a narrow live reader.
pub fn repository_constants() -> Result<Vec<u8>, RulesChannelError> {
    let capture = include_str!("../../../schema/live/rules-block-pid14644.txt");
    let encoded = capture
        .lines()
        .find_map(|line| line.strip_prefix("BLK="))
        .ok_or(RulesChannelError::CaptureMissingBlock)?;
    let decoded = decode_base64(encoded)?;
    if decoded.len() < RULES_BLOCK_BYTES {
        return Err(RulesChannelError::CapturedConstantsTooShort {
            actual: decoded.len(),
        });
    }
    Ok(decoded[..RULES_BLOCK_BYTES].to_vec())
}

fn decode_base64(s: &str) -> Result<Vec<u8>, RulesChannelError> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut quartet = [0u8; 4];
    let mut used = 0;
    let mut padding = 0;
    for (offset, byte) in s.bytes().enumerate() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                padding += 1;
                0
            }
            _ => return Err(RulesChannelError::InvalidBase64 { offset }),
        };
        quartet[used] = value;
        used += 1;
        if used == 4 {
            out.push((quartet[0] << 2) | (quartet[1] >> 4));
            if padding < 2 {
                out.push((quartet[1] << 4) | (quartet[2] >> 2));
            }
            if padding == 0 {
                out.push((quartet[2] << 6) | quartet[3]);
            }
            used = 0;
            padding = 0;
        }
    }
    if used != 0 {
        return Err(RulesChannelError::InvalidBase64 { offset: s.len() });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    static TYPE_IMAGE: [u8; 1_492] = [0; 1_492];
    static TRIBE_IMAGE: [u8; TRIBE_SIZE] = [0; TRIBE_SIZE];

    fn record(kind: TypeRuleKind) -> TypeRuleRecord<'static> {
        TypeRuleRecord {
            kind,
            image: &TYPE_IMAGE,
            object_arrays: [U16ArrayWalk::EMPTY, U16ArrayWalk::EMPTY],
        }
    }

    fn fixture_types() -> Vec<TypeRuleRecord<'static>> {
        let mut out = Vec::with_capacity(TYPE_SLOTS);
        for kind in [
            TypeRuleKind::Unit,
            TypeRuleKind::Build,
            TypeRuleKind::Tech,
            TypeRuleKind::Object,
            TypeRuleKind::Spell,
            TypeRuleKind::Type,
            TypeRuleKind::Good,
        ] {
            out.extend(std::iter::repeat_n(record(kind), kind.shipped_count()));
        }
        out
    }

    fn fixture_tribes() -> Vec<TribeRecord<'static>> {
        vec![
            TribeRecord {
                image: &TRIBE_IMAGE
            };
            TRIBE_COUNT
        ]
    }

    fn fixture<'a>(
        types: &'a [TypeRuleRecord<'a>],
        constants: &'a [u8],
        balance: &'a [u8],
        tribes: &'a [TribeRecord<'a>],
    ) -> StaticRules<'a> {
        StaticRules {
            types,
            constants,
            balance,
            tribes,
        }
    }

    #[test]
    fn repository_captures_have_the_required_lengths() {
        assert_eq!(repository_constants().unwrap().len(), RULES_BLOCK_BYTES);
        assert_eq!(repository_balance().len(), BALANCE_BYTES);
    }

    #[test]
    fn complete_shape_walks_every_expected_byte() {
        let types = fixture_types();
        let constants = vec![0; RULES_BLOCK_BYTES];
        let balance = vec![0; BALANCE_BYTES];
        let tribes = fixture_tribes();
        let result = fixture(&types, &constants, &balance, &tribes)
            .checksum()
            .unwrap();

        assert_eq!(
            RETAIL_TYPE_WALKED_BYTES
                + RETAIL_CONSTANTS_WALKED_BYTES
                + RETAIL_BALANCE_WALKED_BYTES
                + RETAIL_TRIBES_WALKED_BYTES,
            RETAIL_WALKED_BYTES
        );
        // This synthetic fixture has 11,460 fewer non-empty u16-array metadata/element
        // bytes than retail.  Everything else has a fixed, instruction-derived size.
        assert_eq!(result.bytes_walked, 986_386);
        assert_eq!(
            result.bytes_walked - (RETAIL_WALKED_BYTES - RETAIL_TYPE_WALKED_BYTES),
            462_524
        );
    }

    #[test]
    fn every_root_component_bites_the_checksum() {
        let types = fixture_types();
        let constants = vec![0; RULES_BLOCK_BYTES];
        let balance = vec![0; BALANCE_BYTES];
        let tribes = fixture_tribes();
        let baseline = fixture(&types, &constants, &balance, &tribes)
            .checksum()
            .unwrap();

        let mut changed_image = TYPE_IMAGE;
        changed_image[4] = 1;
        let mut changed_types = types.clone();
        changed_types[0].image = &changed_image;
        assert_ne!(
            fixture(&changed_types, &constants, &balance, &tribes)
                .checksum()
                .unwrap()
                .after_types,
            baseline.after_types
        );

        let mut changed_constants = constants.clone();
        changed_constants[RULES_DUPLICATE_OFFSET] = 1;
        assert_ne!(
            fixture(&types, &changed_constants, &balance, &tribes)
                .checksum()
                .unwrap()
                .after_constants,
            baseline.after_constants
        );

        let mut changed_balance = balance.clone();
        *changed_balance.last_mut().unwrap() = 1;
        assert_ne!(
            fixture(&types, &constants, &changed_balance, &tribes)
                .checksum()
                .unwrap()
                .after_balance,
            baseline.after_balance
        );

        let mut changed_tribe_image = TRIBE_IMAGE;
        changed_tribe_image[0x54] = 1;
        let mut changed_tribes = tribes.clone();
        changed_tribes[0].image = &changed_tribe_image;
        assert_ne!(
            fixture(&types, &constants, &balance, &changed_tribes)
                .checksum()
                .unwrap()
                .after_tribes,
            baseline.after_tribes
        );
    }

    #[test]
    fn object_array_elements_and_metadata_bite() {
        let constants = vec![0; RULES_BLOCK_BYTES];
        let balance = vec![0; BALANCE_BYTES];
        let tribes = fixture_tribes();
        let mut types = fixture_types();
        let object = TypeRuleKind::Unit.shipped_count()
            + TypeRuleKind::Build.shipped_count()
            + TypeRuleKind::Tech.shipped_count();

        let element = [7u16];
        types[object].object_arrays[0] = U16ArrayWalk {
            capacity: 4,
            grow: 2,
            flags: 3,
            elements: &element,
        };
        let a = fixture(&types, &constants, &balance, &tribes)
            .checksum()
            .unwrap();

        types[object].object_arrays[0].capacity += 1;
        let b = fixture(&types, &constants, &balance, &tribes)
            .checksum()
            .unwrap();
        assert_ne!(a.after_types, b.after_types);

        // Retail masks the transient 0x40 bit before walking the flags byte.
        types[object].object_arrays[0].flags ^= 0x40;
        let c = fixture(&types, &constants, &balance, &tribes)
            .checksum()
            .unwrap();
        assert_eq!(b.after_types, c.after_types);

        types[object].object_arrays[0].flags ^= 0x01;
        let d = fixture(&types, &constants, &balance, &tribes)
            .checksum()
            .unwrap();
        assert_ne!(c.after_types, d.after_types);
    }

    #[test]
    fn incomplete_roots_are_rejected_not_hashed_as_empty() {
        let types = fixture_types();
        let constants = vec![0; RULES_BLOCK_BYTES];
        let balance = vec![0; BALANCE_BYTES];
        let tribes = fixture_tribes();

        assert!(matches!(
            fixture(&types[..TYPE_SLOTS - 1], &constants, &balance, &tribes).checksum(),
            Err(RulesChannelError::TypeCount { .. })
        ));
        assert!(matches!(
            fixture(
                &types,
                &constants[..RULES_BLOCK_BYTES - 1],
                &balance,
                &tribes
            )
            .checksum(),
            Err(RulesChannelError::ConstantsLength { .. })
        ));
        assert!(matches!(
            fixture(&types, &constants, &balance[..BALANCE_BYTES - 1], &tribes).checksum(),
            Err(RulesChannelError::BalanceLength { .. })
        ));
        assert!(matches!(
            fixture(&types, &constants, &balance, &tribes[..TRIBE_COUNT - 1]).checksum(),
            Err(RulesChannelError::TribeCount { .. })
        ));
    }

    #[test]
    fn wrong_dynamic_type_mix_is_rejected() {
        let mut types = fixture_types();
        types[0].kind = TypeRuleKind::Type;
        let constants = vec![0; RULES_BLOCK_BYTES];
        let balance = vec![0; BALANCE_BYTES];
        let tribes = fixture_tribes();
        assert!(matches!(
            fixture(&types, &constants, &balance, &tribes).checksum(),
            Err(RulesChannelError::TypeKindCount { .. })
        ));
    }
}
