//! Replay-carried UnitType facts needed before the first substantive Groups transition.
//!
//! This module is deliberately narrower than a Unit constructor.  It projects one admitted
//! [`InitialRules`] section back into the exact postload `UnitTypeData` fields read by
//! `Setup::build_units`, `FormData::type_cat`, and the canonical Group-move host.  It creates
//! no Unit, Handle, position, order, path, or formation state.  Those dynamic facts must still
//! come from the existing setup receipts and [`don_sim::tick::Sim`].
//!
//! The serialized field map is the inverse of the already-owned checksum traversal:
//! `Type::walk_rules_data` writes `[+0x04,+0x5e)`, `ObjectType` writes
//! `[+0x1e4,+0x27c)`, and `UnitType` writes four ranges totaling 792 bytes.  Save-only Strings
//! and the two variable `SimpleArray<u16>` bodies are skipped with the same bounds as
//! [`crate::initial::parse_serialized_rules_at`].  The caller cannot substitute a convenient
//! row: the complete Rules span, SHA-256, and all four retail checkpoints are revalidated first.

#![forbid(unsafe_code)]

use crate::initial::{
    InitialRules, ReplayByteSpan, SHIPPED_RULES_SERIALIZED_BYTES, TAG_RULES, TAG_TRIBE,
};
use crate::rules_channel::{
    BALANCE_BYTES, RETAIL_AFTER_BALANCE, RETAIL_AFTER_CONSTANTS, RETAIL_AFTER_TRIBES,
    RETAIL_AFTER_TYPES, RETAIL_WALKED_BYTES, RULES_BLOCK_BYTES, SHIPPED_RULES_CHANNEL, TRIBE_COUNT,
    TRIBE_SIZE, TYPE_SLOTS,
};
use crate::world_owner_frontier::sha256;
use std::fmt;

pub const UNIT_TYPE_FIRST: i32 = 50;
pub const UNIT_TYPE_LAST: i32 = 413;

pub const SETUP_BUILD_UNITS_VA: u32 = 0x005a_afc0;
pub const LEADER_HAS_TRIBE_BONUS_VA: u32 = 0x006e_1370;
pub const FORM_TYPE_CAT_VA: u32 = 0x0072_dfc0;
pub const UNIT_IS_MODERN_INFANTRY_VA: u32 = 0x0060_7b40;

const TYPE_BASE_WALK_BYTES: usize = 90;
const OBJECT_WALK_BYTES: usize = 152;
const UNIT_WALK_BYTES: usize = 792;
const GOOD_TAIL_BYTES: usize = 68;
const BUILD_TAIL_BYTES: usize = 49;
const TECH_TAIL_BYTES: usize = 27;
const SPELL_TAIL_BYTES: usize = 48;
const MAX_RULES_STRING_UNITS: usize = 32_768;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayUnitTypeSpans {
    /// `TypeData + 0x04 .. +0x5e`.
    pub type_base: ReplayByteSpan,
    /// `ObjectTypeData + 0x1e4 .. +0x27c`.
    pub object: ReplayByteSpan,
    /// The four consecutive serialized UnitType ranges, with runtime holes removed.
    pub unit: ReplayByteSpan,
}

