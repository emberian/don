// SPDX-License-Identifier: GPL-3.0-or-later

//! Exact retail economy owner and executable frontier for `Leader::production_ai_setup`.
//!
//! `LeaderDataEncrypt::rate[6]` is a distinct encrypted block at `+0xAC`; it is not the
//! displayed `income[6]` block at `+0x94` or `LeaderData::base_rate[6]` at `+0x4B0`. Sim owns
//! the decoded values in [`CanonicalPlanningRates`] and only applies the retail XOR at a live
//! process boundary.
//!
//! This module closes the complete PE child `LeaderData::get_mod_resource_cap` `0x006D65B0`
//! and the rate/classification cohort at `production_ai_setup` `0x006C8676..0x006C8778`. It
//! deliberately stops before the second six-resource flag loop and the
//! `Leader::market_speculation` tail. The earlier bucket-adjustment prelude is also outside
//! this cohort; none of its market state is synthesized here.

use std::fmt;

pub const RESOURCE_COUNT: usize = 6;
pub const GET_MOD_RESOURCE_CAP_VA: u32 = 0x006d_65b0;
pub const PRODUCTION_AI_SETUP_VA: u32 = 0x006c_83e0;
pub const PRODUCTION_AI_RATE_COHORT_VA: u32 = 0x006c_8676;
pub const MARKET_SPECULATION_VA: u32 = 0x006c_8110;

pub const RESOURCE_CAP_XOR: u32 = 0x0000_1281;
pub const INCOME_XOR: u32 = 0x0009_0236;
pub const PLANNING_RATE_XOR: u32 = 0x0007_3862;

/// DoNSave row projection: six decoded little-endian i32 values, in retail resource order.
pub const PLANNING_RATE_SAVE_VALUES: usize = RESOURCE_COUNT;
pub const PLANNING_RATE_SAVE_BYTES: usize = PLANNING_RATE_SAVE_VALUES * 4;

/// Canonical decoded owner of retail `LeaderDataEncrypt::rate[6]`.
///
/// Human leaders may leave this cache stale. That is retail state and must be preserved; the
/// owner therefore has no invented freshness bit and never aliases the displayed income row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CanonicalPlanningRates {
    pub values: [i32; RESOURCE_COUNT],
}

impl CanonicalPlanningRates {
    /// Decode the six XOR-obfuscated values read from a retail process.
    pub fn from_retail_encrypted(encrypted: [u32; RESOURCE_COUNT]) -> Self {
        Self {
            values: encrypted.map(decode_planning_rate),
        }
    }

    /// Project the canonical decoded owner back to a retail process image.
    pub fn to_retail_encrypted(self) -> [u32; RESOURCE_COUNT] {
        self.values.map(encode_planning_rate)
    }

    /// Bytes appended to an existing canonical Leader row. Engine XOR keys are intentionally
    /// absent from DoNSave; canonical saves carry decoded integer state.
    pub fn save_extension_bytes(self) -> [u8; PLANNING_RATE_SAVE_BYTES] {
        let mut bytes = [0; PLANNING_RATE_SAVE_BYTES];
        for (index, value) in self.values.into_iter().enumerate() {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    pub fn from_save_extension_bytes(bytes: &[u8]) -> Result<Self, PlanningRateCodecError> {
        if bytes.len() != PLANNING_RATE_SAVE_BYTES {
            return Err(PlanningRateCodecError::Length {
                expected: PLANNING_RATE_SAVE_BYTES,
                actual: bytes.len(),
            });
        }

        let mut values = [0; RESOURCE_COUNT];
        for (index, value) in values.iter_mut().enumerate() {
            *value = i32::from_le_bytes(
                bytes[index * 4..index * 4 + 4]
                    .try_into()
                    .expect("planning-rate extension length checked"),
            );
        }
        Ok(Self { values })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanningRateCodecError {
    Length { expected: usize, actual: usize },
}

impl fmt::Display for PlanningRateCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length { expected, actual } => {
                write!(
                    f,
                    "planning-rate row is {actual} bytes; expected {expected}"
                )
            }
        }
    }
}

impl std::error::Error for PlanningRateCodecError {}

#[inline]
pub const fn decode_resource_cap(encrypted: u32) -> i32 {
    (encrypted ^ RESOURCE_CAP_XOR) as i32
}

#[inline]
pub const fn encode_resource_cap(decoded: i32) -> u32 {
    decoded as u32 ^ RESOURCE_CAP_XOR
}

#[inline]
pub const fn decode_income(encrypted: u32) -> i32 {
    (encrypted ^ INCOME_XOR) as i32
}

#[inline]
pub const fn encode_income(decoded: i32) -> u32 {
    decoded as u32 ^ INCOME_XOR
}

#[inline]
pub const fn decode_planning_rate(encrypted: u32) -> i32 {
    (encrypted ^ PLANNING_RATE_XOR) as i32
}

#[inline]
pub const fn encode_planning_rate(decoded: i32) -> u32 {
    decoded as u32 ^ PLANNING_RATE_XOR
}

/// Complete non-array inputs read by `LeaderData::get_mod_resource_cap` `0x006D65B0`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModResourceCapInputs {
    /// Retail `starting_resources`; value 8 is the scenario/sandbox early return.
    pub starting_resources: u8,
    /// Byte at the shared game row `+0x820`; bit 2 bypasses difficulty scaling.
    pub match_flags_820: u8,
    /// First dword in `LeaderData`; bit 2 bypasses difficulty scaling.
    pub leader_flags: u32,
    /// Exact result of `LeaderData::get_diff` `0x006EC000` when the scaling gate is open.
    pub difficulty: i32,
}

