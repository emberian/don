// SPDX-License-Identifier: GPL-3.0-or-later

//! Canonical conquest-power owner and complete retail `LeaderData` setup queries.
//!
//! `LeaderData::has_tribe_bonus` `0x006E1370` reads the three-byte payload of
//! `BitMask<24> conquest_racial_powers` at `LeaderData +0x6D94`. The BitMask header and
//! pointer are allocator history, not semantic state; [`CanonicalConquestRacialPowers`]
//! owns exactly the walked 24-bit payload. The same semantic owner is currently exposed to
//! unit-stat consumers as a decoded bit word, so a future save mount must project this row
//! into that owner rather than introduce a second mutable bonus cache.
//!
//! This module also closes `LeaderData::get_diff` `0x006EC000`, the difficulty child used
//! by `production_ai_setup`. Its `multi_diff` field already has a canonical save owner, so
//! the child adds executable behavior without another save extension.

use std::fmt;

pub const LEADER_COUNT: usize = 8;
pub const TRIBE_BONUS_COUNT: usize = 24;
pub const CONQUEST_POWER_BYTES_PER_LEADER: usize = 3;
pub const CONQUEST_POWER_TABLE_BYTES: usize = LEADER_COUNT * CONQUEST_POWER_BYTES_PER_LEADER;
pub const HAS_TRIBE_BONUS_VA: u32 = 0x006e_1370;
pub const GET_DIFF_VA: u32 = 0x006e_c000;

/// Canonical payload of retail `BitMask<24> conquest_racial_powers`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CanonicalConquestRacialPowers {
    payload: [u8; CONQUEST_POWER_BYTES_PER_LEADER],
}

impl CanonicalConquestRacialPowers {
    pub const fn from_payload(payload: [u8; CONQUEST_POWER_BYTES_PER_LEADER]) -> Self {
        Self { payload }
    }

    pub const fn payload(self) -> [u8; CONQUEST_POWER_BYTES_PER_LEADER] {
        self.payload
    }

    /// Project the exact 24 payload bits into the existing decoded leader-stat word.
    pub const fn decoded_mask(self) -> u32 {
        u32::from_le_bytes([self.payload[0], self.payload[1], self.payload[2], 0])
    }

    pub const fn from_decoded_mask(mask: u32) -> Result<Self, ConquestPowerCodecError> {
        if mask >> TRIBE_BONUS_COUNT != 0 {
            return Err(ConquestPowerCodecError::BitsOutsideRetailRange(mask));
        }
        let bytes = mask.to_le_bytes();
        Ok(Self {
            payload: [bytes[0], bytes[1], bytes[2]],
        })
    }

    pub const fn has(self, bonus: usize) -> Option<bool> {
        if bonus >= TRIBE_BONUS_COUNT {
            return None;
        }
        Some(self.payload[bonus >> 3] & (1 << (bonus & 7)) != 0)
    }

    pub fn set(&mut self, bonus: usize, present: bool) -> bool {
        if bonus >= TRIBE_BONUS_COUNT {
            return false;
        }
        let mask = 1 << (bonus & 7);
        if present {
            self.payload[bonus >> 3] |= mask;
        } else {
            self.payload[bonus >> 3] &= !mask;
        }
        true
    }

    /// Proposed Leader-row extension: the three semantic bytes, without BitMask metadata.
    pub const fn save_extension_bytes(self) -> [u8; CONQUEST_POWER_BYTES_PER_LEADER] {
        self.payload
    }

    pub fn from_save_extension_bytes(bytes: &[u8]) -> Result<Self, ConquestPowerCodecError> {
        let payload: [u8; CONQUEST_POWER_BYTES_PER_LEADER] =
            bytes
                .try_into()
                .map_err(|_| ConquestPowerCodecError::Length {
                    expected: CONQUEST_POWER_BYTES_PER_LEADER,
                    actual: bytes.len(),
                })?;
        Ok(Self { payload })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConquestPowerCodecError {
    Length { expected: usize, actual: usize },
    BitsOutsideRetailRange(u32),
}

impl fmt::Display for ConquestPowerCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length { expected, actual } => write!(
                f,
                "conquest-racial-powers row is {actual} bytes; expected {expected}"
            ),
            Self::BitsOutsideRetailRange(mask) => write!(
                f,
                "decoded conquest-racial-powers mask {mask:#010x} has bits above retail bit 23"
            ),
        }
    }
}