/// Exact replay-carried postload fields consumed by setup and Group formation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayUnitTypeFacts {
    pub spans: ReplayUnitTypeSpans,
    pub type_index: i32,
    pub upgrade: i32,
    pub jump: i32,
    pub obj_masks: u32,
    pub attack: i32,
    pub max_range: i32,
    pub domain: i32,
    pub guy_spacing: i32,
    pub x_spacing: i32,
    pub y_spacing: i32,
    pub graft: i32,
    pub age: i32,
    pub unit_flags: u32,
    pub unit_flags2: u32,
    pub mode: i32,
    pub moves: i32,
    pub turn_speed: i32,
    pub role: i32,
    pub military_level: i32,
    pub squad_size: i32,
    pub uber_size: i32,
    pub crew_size: i32,
    pub base_form: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayTribeTypeFacts {
    pub tribe_index: usize,
    /// `Tribe + 0x54`, also the fallback result compared by
    /// `LeaderData::has_tribe_bonus` when the explicit bit is clear.
    pub tribe_id: i32,
    /// `Tribe + 0x70 + 4 * (type_index - 50)`.
    pub nation_variant: i32,
    pub tribe_row: ReplayByteSpan,
    pub graft_word: ReplayByteSpan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayBuildTypeSpans {
    /// `TypeData + 0x04 .. +0x5e`.
    pub type_base: ReplayByteSpan,
    /// `ObjectTypeData + 0x1e4 .. +0x27c`.
    pub object: ReplayByteSpan,
    /// `BuildTypeData + 0x2b4 .. +0x2e5`.
    pub build: ReplayByteSpan,
}

/// Exact replay-carried BuildType fields used by construction placement and initialization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayBuildTypeFacts {
    pub spans: ReplayBuildTypeSpans,
    pub type_index: i32,
    pub job_time: u32,
    pub costs: [i32; 6],
    pub upgrade: i32,
    pub jump: i32,
    pub obj_masks: u32,
    pub hits: i32,
    pub domain: i32,
    pub x_size: i32,
    pub y_size: i32,
    pub graft: i32,
    pub age: i32,
    pub town_hits: i32,
    pub min_city_size: i32,
    pub misery_rate: i32,
    pub build_flags: u32,
    pub most_shots: i32,
    pub garrison_max: i32,
    pub base_arrows: i32,
    pub wonder_val: i32,
    pub plunder_value: i32,
    pub plunder_good: i32,
    pub behind_height: i32,
    pub to: i32,
    pub civ_graph_mask: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrePairUnitAuthorityError {
    RulesMetadataMismatch,
    RulesSpanOutsidePayload,
    RulesSha256Mismatch,
    WrongRulesTag {
        got: u8,
    },
    TypeIndexOutOfRange {
        type_index: i32,
    },
    TribeIndexOutOfRange {
        tribe_index: usize,
    },
    Truncated {
        at: usize,
        needed: usize,
    },
    InvalidStringLength {
        at: usize,
        units: usize,
    },
    InvalidUtf16 {
        at: usize,
    },
    InvalidArrayCount {
        at: usize,
        count: i32,
    },
    InvalidArrayCapacity {
        at: usize,
        count: i32,
        capacity: i32,
    },
    InvalidArrayFlags {
        at: usize,
        flags: u8,
    },
    WrongSerializedType {
        expected: i32,
        got: i32,
    },
    WrongTribeTag {
        tribe_index: usize,
        got: u8,
    },
}

impl fmt::Display for PrePairUnitAuthorityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pre-pair Unit/content authority refused: {self:?}")
    }
}

impl std::error::Error for PrePairUnitAuthorityError {}

fn admitted_section<'a>(
    payload: &'a [u8],
    rules: &InitialRules,
) -> Result<&'a [u8], PrePairUnitAuthorityError> {
    if rules.serialized_bytes != SHIPPED_RULES_SERIALIZED_BYTES
        || rules.walked_bytes != RETAIL_WALKED_BYTES
        || rules.checksum != SHIPPED_RULES_CHANNEL
        || rules.checksum != RETAIL_AFTER_TRIBES
        || rules.after_types != RETAIL_AFTER_TYPES
        || rules.after_constants != RETAIL_AFTER_CONSTANTS
        || rules.after_balance != RETAIL_AFTER_BALANCE
    {
        return Err(PrePairUnitAuthorityError::RulesMetadataMismatch);
    }
    let end = rules
        .serialized_offset
        .checked_add(rules.serialized_bytes)
        .ok_or(PrePairUnitAuthorityError::RulesSpanOutsidePayload)?;
    let section = payload
        .get(rules.serialized_offset..end)
        .ok_or(PrePairUnitAuthorityError::RulesSpanOutsidePayload)?;
    if sha256(section) != rules.serialized_sha256 {
        return Err(PrePairUnitAuthorityError::RulesSha256Mismatch);
    }
    let got = section
        .first()
        .copied()
        .ok_or(PrePairUnitAuthorityError::RulesSpanOutsidePayload)?;
    if got != TAG_RULES {
        return Err(PrePairUnitAuthorityError::WrongRulesTag { got });
    }
    Ok(section)
}

fn need(section: &[u8], at: usize, bytes: usize) -> Result<(), PrePairUnitAuthorityError> {
    let end = at
        .checked_add(bytes)
        .ok_or(PrePairUnitAuthorityError::Truncated { at, needed: bytes })?;
    if end > section.len() {
        return Err(PrePairUnitAuthorityError::Truncated { at, needed: bytes });
    }
    Ok(())
}

fn read_i32(section: &[u8], at: usize) -> Result<i32, PrePairUnitAuthorityError> {
    need(section, at, 4)?;
    Ok(i32::from_le_bytes(
        section[at..at + 4].try_into().expect("bounded slice"),
    ))
}

