// SPDX-License-Identifier: GPL-3.0-or-later
//! Object-lifetime-invariant Leader BitMask visitor headers.
//!
//! Retail walks only `{bits:i32,size:i32}` before each payload; the flags byte is object
//! metadata and is not visited. These seven fixed-size masks retain their constructor shape
//! for the entire Leader lifetime. Payload mutation and `Leader::close` never alter it.

#![forbid(unsafe_code)]

use crate::leader_initial_prefix::CHECKSUM_LEADER_SLOTS;
use crate::leaders_dynamic_children_frontier::{
    CONQUEST_MASK_BYTES, POWER_MASK_BITS, RARE_MASK_BITS, RARE_MASK_BYTES, TECH_MASK_BITS,
    TECH_MASK_BYTES, WONDER_MASK_BITS,
};
use crate::leaders_runtime_frontier::LeadersWalkFrontier;
use crate::leaders_setup_rare_history_frontier::RuntimeLeadersFrameZeroRareHistoryFrontier;

pub const MASK_HEADER_WALKED_BYTES: usize = 8;
pub const LIFETIME_MASK_HEADERS: usize = 7;
pub const LIFETIME_MASK_HEADER_WALKED_BYTES: usize =
    MASK_HEADER_WALKED_BYTES * LIFETIME_MASK_HEADERS;

pub const TECH_AT_START_BITS_STORE_VA: u32 = 0x006d_75b5;
pub const TECH_AT_START_SIZE_STORE_VA: u32 = 0x006d_75bf;
pub const CONQUEST_WONDERS_BITS_STORE_VA: u32 = 0x006d_7608;
pub const CONQUEST_WONDERS_SIZE_STORE_VA: u32 = 0x006d_7615;
pub const CONQUEST_WONDERS_IN_GAME_BITS_STORE_VA: u32 = 0x006d_7636;
pub const CONQUEST_WONDERS_IN_GAME_SIZE_STORE_VA: u32 = 0x006d_7640;
pub const CONQUEST_RACIAL_POWERS_BITS_STORE_VA: u32 = 0x006d_7661;
pub const CONQUEST_RACIAL_POWERS_SIZE_STORE_VA: u32 = 0x006d_766b;
pub const RARE_BITS_STORE_VA: u32 = 0x006d_768c;
pub const RARE_SIZE_STORE_VA: u32 = 0x006d_7696;
pub const RARE_OWNED_BITS_STORE_VA: u32 = 0x006d_76b7;
pub const RARE_OWNED_SIZE_STORE_VA: u32 = 0x006d_76c1;
pub const RARE_CONQUEST_BITS_STORE_VA: u32 = 0x006d_76e2;
pub const RARE_CONQUEST_SIZE_STORE_VA: u32 = 0x006d_76ec;
pub const LEADER_CLOSE_MASK_PAYLOAD_CLEAR_BEGIN_VA: u32 = 0x006b_802e;
pub const LEADER_CLOSE_MASK_PAYLOAD_CLEAR_END_VA: u32 = 0x006b_80a9;

const MASKS: &[(&str, i32, usize)] = &[
    ("tech_at_start", TECH_MASK_BITS, TECH_MASK_BYTES),
    ("conquest_wonders", WONDER_MASK_BITS, CONQUEST_MASK_BYTES),
    (
        "conquest_wonders_in_game",
        WONDER_MASK_BITS,
        CONQUEST_MASK_BYTES,
    ),
    (
        "conquest_racial_powers",
        POWER_MASK_BITS,
        CONQUEST_MASK_BYTES,
    ),
    ("rare", RARE_MASK_BITS, RARE_MASK_BYTES),
    ("rare_owned", RARE_MASK_BITS, RARE_MASK_BYTES),
    ("rare_conquest", RARE_MASK_BITS, RARE_MASK_BYTES),
];