impl std::error::Error for ConquestPowerCodecError {}

pub fn table_extension_bytes(
    rows: &[CanonicalConquestRacialPowers; LEADER_COUNT],
) -> [u8; CONQUEST_POWER_TABLE_BYTES] {
    let mut bytes = [0; CONQUEST_POWER_TABLE_BYTES];
    for (slot, row) in rows.iter().copied().enumerate() {
        let start = slot * CONQUEST_POWER_BYTES_PER_LEADER;
        bytes[start..start + CONQUEST_POWER_BYTES_PER_LEADER]
            .copy_from_slice(&row.save_extension_bytes());
    }
    bytes
}

pub fn table_from_extension_bytes(
    bytes: &[u8],
) -> Result<[CanonicalConquestRacialPowers; LEADER_COUNT], ConquestPowerCodecError> {
    if bytes.len() != CONQUEST_POWER_TABLE_BYTES {
        return Err(ConquestPowerCodecError::Length {
            expected: CONQUEST_POWER_TABLE_BYTES,
            actual: bytes.len(),
        });
    }
    let mut rows = [CanonicalConquestRacialPowers::default(); LEADER_COUNT];
    for (slot, row) in rows.iter_mut().enumerate() {
        let start = slot * CONQUEST_POWER_BYTES_PER_LEADER;
        *row = CanonicalConquestRacialPowers::from_save_extension_bytes(
            &bytes[start..start + CONQUEST_POWER_BYTES_PER_LEADER],
        )?;
    }
    Ok(rows)
}