fn read_u32(section: &[u8], at: usize) -> Result<u32, PrePairUnitAuthorityError> {
    need(section, at, 4)?;
    Ok(u32::from_le_bytes(
        section[at..at + 4].try_into().expect("bounded slice"),
    ))
}

fn skip_string(section: &[u8], cursor: &mut usize) -> Result<(), PrePairUnitAuthorityError> {
    let at = *cursor;
    let units = usize::try_from(read_u32(section, at)?).map_err(|_| {
        PrePairUnitAuthorityError::InvalidStringLength {
            at,
            units: usize::MAX,
        }
    })?;
    if units > MAX_RULES_STRING_UNITS {
        return Err(PrePairUnitAuthorityError::InvalidStringLength { at, units });
    }
    let bytes = units
        .checked_mul(2)
        .ok_or(PrePairUnitAuthorityError::InvalidStringLength { at, units })?;
    need(section, at + 4, bytes)?;
    let words = section[at + 4..at + 4 + bytes]
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]));
    if std::char::decode_utf16(words).any(|value| value.is_err()) {
        return Err(PrePairUnitAuthorityError::InvalidUtf16 { at });
    }
    *cursor = at + 4 + bytes;
    Ok(())
}

fn skip_u16_array(section: &[u8], cursor: &mut usize) -> Result<(), PrePairUnitAuthorityError> {
    let at = *cursor;
    let count = read_i32(section, at)?;
    if !(0..=TYPE_SLOTS as i32).contains(&count) {
        return Err(PrePairUnitAuthorityError::InvalidArrayCount { at, count });
    }
    *cursor += 4;
    if count == 0 {
        return Ok(());
    }
    need(section, *cursor, 7)?;
    let capacity = read_i32(section, *cursor)?;
    if capacity < count {
        return Err(PrePairUnitAuthorityError::InvalidArrayCapacity {
            at,
            count,
            capacity,
        });
    }
    let flags = section[*cursor + 6];
    if flags & 0x40 != 0 {
        return Err(PrePairUnitAuthorityError::InvalidArrayFlags { at, flags });
    }
    *cursor += 7;
    let elements = usize::try_from(count).expect("nonnegative checked count") * 2;
    need(section, *cursor, elements)?;
    *cursor += elements;
    Ok(())
}

fn absolute_span(rules: &InitialRules, relative: usize, bytes: usize) -> ReplayByteSpan {
    ReplayByteSpan {
        offset: rules.serialized_offset + relative,
        bytes,
    }
}

