// SPDX-License-Identifier: GPL-3.0-or-later
//! Conditional completion of the children walked after `LeaderData::Personality`.
//!
//! `LeaderData::walk_data` (`0x006d6750`) does not continue in address order after
//! the fixed body.  It walks the raw 96-byte `Personality`, six `BitMask` children,
//! `Sites`, `MakeList`, three `SimpleArray<int>` children, a UTF-16 `String`, three
//! rare masks, and finally 62 *decoded* `LeaderDataEncrypt` dwords.  This module
//! reproduces that exact visitor transcript, including the retail array rule that
//! omits representation metadata for an empty array and clears flag bit `0x40` for
//! a non-empty one.
//!
//! Every value here is explicit caller-supplied authority.  Bytes and decoded
//! dwords which already have a runtime owner are required to agree; the remainder
//! are only conditionally admitted.  There is still no same-frame join to canonical
//! `Sim` state, so even a complete traversal never issues or installs a checksum.

#![forbid(unsafe_code)]

use crate::leader_initial_prefix::CHECKSUM_LEADER_SLOTS;
use crate::leaders_deferred_history_frontier::{
    RuntimeLeadersDeferredHistoryFrontier, LEADER_DIPLOMACY_END,
};
use crate::leaders_runtime_frontier::{
    LeadersWalkBoundary, LeadersWalkFrontier, RuntimeCoveredRange, LEADER_DATA_ENCRYPT_DWORDS,
    LEADER_FIXED_BODY_BEGIN, LEADER_FIXED_BODY_END, LEADER_PERSONALITY_BEGIN,
    LEADER_PERSONALITY_END,
};

pub const LEADER_WALK_PERSONALITY_CALL_VA: u32 = 0x006d_67c7;
pub const LEADER_WALK_SITES_CALL_VA: u32 = 0x006d_68f7;
pub const LEADER_WALK_MAKE_LIST_CALL_VA: u32 = 0x006d_6904;
pub const LEADER_WALK_PROD_SCRIPT_CALL_VA: u32 = 0x006d_6937;
pub const LEADER_WALK_ENCRYPTED_CALL_VA: u32 = 0x006d_69d6;
pub const ARRAY_SITE_WALK_DATA_VA: u32 = 0x0047_cee0;
pub const ARRAY_MAKE_OBJECT_WALK_DATA_VA: u32 = 0x0047_d440;
pub const SIMPLE_ARRAY_INT_WALK_DATA_VA: u32 = 0x0047_3120;
pub const STRING_WALK_DATA_VA: u32 = 0x00a1_b2d0;
pub const LEADER_DATA_ENCRYPT_WALK_DATA_VA: u32 = 0x006d_9900;

pub const PERSONALITY_DWORDS: usize = 24;
pub const TECH_MASK_BITS: i32 = 806;
pub const TECH_MASK_BYTES: usize = 101;
pub const WONDER_MASK_BITS: i32 = 17;
pub const POWER_MASK_BITS: i32 = 24;
pub const CONQUEST_MASK_BYTES: usize = 3;
pub const RARE_MASK_BITS: i32 = 44;
pub const RARE_MASK_BYTES: usize = 6;
pub const DEFAULT_DYNAMIC_CHILD_WALK_BYTES: usize = 770;

const TECH_BEGIN: usize = 0x6c0c;
const TECH_AT_START_BEGIN: usize = 0x6c80;
const OBS_FLAGS_BEGIN: usize = 0x6cf4;
const CONQUEST_WONDERS_BEGIN: usize = 0x6d68;
const CONQUEST_WONDERS_IN_GAME_BEGIN: usize = 0x6d78;
const CONQUEST_RACIAL_POWERS_BEGIN: usize = 0x6d88;
const RARE_BEGIN: usize = 0x6d98;
const RARE_OWNED_BEGIN: usize = 0x6dac;
const RARE_CONQUEST_BEGIN: usize = 0x6dc0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetailBitMask<const BYTES: usize> {
    pub bits: i32,
    pub size: i32,
    pub payload: [u8; BYTES],
}

impl<const BYTES: usize> RetailBitMask<BYTES> {
    pub const fn new(bits: i32) -> Self {
        Self {
            bits,
            size: BYTES as i32,
            payload: [0; BYTES],
        }
    }
}

/// Current retail `ArrayBaseMaster` history plus the elements visited by its child.
/// `length` is intentionally not inferred from `elements`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetailArray<T> {
    pub length: i32,
    pub capacity: i32,
    pub increment: i16,
    pub flags: u8,
    pub elements: Vec<T>,
}