/// Complete external and canonical facts read by `LeaderData::has_tribe_bonus`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TribeBonusInputs {
    /// Global no-nation-powers bit (`Game +0x20`, bit 2).
    pub no_nation_powers: bool,
    /// `GameInfo +0x2C`; zero enables the pre-first-city early return.
    pub victory: u8,
    /// `LeaderData +0x3F8`.
    pub city_num: i32,
    /// `LeaderData +0x0C`; negative means that no Tribe row is attached.
    pub tribe: i32,
    /// Low byte of `LeaderData +0x04`; bit `0x40` suppresses the Tribe fallback.
    pub leader_flags2: u32,
    pub conquest_racial_powers: CanonicalConquestRacialPowers,
    /// Resolved PDB field `Tribe +0x54` for `tribe`. Needed only on the final fallback.
    pub tribe_default_bonus: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TribeBonusExit {
    NationPowersDisabled,
    NoCityBeforeVictory,
    NoTribe,
    GrantedByConquest,
    TribeFallbackSuppressed,
    GrantedByTribe,
    NotGranted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TribeBonusReceipt {
    pub exit: TribeBonusExit,
    pub read_victory: bool,
    pub read_city_num: bool,
    pub read_tribe: bool,
    pub read_conquest_payload: bool,
    pub read_leader_flags2: bool,
    pub read_tribe_default: bool,
}

impl TribeBonusReceipt {
    const fn at(exit: TribeBonusExit) -> Self {
        Self {
            exit,
            read_victory: false,
            read_city_num: false,
            read_tribe: false,
            read_conquest_payload: false,
            read_leader_flags2: false,
            read_tribe_default: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TribeBonusInputError {
    InvalidBonus(i32),
    MissingTribeDefault { tribe: i32 },
}

impl fmt::Display for TribeBonusInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBonus(bonus) => {
                write!(f, "tribe bonus {bonus} is outside retail range 0..24")
            }
            Self::MissingTribeDefault { tribe } => {
                write!(f, "no Tribe +0x54 value was supplied for tribe {tribe}")
            }
        }
    }
}

impl std::error::Error for TribeBonusInputError {}

/// Complete 133-byte `LeaderData::has_tribe_bonus` `0x006E1370`.
///
/// Detached invalid indices and a missing resolved Tribe row are refused rather than turning
/// retail's unchecked pointer arithmetic into a host out-of-bounds read.
pub fn has_tribe_bonus(
    inputs: TribeBonusInputs,
    bonus: i32,
) -> Result<(bool, TribeBonusReceipt), TribeBonusInputError> {
    let bonus_index = usize::try_from(bonus)
        .ok()
        .filter(|&index| index < TRIBE_BONUS_COUNT)
        .ok_or(TribeBonusInputError::InvalidBonus(bonus))?;

    let mut receipt = TribeBonusReceipt::at(TribeBonusExit::NationPowersDisabled);
    if inputs.no_nation_powers {
        return Ok((false, receipt));
    }

    receipt.read_victory = true;
    if inputs.victory == 0 {
        receipt.read_city_num = true;
        if inputs.city_num == 0 {
            receipt.exit = TribeBonusExit::NoCityBeforeVictory;
            return Ok((false, receipt));
        }
    }

    receipt.read_tribe = true;
    if inputs.tribe < 0 {
        receipt.exit = TribeBonusExit::NoTribe;
        return Ok((false, receipt));
    }

    receipt.read_conquest_payload = true;
    if inputs
        .conquest_racial_powers
        .has(bonus_index)
        .expect("validated retail bonus index")
    {
        receipt.exit = TribeBonusExit::GrantedByConquest;
        return Ok((true, receipt));
    }

    receipt.read_leader_flags2 = true;
    if inputs.leader_flags2 & 0x40 != 0 {
        receipt.exit = TribeBonusExit::TribeFallbackSuppressed;
        return Ok((false, receipt));
    }

    receipt.read_tribe_default = true;
    let default = inputs
        .tribe_default_bonus
        .ok_or(TribeBonusInputError::MissingTribeDefault {
            tribe: inputs.tribe,
        })?;
    let granted = default == bonus;
    receipt.exit = if granted {
        TribeBonusExit::GrantedByTribe
    } else {
        TribeBonusExit::NotGranted
    };
    Ok((granted, receipt))
}

/// All bytes and fields read by `LeaderData::get_diff`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GetDiffInputs {
    pub match_flags_820: u8,
    pub match_flags_821: u8,
    pub match_flags_822: u8,
    pub global_difficulty: u8,
    pub multi_diff: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GetDiffExit {
    ForcedLeaderDifficulty,
    GlobalFlagFallback,
    NegativeLeaderFallback,
    LeaderDifficulty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GetDiffReceipt {
    pub exit: GetDiffExit,
    pub read_multi_diff: bool,
}

/// Complete 58-byte `LeaderData::get_diff` `0x006EC000`.
///
/// The return is `u32` because retail returns the raw dword on the forced per-leader arm;
/// a negative `multi_diff` therefore remains its two's-complement bit pattern on that path.
pub const fn get_diff(inputs: GetDiffInputs) -> (u32, GetDiffReceipt) {
    if inputs.match_flags_820 & 4 != 0 {
        return (
            inputs.multi_diff as u32,
            GetDiffReceipt {
                exit: GetDiffExit::ForcedLeaderDifficulty,
                read_multi_diff: true,
            },
        );
    }

    if inputs.match_flags_821 & 0x10 == 0 || inputs.match_flags_822 & 2 != 0 {
        return (
            inputs.global_difficulty as u32,
            GetDiffReceipt {
                exit: GetDiffExit::GlobalFlagFallback,
                read_multi_diff: false,
            },
        );
    }

    if inputs.multi_diff < 0 {
        return (
            inputs.global_difficulty as u32,
            GetDiffReceipt {
                exit: GetDiffExit::NegativeLeaderFallback,
                read_multi_diff: true,
            },
        );
    }

    (
        inputs.multi_diff as u32,
        GetDiffReceipt {
            exit: GetDiffExit::LeaderDifficulty,
            read_multi_diff: true,
        },
    )
}