/// Extract one exact UnitType row from a replay's already-admitted Rules section.
pub fn replay_unit_type_facts(
    payload: &[u8],
    rules: &InitialRules,
    type_index: i32,
) -> Result<ReplayUnitTypeFacts, PrePairUnitAuthorityError> {
    if !(UNIT_TYPE_FIRST..=UNIT_TYPE_LAST).contains(&type_index) {
        return Err(PrePairUnitAuthorityError::TypeIndexOutOfRange { type_index });
    }
    let section = admitted_section(payload, rules)?;
    let mut cursor = 1usize;
    for slot in 0..=type_index as usize {
        let type_base = cursor;
        need(section, cursor, TYPE_BASE_WALK_BYTES)?;
        let got = read_i32(section, cursor)?;
        if got != slot as i32 {
            return Err(PrePairUnitAuthorityError::WrongSerializedType {
                expected: slot as i32,
                got,
            });
        }
        cursor += TYPE_BASE_WALK_BYTES;
        skip_string(section, &mut cursor)?;

        let kind = match slot {
            0..=49 => 0,    // GoodType
            50..=413 => 1,  // UnitType
            414..=542 => 2, // BuildType
            543 => 3,       // ItemType -> ObjectType walker
            544..=628 => 4, // TechType
            629..=683 => 5, // SpellType
            _ => 6,         // BonusType -> Type walker
        };
        if kind <= 3 {
            let object = cursor;
            need(section, cursor, OBJECT_WALK_BYTES)?;
            cursor += OBJECT_WALK_BYTES;
            skip_u16_array(section, &mut cursor)?;
            skip_u16_array(section, &mut cursor)?;
            if slot as i32 == type_index {
                let unit = cursor;
                need(section, unit, UNIT_WALK_BYTES)?;
                let base = |runtime_offset: usize| type_base + (runtime_offset - 4);
                let object_at = |runtime_offset: usize| object + (runtime_offset - 0x1e4);
                let unit_at = |runtime_offset: usize| match runtime_offset {
                    0x2b4..=0x2cb => unit + (runtime_offset - 0x2b4),
                    0x2d4..=0x2db => unit + 24 + (runtime_offset - 0x2d4),
                    0x2dc..=0x2df => unit + 32 + (runtime_offset - 0x2dc),
                    0x2e0..=0x5d3 => unit + 36 + (runtime_offset - 0x2e0),
                    _ => unreachable!("requested UnitType offset is outside walked ranges"),
                };
                return Ok(ReplayUnitTypeFacts {
                    spans: ReplayUnitTypeSpans {
                        type_base: absolute_span(rules, type_base, TYPE_BASE_WALK_BYTES),
                        object: absolute_span(rules, object, OBJECT_WALK_BYTES),
                        unit: absolute_span(rules, unit, UNIT_WALK_BYTES),
                    },
                    type_index: got,
                    upgrade: read_i32(section, base(0x44))?,
                    jump: read_i32(section, base(0x48))?,
                    obj_masks: read_u32(section, object_at(0x1e4))?,
                    attack: read_i32(section, object_at(0x1e8))?,
                    max_range: read_i32(section, object_at(0x1fc))?,
                    domain: read_i32(section, object_at(0x218))?,
                    guy_spacing: read_i32(section, object_at(0x224))?,
                    x_spacing: read_i32(section, object_at(0x228))?,
                    y_spacing: read_i32(section, object_at(0x22c))?,
                    graft: read_i32(section, object_at(0x25c))?,
                    age: read_i32(section, object_at(0x278))?,
                    unit_flags: read_u32(section, unit_at(0x2b4))?,
                    unit_flags2: read_u32(section, unit_at(0x2b8))?,
                    mode: read_i32(section, unit_at(0x2bc))?,
                    moves: read_i32(section, unit_at(0x2c0))?,
                    turn_speed: read_i32(section, unit_at(0x2c4))?,
                    role: read_i32(section, unit_at(0x2c8))?,
                    military_level: read_i32(section, unit_at(0x2dc))?,
                    squad_size: read_i32(section, unit_at(0x304))?,
                    uber_size: read_i32(section, unit_at(0x308))?,
                    crew_size: read_i32(section, unit_at(0x30c))?,
                    base_form: read_i32(section, unit_at(0x310))?,
                });
            }
        }
        match kind {
            0 => cursor += GOOD_TAIL_BYTES,
            1 => cursor += UNIT_WALK_BYTES,
            2 => cursor += BUILD_TAIL_BYTES,
            4 => {
                cursor += TECH_TAIL_BYTES;
                for _ in 0..8 {
                    skip_string(section, &mut cursor)?;
                }
            }
            5 => cursor += SPELL_TAIL_BYTES,
            3 | 6 => {}
            _ => unreachable!(),
        }
        need(section, cursor, 0)?;
    }
    unreachable!("validated Unit TypeIndex must be reached")
}

