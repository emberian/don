// SPDX-License-Identifier: GPL-3.0-or-later

//! Exact remaining trade passes of `Leader::market_speculation` `0x006C8110`.
//!
//! The opening/scarcity pass and its canonical production-economy owner live in the sibling
//! standalone runtime. This continuation begins with that pass's scarcity result and executes
//! both remaining six-resource loops. Market quote and trade arithmetic stay with their
//! existing canonical owners: the host delegates to `LeaderData::calc_market_prices`
//! `0x006DC2A0`, `Leader::do_sell` `0x006CFC60`, and `Leader::do_buy` `0x006CFBD0`.
//! This file does not copy that arithmetic or own presentation.

pub const RESOURCE_COUNT: usize = 6;
pub const RES_WEALTH: usize = 2;
pub const RES_KNOWLEDGE: usize = 3;
pub const MARKET_SPECULATION_VA: u32 = 0x006c_8110;
pub const CALC_MARKET_PRICES_VA: u32 = 0x006d_c2a0;
pub const DO_BUY_VA: u32 = 0x006c_fbd0;
pub const DO_SELL_VA: u32 = 0x006c_fc60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScarcityLevel {
    None,
    UnderTwoHundred,
    UnderOneHundred,
}

impl ScarcityLevel {
    pub const fn from_opening(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::UnderTwoHundred),
            2 => Some(Self::UnderOneHundred),
            _ => None,
        }
    }

    const fn divisor(self) -> i32 {
        match self {
            Self::None => 1,
            Self::UnderTwoHundred => 2,
            Self::UnderOneHundred => 4,
        }
    }

    const fn ordinal(self) -> i32 {
        match self {
            Self::None => 0,
            Self::UnderTwoHundred => 1,
            Self::UnderOneHundred => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MarketQuote {
    pub buy: i32,
    pub sell: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeSide {
    Sell,
    Buy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeOutcome {
    Done,
    Refused,
}

/// Ordered external calls made by the two retail loops. Plain state reads are represented in
/// the per-resource decisions rather than fabricated as host calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarketSpeculationCall {
    TypeAvailable {
        side: TradeSide,
        resource: usize,
        available: bool,
    },
    Quote {
        side: TradeSide,
        resource: usize,
        quote: MarketQuote,
    },
    Nubian {
        side: TradeSide,
        resource: usize,
        present: bool,
    },
    CommerceResearch {
        side: TradeSide,
        resource: usize,
        present: bool,
    },
    HasMarket {
        side: TradeSide,
        resource: usize,
        present: bool,
    },
    NukeEmbargo {
        side: TradeSide,
        resource: usize,
        present: bool,
    },
    Trade {
        side: TradeSide,
        resource: usize,
        outcome: TradeOutcome,
    },
    TellEmbargo {
        side: TradeSide,
        resource: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarketSpeculationDecision {
    Unavailable,
    NonTradeResource,
    InsufficientSurplus,
    QuoteRejected,
    MissingUnlock,
    MissingMarket,
    Embargoed,
    Traded(TradeOutcome),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarketSpeculationTradeReceipt {
    pub scarcity: ScarcityLevel,
    pub sell: [MarketSpeculationDecision; RESOURCE_COUNT],
    pub buy: [MarketSpeculationDecision; RESOURCE_COUNT],
    pub calls: Vec<MarketSpeculationCall>,
}

/// Canonical facts and transaction delegates for the remaining parent body.
///
/// The quote/trade methods are the exact ownership boundary: a Sim host delegates them to the
/// existing `systems::economy` children. UI/audio work from `tell_embargo` is emitted as an
/// ordered effect and stays outside canonical state mutation.
pub trait MarketSpeculationHost {
    fn type_available(&mut self, resource: usize) -> bool;
    fn bucket(&self, resource: usize) -> i32;
    fn planning_target(&self, resource: usize) -> i32;
    fn quote(&mut self, resource: usize) -> MarketQuote;
    fn has_nubian_bonus(&mut self) -> bool;
    fn has_commerce_research(&mut self) -> bool;
    fn has_market(&mut self) -> bool;
    fn has_nuke_embargo(&mut self) -> bool;
    fn do_sell(&mut self, resource: usize) -> TradeOutcome;
    fn do_buy(&mut self, resource: usize) -> TradeOutcome;
    fn tell_embargo(&mut self);
}

fn unlock_and_market<H: MarketSpeculationHost>(
    host: &mut H,
    calls: &mut Vec<MarketSpeculationCall>,
    side: TradeSide,
    resource: usize,
) -> Result<(), MarketSpeculationDecision> {
    let nubian = host.has_nubian_bonus();
    calls.push(MarketSpeculationCall::Nubian {
        side,
        resource,
        present: nubian,
    });
    if !nubian {
        let research = host.has_commerce_research();
        calls.push(MarketSpeculationCall::CommerceResearch {
            side,
            resource,
            present: research,
        });
        if !research {
            return Err(MarketSpeculationDecision::MissingUnlock);
        }
    }

    let market = host.has_market();
    calls.push(MarketSpeculationCall::HasMarket {
        side,
        resource,
        present: market,
    });
    if !market {
        return Err(MarketSpeculationDecision::MissingMarket);
    }
    Ok(())
}

fn trade_or_embargo<H: MarketSpeculationHost>(
    host: &mut H,
    calls: &mut Vec<MarketSpeculationCall>,
    side: TradeSide,
    resource: usize,
) -> MarketSpeculationDecision {
    let embargo = host.has_nuke_embargo();
    calls.push(MarketSpeculationCall::NukeEmbargo {
        side,
        resource,
        present: embargo,
    });
    if embargo {
        host.tell_embargo();
        calls.push(MarketSpeculationCall::TellEmbargo { side, resource });
        return MarketSpeculationDecision::Embargoed;
    }

    let outcome = match side {
        TradeSide::Sell => host.do_sell(resource),
        TradeSide::Buy => host.do_buy(resource),
    };
    calls.push(MarketSpeculationCall::Trade {
        side,
        resource,
        outcome,
    });
    MarketSpeculationDecision::Traded(outcome)
}

/// Execute `market_speculation`'s sell pass followed by its buy pass.
///
/// `scarcity` must be the exact 0..2 value returned by the opening pass. The host remains
/// untouched for an invalid detached value rather than turning impossible state into gameplay.
pub fn execute_market_speculation_trade_passes<H: MarketSpeculationHost>(
    host: &mut H,
    scarcity: u32,
) -> Option<MarketSpeculationTradeReceipt> {
    let scarcity = ScarcityLevel::from_opening(scarcity)?;
    let mut calls = Vec::new();
    let mut sell = [MarketSpeculationDecision::Unavailable; RESOURCE_COUNT];
    let mut buy = [MarketSpeculationDecision::Unavailable; RESOURCE_COUNT];

    for resource in 0..RESOURCE_COUNT {
        let available = host.type_available(resource);
        calls.push(MarketSpeculationCall::TypeAvailable {
            side: TradeSide::Sell,
            resource,
            available,
        });
        if !available {
            continue;
        }
        if resource == RES_WEALTH || resource == RES_KNOWLEDGE {
            sell[resource] = MarketSpeculationDecision::NonTradeResource;
            continue;
        }

        let surplus = host
            .bucket(resource)
            .wrapping_sub(host.planning_target(resource));
        if surplus < 2000 / scarcity.divisor() {
            sell[resource] = MarketSpeculationDecision::InsufficientSurplus;
            continue;
        }

        let quote = host.quote(resource);
        calls.push(MarketSpeculationCall::Quote {
            side: TradeSide::Sell,
            resource,
            quote,
        });
        if quote.sell < 75 / scarcity.ordinal().wrapping_add(1) {
            sell[resource] = MarketSpeculationDecision::QuoteRejected;
            continue;
        }
        if let Err(decision) = unlock_and_market(host, &mut calls, TradeSide::Sell, resource) {
            sell[resource] = decision;
            continue;
        }
        sell[resource] = trade_or_embargo(host, &mut calls, TradeSide::Sell, resource);
    }

    for resource in 0..RESOURCE_COUNT {
        let available = host.type_available(resource);
        calls.push(MarketSpeculationCall::TypeAvailable {
            side: TradeSide::Buy,
            resource,
            available,
        });
        if !available {
            continue;
        }
        if resource == RES_WEALTH || resource == RES_KNOWLEDGE {
            buy[resource] = MarketSpeculationDecision::NonTradeResource;
            continue;
        }

        let quote = host.quote(resource);
        calls.push(MarketSpeculationCall::Quote {
            side: TradeSide::Buy,
            resource,
            quote,
        });
        let wealth_surplus = host
            .bucket(RES_WEALTH)
            .wrapping_sub(host.planning_target(RES_WEALTH));
        let bucket = host.bucket(resource);
        let quote_admitted = quote.buy <= wealth_surplus
            && bucket < 2000
            && quote.buy < 201
            && (bucket < 500 || quote.buy < 26)
            && (bucket < 200 || quote.buy < 51)
            && (bucket < 100 || quote.buy < 101);
        if !quote_admitted {
            buy[resource] = MarketSpeculationDecision::QuoteRejected;
            continue;
        }
        if let Err(decision) = unlock_and_market(host, &mut calls, TradeSide::Buy, resource) {
            buy[resource] = decision;
            continue;
        }
        buy[resource] = trade_or_embargo(host, &mut calls, TradeSide::Buy, resource);
    }

    Some(MarketSpeculationTradeReceipt {
        scarcity,
        sell,
        buy,
        calls,
    })
}