impl<T> Default for RetailArray<T> {
    fn default() -> Self {
        Self {
            length: 0,
            capacity: 0,
            increment: -1,
            flags: 0,
            elements: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SiteImage {
    pub wx: i32,
    pub wy: i32,
    pub value: i32,
    pub region: i32,
    pub distance: i32,
    pub rank: i32,
}

impl SiteImage {
    fn encode(self, out: &mut Vec<u8>) {
        for value in [
            self.wx,
            self.wy,
            self.value,
            self.region,
            self.distance,
            self.rank,
        ] {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MakeObjectImage {
    pub type_id: i32,
    pub value: i32,
    pub escrow: i32,
    pub city: i32,
    pub upgrade: i32,
    pub object: i32,
    pub number: i32,
    pub category: i32,
    pub wx: i32,
    pub wy: i32,
}

impl MakeObjectImage {
    fn encode(self, out: &mut Vec<u8>) {
        for value in [
            self.type_id,
            self.value,
            self.escrow,
            self.city,
            self.upgrade,
            self.object,
            self.number,
            self.category,
            self.wx,
            self.wy,
        ] {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
}

/// Complete 96-byte PDB layout walked by the raw `Personality` visitor call.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PersonalityImage {
    pub rush: i32,
    pub cities: i32,
    pub upgrades: i32,
    pub arms: i32,
    pub army: i32,
    pub army_size: i32,
    pub raid: i32,
    pub invade: i32,
    pub target: i32,
    pub strategy: i32,
    pub raze: i32,
    pub spells: i32,
    pub forts: i32,
    pub nukes: i32,
    pub air: i32,
    pub naval: i32,
    pub market: i32,
    pub scouts: i32,
    pub civilians: i32,
    pub early_army: i32,
    pub friendly_human: i32,
    pub alliance_human: i32,
    pub friendly_ai: i32,
    pub alliance_ai: i32,
}

impl PersonalityImage {
    pub const fn dwords(self) -> [i32; PERSONALITY_DWORDS] {
        [
            self.rush,
            self.cities,
            self.upgrades,
            self.arms,
            self.army,
            self.army_size,
            self.raid,
            self.invade,
            self.target,
            self.strategy,
            self.raze,
            self.spells,
            self.forts,
            self.nukes,
            self.air,
            self.naval,
            self.market,
            self.scouts,
            self.civilians,
            self.early_army,
            self.friendly_human,
            self.alliance_human,
            self.friendly_ai,
            self.alliance_ai,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicLeaderRowAuthority {
    pub personality: PersonalityImage,
    pub tech: RetailBitMask<TECH_MASK_BYTES>,
    pub tech_at_start: RetailBitMask<TECH_MASK_BYTES>,
    pub obs_flags: RetailBitMask<TECH_MASK_BYTES>,
    pub conquest_wonders: RetailBitMask<CONQUEST_MASK_BYTES>,
    pub conquest_wonders_in_game: RetailBitMask<CONQUEST_MASK_BYTES>,
    pub conquest_racial_powers: RetailBitMask<CONQUEST_MASK_BYTES>,
    pub sites: RetailArray<SiteImage>,
    pub make_list: RetailArray<MakeObjectImage>,
    pub military_trainers: RetailArray<i32>,
    pub new_rares: RetailArray<i32>,
    pub oil_patches: RetailArray<i32>,
    /// Logical UTF-16 code units. Retail walks an `i32` logical length followed by
    /// exactly this payload, not the object metadata or a terminator.
    pub production_script_utf16: Vec<u16>,
    pub rare: RetailBitMask<RARE_MASK_BYTES>,
    pub rare_owned: RetailBitMask<RARE_MASK_BYTES>,
    pub rare_conquest: RetailBitMask<RARE_MASK_BYTES>,
    /// Plaintext visitor order, not the encrypted in-object representation.
    pub economy_plaintext: [i32; LEADER_DATA_ENCRYPT_DWORDS],
}

impl Default for DynamicLeaderRowAuthority {
    fn default() -> Self {
        Self {
            personality: PersonalityImage::default(),
            tech: RetailBitMask::new(TECH_MASK_BITS),
            tech_at_start: RetailBitMask::new(TECH_MASK_BITS),
            obs_flags: RetailBitMask::new(TECH_MASK_BITS),
            conquest_wonders: RetailBitMask::new(WONDER_MASK_BITS),
            conquest_wonders_in_game: RetailBitMask::new(WONDER_MASK_BITS),
            conquest_racial_powers: RetailBitMask::new(POWER_MASK_BITS),
            sites: RetailArray::default(),
            make_list: RetailArray::default(),
            military_trainers: RetailArray::default(),
            new_rares: RetailArray::default(),
            oil_patches: RetailArray::default(),
            production_script_utf16: Vec::new(),
            rare: RetailBitMask::new(RARE_MASK_BITS),
            rare_owned: RetailBitMask::new(RARE_MASK_BITS),
            rare_conquest: RetailBitMask::new(RARE_MASK_BITS),
            economy_plaintext: [0; LEADER_DATA_ENCRYPT_DWORDS],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicLeadersAuthority {
    pub rows: [DynamicLeaderRowAuthority; CHECKSUM_LEADER_SLOTS],
}

impl Default for DynamicLeadersAuthority {
    fn default() -> Self {
        Self {
            rows: std::array::from_fn(|_| DynamicLeaderRowAuthority::default()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DynamicChildRepresentation {
    RawObjectBytes,
    BitMaskHeaderAndPayload,
    ArrayHistoryAndElements,
    Utf16LogicalString,
    DecodedEncryptedDwords,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DynamicChildClaim {
    pub field: &'static str,
    pub transcript_begin: usize,
    pub transcript_end: usize,
    pub representation: DynamicChildRepresentation,
    pub conditionally_admitted_bytes: usize,
    pub duplicate_checked_bytes: usize,
}

impl DynamicChildClaim {
    pub const fn walked_bytes(self) -> usize {
        self.transcript_end - self.transcript_begin
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicChildrenRow {
    pub slot: u8,
    pub active: bool,
    transcript: Vec<u8>,
    claims: Vec<DynamicChildClaim>,
    conditionally_admitted_walked_bytes: usize,
    duplicate_checked_walked_bytes: usize,
}

impl DynamicChildrenRow {
    pub fn claims(&self) -> &[DynamicChildClaim] {
        &self.claims
    }

    pub const fn conditionally_admitted_walked_bytes(&self) -> usize {
        self.conditionally_admitted_walked_bytes
    }

    pub const fn duplicate_checked_walked_bytes(&self) -> usize {
        self.duplicate_checked_walked_bytes
    }

    pub fn walked_bytes(&self) -> &[u8] {
        &self.transcript
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLeadersDynamicChildrenFrontier {
    previous: RuntimeLeadersDeferredHistoryFrontier,
    rows: [DynamicChildrenRow; CHECKSUM_LEADER_SLOTS],
}

impl RuntimeLeadersDynamicChildrenFrontier {
    pub fn previous(&self) -> &RuntimeLeadersDeferredHistoryFrontier {
        &self.previous
    }

    pub fn rows(&self) -> &[DynamicChildrenRow; CHECKSUM_LEADER_SLOTS] {
        &self.rows
    }

    pub fn conditionally_admitted_walked_bytes(&self) -> usize {
        self.rows
            .iter()
            .map(DynamicChildrenRow::conditionally_admitted_walked_bytes)
            .sum()
    }

    pub fn duplicate_checked_walked_bytes(&self) -> usize {
        self.rows
            .iter()
            .map(DynamicChildrenRow::duplicate_checked_walked_bytes)
            .sum()
    }

    pub const fn source_produced_walked_bytes(&self) -> usize {
        0
    }

    pub fn walk_frontier(&self) -> LeadersWalkFrontier {
        let mut checksum = 1u32;
        let mut bytes_walked = 0u64;
        for row in &self.rows {
            checksum = don_sim::checksum::adler32(checksum, &row.transcript);
            bytes_walked += row.transcript.len() as u64;
        }
        LeadersWalkFrontier {
            checksum,
            bytes_walked,
            boundary: LeadersWalkBoundary::Complete,
        }
    }

    /// Traversal completeness is not source authority.  Until a canonical same-frame
    /// `Sim` join exists, callers may inspect the receipt but cannot obtain a checksum.
    pub fn checksum(&self) -> Result<(u32, u64), LeadersWalkFrontier> {
        Err(self.walk_frontier())
    }

    pub const fn installed_in_scoreboard(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DynamicChildrenFrontierError {
    PreviousBoundaryChanged {
        expected: LeadersWalkBoundary,
        got: LeadersWalkBoundary,
    },
    RosterDisagreement {
        slot: usize,
    },
    InvalidBitMask {
        slot: usize,
        field: &'static str,
        expected_bits: i32,
        bits: i32,
        expected_size: usize,
        size: i32,
    },
    NegativeArrayLength {
        slot: usize,
        field: &'static str,
        length: i32,
    },
    ArrayElementCount {
        slot: usize,
        field: &'static str,
        length: usize,
        elements: usize,
    },
    InvalidArrayCapacity {
        slot: usize,
        field: &'static str,
        length: i32,
        capacity: i32,
    },
    StringTooLong {
        slot: usize,
        code_units: usize,
    },
    MissingPreviousRange {
        slot: usize,
        begin: usize,
        end: usize,
    },
    RuntimeDisagreement {
        slot: usize,
        field: &'static str,
        index: usize,
        runtime: u8,
        conditional: u8,
    },
    EconomyDisagreement {
        slot: usize,
        index: usize,
        runtime: i32,
        conditional: i32,
    },
}

impl std::fmt::Display for DynamicChildrenFrontierError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "dynamic Leader child frontier refused: {self:?}")
    }
}

impl std::error::Error for DynamicChildrenFrontierError {}

struct TranscriptBuilder<'a> {
    slot: usize,
    base: &'a crate::leaders_runtime_frontier::RuntimeLeaderRow,
    bytes: Vec<u8>,
    claims: Vec<DynamicChildClaim>,
    conditional: usize,
    duplicate: usize,
}

impl<'a> TranscriptBuilder<'a> {
    fn claim<F>(
        &mut self,
        field: &'static str,
        representation: DynamicChildRepresentation,
        append: F,
    ) -> Result<(), DynamicChildrenFrontierError>
    where
        F: FnOnce(&mut Self) -> Result<(), DynamicChildrenFrontierError>,
    {
        let begin = self.bytes.len();
        let conditional_before = self.conditional;
        let duplicate_before = self.duplicate;
        append(self)?;
        self.claims.push(DynamicChildClaim {
            field,
            transcript_begin: begin,
            transcript_end: self.bytes.len(),
            representation,
            conditionally_admitted_bytes: self.conditional - conditional_before,
            duplicate_checked_bytes: self.duplicate - duplicate_before,
        });
        Ok(())
    }

    fn conditional_bytes(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
        self.conditional += bytes.len();
    }

    fn layout_bytes(
        &mut self,
        field: &'static str,
        begin: usize,
        bytes: &[u8],
    ) -> Result<(), DynamicChildrenFrontierError> {
        for (relative, conditional) in bytes.iter().copied().enumerate() {
            let offset = begin + relative;
            if let Some(runtime) = self.base.owned_slice(RuntimeCoveredRange {
                begin: offset,
                end: offset + 1,
            }) {
                if runtime[0] != conditional {
                    return Err(DynamicChildrenFrontierError::RuntimeDisagreement {
                        slot: self.slot,
                        field,
                        index: relative,
                        runtime: runtime[0],
                        conditional,
                    });
                }
                self.duplicate += 1;
            } else {
                self.conditional += 1;
            }
            self.bytes.push(conditional);
        }
        Ok(())
    }

    fn mask<const N: usize>(
        &mut self,
        field: &'static str,
        layout_begin: usize,
        expected_bits: i32,
        mask: &RetailBitMask<N>,
    ) -> Result<(), DynamicChildrenFrontierError> {
        if mask.bits != expected_bits || mask.size != N as i32 {
            return Err(DynamicChildrenFrontierError::InvalidBitMask {
                slot: self.slot,
                field,
                expected_bits,
                bits: mask.bits,
                expected_size: N,
                size: mask.size,
            });
        }
        self.layout_bytes(field, layout_begin, &mask.bits.to_le_bytes())?;
        self.layout_bytes(field, layout_begin + 4, &mask.size.to_le_bytes())?;
        // `BitMask::flags` at +8 is object metadata and is not visited.
        self.layout_bytes(field, layout_begin + 0x0c, &mask.payload)
    }

    fn array<T, F>(
        &mut self,
        field: &'static str,
        array: &RetailArray<T>,
        mut encode: F,
    ) -> Result<(), DynamicChildrenFrontierError>
    where
        F: FnMut(&T, &mut Vec<u8>),
    {
        if array.length < 0 {
            return Err(DynamicChildrenFrontierError::NegativeArrayLength {
                slot: self.slot,
                field,
                length: array.length,
            });
        }
        let length = array.length as usize;
        if length != array.elements.len() {
            return Err(DynamicChildrenFrontierError::ArrayElementCount {
                slot: self.slot,
                field,
                length,
                elements: array.elements.len(),
            });
        }
        self.conditional_bytes(&array.length.to_le_bytes());
        if length == 0 {
            return Ok(());
        }
        if array.capacity < array.length {
            return Err(DynamicChildrenFrontierError::InvalidArrayCapacity {
                slot: self.slot,
                field,
                length: array.length,
                capacity: array.capacity,
            });
        }
        self.conditional_bytes(&array.capacity.to_le_bytes());
        self.conditional_bytes(&array.increment.to_le_bytes());
        self.conditional_bytes(&[array.flags & 0xbf]);
        for element in &array.elements {
            let begin = self.bytes.len();
            encode(element, &mut self.bytes);
            self.conditional += self.bytes.len() - begin;
        }
        Ok(())
    }

    fn economy(
        &mut self,
        plaintext: &[i32; LEADER_DATA_ENCRYPT_DWORDS],
    ) -> Result<(), DynamicChildrenFrontierError> {
        for (index, conditional) in plaintext.iter().copied().enumerate() {
            if let Some(runtime) = self.base.econ_plaintext()[index] {
                if runtime != conditional {
                    return Err(DynamicChildrenFrontierError::EconomyDisagreement {
                        slot: self.slot,
                        index,
                        runtime,
                        conditional,
                    });
                }
                self.duplicate += 4;
            } else {
                self.conditional += 4;
            }
            self.bytes.extend_from_slice(&conditional.to_le_bytes());
        }
        Ok(())
    }
}

/// Bind every child after the established `Personality +0x6dd4` boundary.  Success
/// means that the retail traversal is structurally complete, not that its values are
/// source-produced by the simulation.
pub fn bind_dynamic_children_frontier(
    previous: RuntimeLeadersDeferredHistoryFrontier,
    authority: &DynamicLeadersAuthority,
) -> Result<RuntimeLeadersDynamicChildrenFrontier, DynamicChildrenFrontierError> {
    let first_active = previous.rows().iter().position(|row| row.active);
    let expected = first_active.map_or(LeadersWalkBoundary::Complete, |slot| {
        LeadersWalkBoundary::DynamicChildren {
            slot,
            child: "Personality at +0x6dd4",
        }
    });
    let got = previous.walk_frontier().boundary;
    if got != expected {
        return Err(DynamicChildrenFrontierError::PreviousBoundaryChanged { expected, got });
    }

    let base = previous.previous().previous().base();
    let mut rows = Vec::with_capacity(CHECKSUM_LEADER_SLOTS);
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let prior_row = &previous.rows()[slot];
        let base_row = &base.rows[slot];
        if prior_row.slot != slot as u8
            || base_row.slot != slot as u8
            || prior_row.active != base_row.active
        {
            return Err(DynamicChildrenFrontierError::RosterDisagreement { slot });
        }

        let mut transcript = Vec::new();
        let prefix_end = if prior_row.active {
            LEADER_FIXED_BODY_END
        } else {
            LEADER_FIXED_BODY_BEGIN
        };
        let prefix = prior_row
            .conditionally_admitted_slice(RuntimeCoveredRange {
                begin: 0,
                end: prefix_end,
            })
            .ok_or(DynamicChildrenFrontierError::MissingPreviousRange {
                slot,
                begin: 0,
                end: prefix_end,
            })?;
        transcript.extend_from_slice(prefix);

        if !prior_row.active {
            rows.push(DynamicChildrenRow {
                slot: slot as u8,
                active: false,
                transcript,
                claims: Vec::new(),
                conditionally_admitted_walked_bytes: 0,
                duplicate_checked_walked_bytes: 0,
            });
            continue;
        }

        let diplomacy = prior_row
            .conditionally_admitted_slice(RuntimeCoveredRange {
                begin: crate::leaders_runtime_frontier::LEADER_DIPLOMACY_BEGIN,
                end: LEADER_DIPLOMACY_END,
            })
            .ok_or(DynamicChildrenFrontierError::MissingPreviousRange {
                slot,
                begin: crate::leaders_runtime_frontier::LEADER_DIPLOMACY_BEGIN,
                end: LEADER_DIPLOMACY_END,
            })?;
        transcript.extend_from_slice(diplomacy);

        let row = &authority.rows[slot];
        let mut builder = TranscriptBuilder {
            slot,
            base: base_row,
            bytes: transcript,
            claims: Vec::new(),
            conditional: 0,
            duplicate: 0,
        };

        builder.claim(
            "Personality",
            DynamicChildRepresentation::RawObjectBytes,
            |builder| {
                for (index, value) in row.personality.dwords().iter().enumerate() {
                    builder.layout_bytes(
                        "Personality",
                        LEADER_PERSONALITY_BEGIN + index * 4,
                        &value.to_le_bytes(),
                    )?;
                }
                Ok(())
            },
        )?;
        debug_assert_eq!(LEADER_PERSONALITY_END - LEADER_PERSONALITY_BEGIN, 96);

        for (field, begin, bits, mask) in [
            ("tech", TECH_BEGIN, TECH_MASK_BITS, &row.tech),
            (
                "tech_at_start",
                TECH_AT_START_BEGIN,
                TECH_MASK_BITS,
                &row.tech_at_start,
            ),
            ("obs_flags", OBS_FLAGS_BEGIN, TECH_MASK_BITS, &row.obs_flags),
        ] {
            builder.claim(
                field,
                DynamicChildRepresentation::BitMaskHeaderAndPayload,
                |builder| builder.mask(field, begin, bits, mask),
            )?;
        }
        for (field, begin, bits, mask) in [
            (
                "conquest_wonders",
                CONQUEST_WONDERS_BEGIN,
                WONDER_MASK_BITS,
                &row.conquest_wonders,
            ),
            (
                "conquest_wonders_in_game",
                CONQUEST_WONDERS_IN_GAME_BEGIN,
                WONDER_MASK_BITS,
                &row.conquest_wonders_in_game,
            ),
            (
                "conquest_racial_powers",
                CONQUEST_RACIAL_POWERS_BEGIN,
                POWER_MASK_BITS,
                &row.conquest_racial_powers,
            ),
        ] {
            builder.claim(
                field,
                DynamicChildRepresentation::BitMaskHeaderAndPayload,
                |builder| builder.mask(field, begin, bits, mask),
            )?;
        }

        builder.claim(
            "Sites",
            DynamicChildRepresentation::ArrayHistoryAndElements,
            |builder| builder.array("Sites", &row.sites, |site, out| (*site).encode(out)),
        )?;
        builder.claim(
            "MakeList",
            DynamicChildRepresentation::ArrayHistoryAndElements,
            |builder| {
                builder.array("MakeList", &row.make_list, |object, out| {
                    (*object).encode(out)
                })
            },
        )?;
        for (field, array) in [
            ("military_trainers", &row.military_trainers),
            ("new_rares", &row.new_rares),
            ("oil_patches", &row.oil_patches),
        ] {
            builder.claim(
                field,
                DynamicChildRepresentation::ArrayHistoryAndElements,
                |builder| {
                    builder.array(field, array, |value, out| {
                        out.extend_from_slice(&value.to_le_bytes())
                    })
                },
            )?;
        }

        builder.claim(
            "production_script",
            DynamicChildRepresentation::Utf16LogicalString,
            |builder| {
                let length = row.production_script_utf16.len();
                if length > u16::MAX as usize {
                    return Err(DynamicChildrenFrontierError::StringTooLong {
                        slot,
                        code_units: length,
                    });
                }
                builder.conditional_bytes(&(length as i32).to_le_bytes());
                for unit in &row.production_script_utf16 {
                    builder.conditional_bytes(&unit.to_le_bytes());
                }
                Ok(())
            },
        )?;

        for (field, begin, mask) in [
            ("rare", RARE_BEGIN, &row.rare),
            ("rare_owned", RARE_OWNED_BEGIN, &row.rare_owned),
            ("rare_conquest", RARE_CONQUEST_BEGIN, &row.rare_conquest),
        ] {
            builder.claim(
                field,
                DynamicChildRepresentation::BitMaskHeaderAndPayload,
                |builder| builder.mask(field, begin, RARE_MASK_BITS, mask),
            )?;
        }
        builder.claim(
            "LeaderDataEncrypt plaintext",
            DynamicChildRepresentation::DecodedEncryptedDwords,
            |builder| builder.economy(&row.economy_plaintext),
        )?;

        rows.push(DynamicChildrenRow {
            slot: slot as u8,
            active: true,
            transcript: builder.bytes,
            claims: builder.claims,
            conditionally_admitted_walked_bytes: builder.conditional,
            duplicate_checked_walked_bytes: builder.duplicate,
        });
    }

    Ok(RuntimeLeadersDynamicChildrenFrontier {
        previous,
        rows: rows.try_into().expect("exact eight-row projection"),
    })
}