/// Extract one exact BuildType row from the replay-carried Rules section.
///
/// This walks the same variable-length serialized Type table as
/// [`replay_unit_type_facts`]. It does not consult the installed live catalog and does not
/// infer a footprint from a TypeIndex name.
pub fn replay_build_type_facts(
    payload: &[u8],
    rules: &InitialRules,
    type_index: i32,
) -> Result<ReplayBuildTypeFacts, PrePairUnitAuthorityError> {
    if !(414..=542).contains(&type_index) {
        return Err(PrePairUnitAuthorityError::TypeIndexOutOfRange { type_index });
    }
    let section = admitted_section(payload, rules)?;
    let mut cursor = 1usize;
    for slot in 0..=type_index as usize {
        let type_base = cursor;
        need(section, cursor, TYPE_BASE_WALK_BYTES)?;
        let got = read_i32(section, cursor)?;
        if got != slot as i32 {
            return Err(PrePairUnitAuthorityError::WrongSerializedType {
                expected: slot as i32,
                got,
            });
        }
        cursor += TYPE_BASE_WALK_BYTES;
        skip_string(section, &mut cursor)?;

        let kind = match slot {
            0..=49 => 0,
            50..=413 => 1,
            414..=542 => 2,
            543 => 3,
            544..=628 => 4,
            629..=683 => 5,
            _ => 6,
        };
        if kind <= 3 {
            let object = cursor;
            need(section, cursor, OBJECT_WALK_BYTES)?;
            cursor += OBJECT_WALK_BYTES;
            skip_u16_array(section, &mut cursor)?;
            skip_u16_array(section, &mut cursor)?;
            if slot as i32 == type_index {
                let build = cursor;
                need(section, build, BUILD_TAIL_BYTES)?;
                let base = |runtime_offset: usize| type_base + (runtime_offset - 4);
                let object_at = |runtime_offset: usize| object + (runtime_offset - 0x1e4);
                let build_at = |runtime_offset: usize| build + (runtime_offset - 0x2b4);
                let costs = std::array::from_fn(|resource| {
                    read_i32(section, base(0x18 + resource * 4))
                        .expect("the fixed TypeData span was preflighted")
                });
                return Ok(ReplayBuildTypeFacts {
                    spans: ReplayBuildTypeSpans {
                        type_base: absolute_span(rules, type_base, TYPE_BASE_WALK_BYTES),
                        object: absolute_span(rules, object, OBJECT_WALK_BYTES),
                        build: absolute_span(rules, build, BUILD_TAIL_BYTES),
                    },
                    type_index: got,
                    job_time: read_u32(section, base(0x08))?,
                    costs,
                    upgrade: read_i32(section, base(0x44))?,
                    jump: read_i32(section, base(0x48))?,
                    obj_masks: read_u32(section, object_at(0x1e4))?,
                    hits: read_i32(section, object_at(0x210))?,
                    domain: read_i32(section, object_at(0x218))?,
                    x_size: read_i32(section, object_at(0x234))?,
                    y_size: read_i32(section, object_at(0x238))?,
                    graft: read_i32(section, object_at(0x25c))?,
                    age: read_i32(section, object_at(0x278))?,
                    town_hits: read_i32(section, build_at(0x2b4))?,
                    min_city_size: read_i32(section, build_at(0x2b8))?,
                    misery_rate: read_i32(section, build_at(0x2bc))?,
                    build_flags: read_u32(section, build_at(0x2c0))?,
                    most_shots: read_i32(section, build_at(0x2c4))?,
                    garrison_max: read_i32(section, build_at(0x2c8))?,
                    base_arrows: read_i32(section, build_at(0x2cc))?,
                    wonder_val: read_i32(section, build_at(0x2d0))?,
                    plunder_value: read_i32(section, build_at(0x2d4))?,
                    plunder_good: read_i32(section, build_at(0x2d8))?,
                    behind_height: read_i32(section, build_at(0x2dc))?,
                    to: read_i32(section, build_at(0x2e0))?,
                    civ_graph_mask: section[build_at(0x2e4)],
                });
            }
        }
        match kind {
            0 => cursor += GOOD_TAIL_BYTES,
            1 => cursor += UNIT_WALK_BYTES,
            2 => cursor += BUILD_TAIL_BYTES,
            4 => {
                cursor += TECH_TAIL_BYTES;
                for _ in 0..8 {
                    skip_string(section, &mut cursor)?;
                }
            }
            5 => cursor += SPELL_TAIL_BYTES,
            3 | 6 => {}
            _ => unreachable!(),
        }
        need(section, cursor, 0)?;
    }
    unreachable!("validated Build TypeIndex must be reached")
}

/// Resolve the exact nation row and graft selected by setup without constructing a Unit.
pub fn replay_tribe_type_facts(
    payload: &[u8],
    rules: &InitialRules,
    tribe_index: usize,
    type_index: i32,
) -> Result<ReplayTribeTypeFacts, PrePairUnitAuthorityError> {
    if tribe_index >= TRIBE_COUNT {
        return Err(PrePairUnitAuthorityError::TribeIndexOutOfRange { tribe_index });
    }
    if !(UNIT_TYPE_FIRST..=401).contains(&type_index) {
        return Err(PrePairUnitAuthorityError::TypeIndexOutOfRange { type_index });
    }
    let section = admitted_section(payload, rules)?;
    let tribes =
        1 + crate::initial::SHIPPED_TYPES_SERIALIZED_BYTES + RULES_BLOCK_BYTES + 4 + BALANCE_BYTES;
    let serialized_tribe_bytes = 1 + 0x18 + (TRIBE_SIZE - 0x70);
    let row = tribes + tribe_index * serialized_tribe_bytes;
    need(section, row, serialized_tribe_bytes)?;
    let got = section[row];
    if got != TAG_TRIBE {
        return Err(PrePairUnitAuthorityError::WrongTribeTag { tribe_index, got });
    }
    let graft = row + 1 + 0x18 + usize::try_from(type_index - UNIT_TYPE_FIRST).unwrap() * 4;
    Ok(ReplayTribeTypeFacts {
        tribe_index,
        tribe_id: read_i32(section, row + 1)?,
        nation_variant: read_i32(section, graft)?,
        tribe_row: absolute_span(rules, row, serialized_tribe_bytes),
        graft_word: absolute_span(rules, graft, 4),
    })
}
