// SPDX-License-Identifier: GPL-3.0-or-later

//! Canonical `LeaderData` production-economy owner and the executable opening of
//! `Leader::market_speculation` `0x006C8110`.
//!
//! The plain PDB block `LeaderData +0x450..+0x4D4` contains five six-i32 arrays followed by
//! `worst_good`, `best_good`, and `shortages`. It is distinct from the encrypted economy block
//! at `LeaderData::data_encrypted`. This module owns the decoded/plain block without borrowing
//! the live market or resource stores.
//!
//! The executable closes the complete child `LeaderData::has_market` `0x006D5410` and the
//! parent entry plus its first six-resource scarcity pass. It stops before the first
//! `calc_market_prices` call. Buy/sell transactions, embargo presentation, and the remaining
//! two loops are deliberately outside this boundary.

use std::fmt;

pub const RESOURCE_COUNT: usize = 6;
pub const MARKET_SPECULATION_VA: u32 = 0x006c_8110;
pub const HAS_MARKET_VA: u32 = 0x006d_5410;
pub const CALC_MARKET_PRICES_VA: u32 = 0x006d_c2a0;
pub const PRODUCTION_ECONOMY_VALUES: usize = RESOURCE_COUNT * 5 + 3;
pub const PRODUCTION_ECONOMY_BYTES: usize = PRODUCTION_ECONOMY_VALUES * 4;

/// Canonical plain `LeaderData +0x450..+0x4D4` state, in exact PDB field order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CanonicalProductionEconomy {
    /// `LeaderData +0x450`, the planning target/flag row read by market speculation.
    pub econ: [i32; RESOURCE_COUNT],
    /// `LeaderData +0x468`, counters decremented by retail `do_buy` / `do_sell`.
    pub escrow: [i32; RESOURCE_COUNT],
    pub escrow_rate: [i32; RESOURCE_COUNT],
    pub tributes: [i32; RESOURCE_COUNT],
    /// `LeaderData +0x4B0`; production setup clears all six values in its normal rate loop.
    pub base_rate: [i32; RESOURCE_COUNT],
    pub worst_good: i32,
    pub best_good: i32,
    pub shortages: i32,
}

impl CanonicalProductionEconomy {
    pub const fn save_extension_values(self) -> [i32; PRODUCTION_ECONOMY_VALUES] {
        let mut values = [0; PRODUCTION_ECONOMY_VALUES];
        let mut resource = 0;
        while resource < RESOURCE_COUNT {
            values[resource] = self.econ[resource];
            values[RESOURCE_COUNT + resource] = self.escrow[resource];
            values[RESOURCE_COUNT * 2 + resource] = self.escrow_rate[resource];
            values[RESOURCE_COUNT * 3 + resource] = self.tributes[resource];
            values[RESOURCE_COUNT * 4 + resource] = self.base_rate[resource];
            resource += 1;
        }
        values[RESOURCE_COUNT * 5] = self.worst_good;
        values[RESOURCE_COUNT * 5 + 1] = self.best_good;
        values[RESOURCE_COUNT * 5 + 2] = self.shortages;
        values
    }

    pub const fn from_save_extension_values(values: [i32; PRODUCTION_ECONOMY_VALUES]) -> Self {
        let mut state = Self {
            econ: [0; RESOURCE_COUNT],
            escrow: [0; RESOURCE_COUNT],
            escrow_rate: [0; RESOURCE_COUNT],
            tributes: [0; RESOURCE_COUNT],
            base_rate: [0; RESOURCE_COUNT],
            worst_good: values[RESOURCE_COUNT * 5],
            best_good: values[RESOURCE_COUNT * 5 + 1],
            shortages: values[RESOURCE_COUNT * 5 + 2],
        };
        let mut resource = 0;
        while resource < RESOURCE_COUNT {
            state.econ[resource] = values[resource];
            state.escrow[resource] = values[RESOURCE_COUNT + resource];
            state.escrow_rate[resource] = values[RESOURCE_COUNT * 2 + resource];
            state.tributes[resource] = values[RESOURCE_COUNT * 3 + resource];
            state.base_rate[resource] = values[RESOURCE_COUNT * 4 + resource];
            resource += 1;
        }
        state
    }

    /// Exact proposed canonical Leader-row projection: 33 decoded/plain little-endian i32s.
    pub fn save_extension_bytes(self) -> [u8; PRODUCTION_ECONOMY_BYTES] {
        let mut bytes = [0; PRODUCTION_ECONOMY_BYTES];
        for (index, value) in self.save_extension_values().into_iter().enumerate() {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    pub fn from_save_extension_bytes(bytes: &[u8]) -> Result<Self, ProductionEconomyCodecError> {
        if bytes.len() != PRODUCTION_ECONOMY_BYTES {
            return Err(ProductionEconomyCodecError::Length {
                expected: PRODUCTION_ECONOMY_BYTES,
                actual: bytes.len(),
            });
        }
        let mut values = [0; PRODUCTION_ECONOMY_VALUES];
        for (index, value) in values.iter_mut().enumerate() {
            *value = i32::from_le_bytes(
                bytes[index * 4..index * 4 + 4]
                    .try_into()
                    .expect("production-economy extension length checked"),
            );
        }
        Ok(Self::from_save_extension_values(values))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProductionEconomyCodecError {
    Length { expected: usize, actual: usize },
}

impl fmt::Display for ProductionEconomyCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length { expected, actual } => write!(
                f,
                "production-economy row is {actual} bytes; expected {expected}"
            ),
        }
    }
}

impl std::error::Error for ProductionEconomyCodecError {}

/// One `Build` row visible to `LeaderData::has_market` through the owner's compact object list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MarketBuildingRow {
    /// `ObjectData +0x04`; bit 0 is active and bit 11 is the market capability.
    pub object_flags: u16,
    /// Signed owner byte at `ObjectData +0x5F`.
    pub owner: i8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HasMarketInputs<'a> {
    /// `LeaderData +0x08`.
    pub who: i32,
    /// `LeaderData +0x558A`. Zero returns before the object list is touched.
    pub market_type_count: i16,
    /// Authoritative compact per-owner Build rows, in retail list order.
    pub buildings: &'a [MarketBuildingRow],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HasMarketReceipt {
    pub visited: usize,
    pub found_at: Option<usize>,
}