const _: () = assert!(MASKS.len() == LIFETIME_MASK_HEADERS);
const _: () = assert!(LIFETIME_MASK_HEADER_WALKED_BYTES == 56);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifetimeMaskHeaderSource {
    FixedSizeBitMaskConstructor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LifetimeMaskHeaderClaim {
    pub slot: u8,
    pub masks: usize,
    pub newly_canonical_walked_bytes: usize,
    pub source: LifetimeMaskHeaderSource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeLeadersLifetimeMaskHeaderFrontier {
    inner: Box<RuntimeLeadersFrameZeroRareHistoryFrontier>,
    claims: Vec<LifetimeMaskHeaderClaim>,
}

impl RuntimeLeadersLifetimeMaskHeaderFrontier {
    pub fn inner(&self) -> &RuntimeLeadersFrameZeroRareHistoryFrontier {
        self.inner.as_ref()
    }

    pub fn claims(&self) -> &[LifetimeMaskHeaderClaim] {
        &self.claims
    }

    pub fn newly_canonicalized_walked_bytes(&self) -> usize {
        self.claims.len() * LIFETIME_MASK_HEADER_WALKED_BYTES
    }

    pub fn unique_canonical_walked_bytes(&self) -> usize {
        self.inner.unique_canonical_walked_bytes() + self.newly_canonicalized_walked_bytes()
    }

    pub fn remaining_unsourced_walked_bytes(&self) -> u64 {
        self.walk_frontier()
            .bytes_walked
            .saturating_sub(self.unique_canonical_walked_bytes() as u64)
    }

    pub fn walk_frontier(&self) -> LeadersWalkFrontier {
        self.inner.walk_frontier()
    }

    pub fn checksum(&self) -> Result<(u32, u64), LeadersWalkFrontier> {
        Err(self.walk_frontier())
    }

    /// The header values themselves survive for the Leader object's lifetime. The composed
    /// receipt remains frame-zero-only because its inner authorities expire.
    pub const fn header_lifetime_stable(&self) -> bool {
        true
    }

    pub const fn installed_in_scoreboard(&self) -> bool {
        false
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LifetimeMaskHeaderError {
    MissingField {
        slot: usize,
        field: &'static str,
    },
    HeaderDisagreement {
        slot: usize,
        field: &'static str,
        byte: usize,
        expected: u8,
        conditional: u8,
    },
}

impl std::fmt::Display for LifetimeMaskHeaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Leader lifetime mask header refused: {self:?}")
    }
}

impl std::error::Error for LifetimeMaskHeaderError {}

pub fn bind_lifetime_mask_headers(
    inner: RuntimeLeadersFrameZeroRareHistoryFrontier,
) -> Result<RuntimeLeadersLifetimeMaskHeaderFrontier, LifetimeMaskHeaderError> {
    let mut claims = Vec::new();
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let active = inner
            .inner()
            .inner()
            .conditional_row_active(slot)
            .expect("the complete Leader transcript has ten rows");
        if !active {
            continue;
        }
        for &(field, bits, size) in MASKS {
            let conditional = inner
                .inner()
                .inner()
                .conditional_dynamic_field(slot, field)
                .ok_or(LifetimeMaskHeaderError::MissingField { slot, field })?;
            let expected = [bits.to_le_bytes(), (size as i32).to_le_bytes()].concat();
            if let Some(byte) = conditional[..MASK_HEADER_WALKED_BYTES]
                .iter()
                .zip(&expected)
                .position(|(conditional, expected)| conditional != expected)
            {
                return Err(LifetimeMaskHeaderError::HeaderDisagreement {
                    slot,
                    field,
                    byte,
                    expected: expected[byte],
                    conditional: conditional[byte],
                });
            }
        }
        claims.push(LifetimeMaskHeaderClaim {
            slot: slot as u8,
            masks: LIFETIME_MASK_HEADERS,
            newly_canonical_walked_bytes: LIFETIME_MASK_HEADER_WALKED_BYTES,
            source: LifetimeMaskHeaderSource::FixedSizeBitMaskConstructor,
        });
    }
    Ok(RuntimeLeadersLifetimeMaskHeaderFrontier {
        inner: Box::new(inner),
        claims,
    })
}
