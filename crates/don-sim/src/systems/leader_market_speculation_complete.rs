// SPDX-License-Identifier: GPL-3.0-or-later

//! Complete standalone composition of `Leader::market_speculation` `0x006C8110` (707 bytes).
//!
//! The sibling runtimes own the entry/scarcity pass and the remaining sell/buy passes. This
//! adapter binds both phases to one canonical host so the planning target mutated by the first
//! pass is the target observed by the second, while quote/trade arithmetic remains delegated to
//! the existing economy children. It introduces no alternate market, resource, or effect store.

use super::leader_market_speculation_runtime::{
    execute_market_speculation_opening, CanonicalProductionEconomy, MarketBuildingRow,
    MarketSpeculationOpeningExit, MarketSpeculationOpeningInputs, MarketSpeculationOpeningReceipt,
};
use super::leader_market_speculation_transaction::{
    execute_market_speculation_trade_passes, MarketSpeculationHost, MarketSpeculationTradeReceipt,
};

pub const MARKET_SPECULATION_VA: u32 = 0x006c_8110;

/// Owned copy of every fact used by the entry/scarcity phase. Taking this snapshot before the
/// mutable production-owner borrow makes the two-phase transaction explicit and borrow-safe.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompleteMarketSpeculationFacts {
    pub who: i32,
    pub nubian: bool,
    pub commerce_research: bool,
    pub market_type_count: i16,
    pub buildings: Vec<MarketBuildingRow>,
    pub nuke_embargo: bool,
    pub starting_resources: u8,
    pub type_available: [bool; 6],
    pub buckets: [i32; 6],
}

impl CompleteMarketSpeculationFacts {
    fn as_opening_inputs(&self) -> MarketSpeculationOpeningInputs<'_> {
        MarketSpeculationOpeningInputs {
            who: self.who,
            nubian: self.nubian,
            commerce_research: self.commerce_research,
            market_type_count: self.market_type_count,
            buildings: &self.buildings,
            nuke_embargo: self.nuke_embargo,
            starting_resources: self.starting_resources,
            type_available: self.type_available,
            buckets: self.buckets,
        }
    }
}

/// One host owns both phases. Implementations must return their authoritative production row,
/// not a projection detached from the `planning_target` reads used by `MarketSpeculationHost`.
pub trait CompleteMarketSpeculationHost: MarketSpeculationHost {
    fn production_economy(&mut self) -> &mut CanonicalProductionEconomy;
    fn opening_facts(&self) -> CompleteMarketSpeculationFacts;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompleteMarketSpeculationReceipt {
    pub opening: MarketSpeculationOpeningReceipt,
    /// Present exactly when the opening reaches `ReadyForSellPass`.
    pub trades: Option<MarketSpeculationTradeReceipt>,
}

/// Execute the complete recovered parent, preserving its phase order and early returns.
pub fn execute_market_speculation_complete<H: CompleteMarketSpeculationHost>(
    host: &mut H,
) -> CompleteMarketSpeculationReceipt {
    let facts = host.opening_facts();
    let opening =
        execute_market_speculation_opening(host.production_economy(), facts.as_opening_inputs());
    let trades = if opening.exit == MarketSpeculationOpeningExit::ReadyForSellPass {
        Some(
            execute_market_speculation_trade_passes(host, opening.scarcity)
                .expect("the retail opening emits only scarcity levels 0, 1, or 2"),
        )
    } else {
        None
    };
    CompleteMarketSpeculationReceipt { opening, trades }
}