/// Complete `LeaderData::has_market` `0x006D5410`.
pub fn has_market(inputs: HasMarketInputs<'_>) -> (bool, HasMarketReceipt) {
    if inputs.market_type_count == 0 {
        return (false, HasMarketReceipt::default());
    }

    for (index, row) in inputs.buildings.iter().copied().enumerate() {
        if row.object_flags & 1 != 0
            && i32::from(row.owner) == inputs.who
            && row.object_flags & 0x0800 != 0
        {
            return (
                true,
                HasMarketReceipt {
                    visited: index + 1,
                    found_at: Some(index),
                },
            );
        }
    }
    (
        false,
        HasMarketReceipt {
            visited: inputs.buildings.len(),
            found_at: None,
        },
    )
}

/// Complete external facts read before `market_speculation` enters its scarcity pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MarketSpeculationOpeningInputs<'a> {
    pub who: i32,
    /// Exact `has_tribe_bonus(4)` answer.
    pub nubian: bool,
    /// Exact `has_preq(0x2AD)` answer, read only when `nubian` is false in retail.
    pub commerce_research: bool,
    pub market_type_count: i16,
    pub buildings: &'a [MarketBuildingRow],
    /// Exact `get_nuke_embargo()` answer. Nonzero blocks speculation.
    pub nuke_embargo: bool,
    pub starting_resources: u8,
    /// Exact `ResourceType::type_avail(resource, 1)` answers.
    pub type_available: [bool; RESOURCE_COUNT],
    /// Existing canonical decoded `LeaderDataEncrypt::bucket[6]` owner.
    pub buckets: [i32; RESOURCE_COUNT],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarketSpeculationOpeningExit {
    MissingUnlock,
    MissingMarket,
    Embargoed,
    StartingResourcesEight,
    ReadyForSellPass,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarketSpeculationOpeningReceipt {
    pub exit: MarketSpeculationOpeningExit,
    pub read_commerce_research: bool,
    pub market: HasMarketReceipt,
    /// Resources for which `type_avail` admitted the scarcity body.
    pub visited_mask: u8,
    /// Resources whose `econ` value was signed-greater than 4000 and reset to 2000.
    pub clamped_mask: u8,
    /// 0 none, 1 at least one available bucket below 200, 2 at least one below 100.
    pub scarcity: u32,
}

impl MarketSpeculationOpeningReceipt {
    fn early(
        exit: MarketSpeculationOpeningExit,
        read_commerce_research: bool,
        market: HasMarketReceipt,
    ) -> Self {
        Self {
            exit,
            read_commerce_research,
            market,
            visited_mask: 0,
            clamped_mask: 0,
            scarcity: 0,
        }
    }
}

/// Execute the exact entry gate and first resource loop of `Leader::market_speculation`.
pub fn execute_market_speculation_opening(
    state: &mut CanonicalProductionEconomy,
    inputs: MarketSpeculationOpeningInputs<'_>,
) -> MarketSpeculationOpeningReceipt {
    let read_commerce_research = !inputs.nubian;
    if !inputs.nubian && !inputs.commerce_research {
        return MarketSpeculationOpeningReceipt::early(
            MarketSpeculationOpeningExit::MissingUnlock,
            read_commerce_research,
            HasMarketReceipt::default(),
        );
    }

    let (market_present, market) = has_market(HasMarketInputs {
        who: inputs.who,
        market_type_count: inputs.market_type_count,
        buildings: inputs.buildings,
    });
    if !market_present {
        return MarketSpeculationOpeningReceipt::early(
            MarketSpeculationOpeningExit::MissingMarket,
            read_commerce_research,
            market,
        );
    }
    if inputs.nuke_embargo {
        return MarketSpeculationOpeningReceipt::early(
            MarketSpeculationOpeningExit::Embargoed,
            read_commerce_research,
            market,
        );
    }
    if inputs.starting_resources == 8 {
        return MarketSpeculationOpeningReceipt::early(
            MarketSpeculationOpeningExit::StartingResourcesEight,
            read_commerce_research,
            market,
        );
    }

    let mut visited_mask = 0_u8;
    let mut clamped_mask = 0_u8;
    let mut scarcity = 0_u32;
    for resource in 0..RESOURCE_COUNT {
        if !inputs.type_available[resource] {
            continue;
        }
        visited_mask |= 1 << resource;
        if state.econ[resource] > 4000 {
            state.econ[resource] = 2000;
            clamped_mask |= 1 << resource;
        }
        if inputs.buckets[resource] < 100 {
            scarcity = 2;
        } else if inputs.buckets[resource] < 200 && scarcity == 0 {
            scarcity = 1;
        }
    }

    MarketSpeculationOpeningReceipt {
        exit: MarketSpeculationOpeningExit::ReadyForSellPass,
        read_commerce_research,
        market,
        visited_mask,
        clamped_mask,
        scarcity,
    }
}