/// Complete `LeaderData::get_mod_resource_cap` `0x006D65B0`.
///
/// Retail converts the decoded i32 to binary32 and back even on the nominal `* 1.0` path.
/// Keeping that round trip preserves its precision loss and x86 invalid-conversion result.
pub fn get_mod_resource_cap(inputs: ModResourceCapInputs, encrypted_cap: u32) -> i32 {
    if inputs.starting_resources == 8 {
        return 0;
    }

    let cap = decode_resource_cap(encrypted_cap) as f32;
    if inputs.match_flags_820 & 4 == 0 && inputs.leader_flags & 4 == 0 {
        if inputs.difficulty == 0 {
            return cvttss2si(cap * 0.5);
        }
        if inputs.difficulty == 1 {
            return cvttss2si(cap * 0.75);
        }
    }
    cvttss2si(cap * 1.0)
}

/// Canonical fields written by the bounded setup rate cohort.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProductionAiRateState {
    pub econ_flags: [u32; RESOURCE_COUNT],
    pub base_rate: [i32; RESOURCE_COUNT],
    pub planning_rates: CanonicalPlanningRates,
    pub worst_good: i32,
    pub best_good: i32,
    pub shortages: i32,
}

/// Complete inputs to the bounded rate cohort after the setup bucket-adjustment prelude.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProductionAiRateInputs {
    pub cap: ModResourceCapInputs,
    pub encrypted_resource_caps: [u32; RESOURCE_COUNT],
    pub encrypted_income: [u32; RESOURCE_COUNT],
    /// Exact answers from `ResourceType::type_avail(resource, 1)` `0x006E33A0`.
    pub type_available: [bool; RESOURCE_COUNT],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProductionAiRateReceipt {
    pub returned_for_starting_resources: bool,
    /// Which `rate` elements retail wrote. Zero for the starting-resources-8 early return.
    pub written_mask: u8,
    pub modified_caps: [i32; RESOURCE_COUNT],
    pub decoded_income: [i32; RESOURCE_COUNT],
    pub capped_income: [i32; RESOURCE_COUNT],
    pub encrypted_rate_writes: [u32; RESOURCE_COUNT],
}

/// Execute only the owner-complete setup cohort described by this module.
///
/// The entry reset and the `starting_resources == 8` return are included because they decide
/// whether `rate[6]` is overwritten. On the normal branch the function starts at retail label
/// `0x006C8676`; it neither changes stockpiles nor invents the omitted market prelude/tail.
pub fn execute_production_ai_rate_cohort(
    state: &mut ProductionAiRateState,
    inputs: ProductionAiRateInputs,
) -> ProductionAiRateReceipt {
    state.shortages = 0;
    state.worst_good = 0;
    state.best_good = 0;

    if inputs.cap.starting_resources == 8 {
        for flags in &mut state.econ_flags {
            *flags |= 8;
        }
        return ProductionAiRateReceipt {
            returned_for_starting_resources: true,
            written_mask: 0,
            modified_caps: [0; RESOURCE_COUNT],
            decoded_income: [0; RESOURCE_COUNT],
            capped_income: [0; RESOURCE_COUNT],
            encrypted_rate_writes: [0; RESOURCE_COUNT],
        };
    }

    let mut lowest_rate = 99_999_999_i32;
    let mut highest_rate = -99_999_999_i32;
    let mut modified_caps = [0; RESOURCE_COUNT];
    let mut decoded_income = [0; RESOURCE_COUNT];
    let mut capped_income = [0; RESOURCE_COUNT];
    let mut encrypted_rate_writes = [0; RESOURCE_COUNT];

    for resource in 0..RESOURCE_COUNT {
        state.base_rate[resource] = 0;

        let cap = get_mod_resource_cap(inputs.cap, inputs.encrypted_resource_caps[resource]);
        let income = decode_income(inputs.encrypted_income[resource]);
        let capped = income.min(cap);
        let rate = capped / 16;
        let encrypted_rate = encode_planning_rate(rate);

        modified_caps[resource] = cap;
        decoded_income[resource] = income;
        capped_income[resource] = capped;
        encrypted_rate_writes[resource] = encrypted_rate;
        state.planning_rates.values[resource] = decode_planning_rate(encrypted_rate);

        if inputs.type_available[resource] {
            if rate < lowest_rate {
                state.worst_good = resource as i32;
                lowest_rate = rate;
            }
            if rate > highest_rate {
                state.best_good = resource as i32;
                highest_rate = rate;
            }
            if rate < 30 {
                state.econ_flags[resource] |= 1;
                state.shortages = state.shortages.wrapping_add(1);
            }
        }
    }

    ProductionAiRateReceipt {
        returned_for_starting_resources: false,
        written_mask: (1 << RESOURCE_COUNT) - 1,
        modified_caps,
        decoded_income,
        capped_income,
        encrypted_rate_writes,
    }
}

/// SSE `cvttss2si`: truncate toward zero, returning the architectural indefinite integer for
/// NaN, infinities and out-of-range inputs. Rust's float cast has different saturation rules.
#[inline]
fn cvttss2si(value: f32) -> i32 {
    if value.is_nan() || value >= 2_147_483_648.0 || value < -2_147_483_648.0 {
        i32::MIN
    } else {
        value as i32
    }
}
