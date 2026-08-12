// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../src/systems/leader_market_speculation_runtime.rs"]
mod leader_market_speculation_runtime;
#[path = "../src/systems/leader_market_speculation_transaction.rs"]
mod leader_market_speculation_transaction;

use don_sim::systems::economy::{
    self, EconRules, LeaderEcon, MarketPriceGates, MarketState, TradeResult,
};
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::tick::Sim;
use leader_market_speculation_runtime::CanonicalProductionEconomy;
use leader_market_speculation_transaction::{
    execute_market_speculation_trade_passes, MarketQuote, MarketSpeculationCall,
    MarketSpeculationDecision, MarketSpeculationHost, ScarcityLevel, TradeOutcome, TradeSide,
};

#[derive(Clone, Copy, Debug)]
struct SeatFacts {
    type_available: [bool; 6],
    nubian_unlock: bool,
    commerce_research: bool,
    has_market: bool,
    embargo: bool,
    quote_gates: MarketPriceGates,
}

struct CanonicalHost<'a> {
    production: &'a mut CanonicalProductionEconomy,
    econ: &'a mut LeaderEcon,
    market: &'a mut MarketState,
    rules: &'a EconRules,
    facts: SeatFacts,
    embargo_effects: usize,
}

impl MarketSpeculationHost for CanonicalHost<'_> {
    fn type_available(&mut self, resource: usize) -> bool {
        self.facts.type_available[resource]
    }

    fn bucket(&self, resource: usize) -> i32 {
        self.econ.stockpile[resource]
    }

    fn planning_target(&self, resource: usize) -> i32 {
        self.production.econ[resource]
    }

    fn quote(&mut self, resource: usize) -> MarketQuote {
        let quote =
            economy::calc_market_prices(self.rules, self.market, resource, &self.facts.quote_gates);
        MarketQuote {
            buy: quote.buy,
            sell: quote.sell,
        }
    }

    fn has_nubian_bonus(&mut self) -> bool {
        self.facts.nubian_unlock
    }

    fn has_commerce_research(&mut self) -> bool {
        self.facts.commerce_research
    }

    fn has_market(&mut self) -> bool {
        self.facts.has_market
    }

    fn has_nuke_embargo(&mut self) -> bool {
        self.facts.embargo
    }

    fn do_sell(&mut self, resource: usize) -> TradeOutcome {
        let result = economy::do_sell(
            self.rules,
            self.market,
            self.econ,
            &mut self.production.escrow[resource],
            resource,
            &self.facts.quote_gates,
        );
        map_trade(result)
    }

    fn do_buy(&mut self, resource: usize) -> TradeOutcome {
        let result = economy::do_buy(
            self.rules,
            self.market,
            self.econ,
            &mut self.production.escrow[economy::RES_WEALTH],
            resource,
            &self.facts.quote_gates,
        );
        map_trade(result)
    }

    fn tell_embargo(&mut self) {
        self.embargo_effects += 1;
    }
}

fn map_trade(result: TradeResult) -> TradeOutcome {
    match result {
        TradeResult::Done => TradeOutcome::Done,
        TradeResult::Refused => TradeOutcome::Refused,
    }
}

fn ordinary_facts() -> SeatFacts {
    SeatFacts {
        type_available: [true; 6],
        nubian_unlock: false,
        commerce_research: true,
        has_market: true,
        embargo: false,
        quote_gates: MarketPriceGates::default(),
    }
}

fn ordinary_state() -> (
    CanonicalProductionEconomy,
    LeaderEcon,
    MarketState,
    EconRules,
) {
    let production = CanonicalProductionEconomy {
        econ: [300, 300, 250, 0, 300, 300],
        escrow: [800; 6],
        ..CanonicalProductionEconomy::default()
    };
    let mut econ = LeaderEcon::new();
    econ.stockpile = [1600, 1600, 1300, 900, 1600, 1600];
    let market = MarketState {
        base_price: [50, 60, 70, 80, 55, 65],
        spread: [5, 5, 5, 5, 5, 5],
        ..MarketState::default()
    };
    (production, econ, market, EconRules::shipped())
}

#[test]
fn exact_quote_child_drives_one_sell_and_one_buy_per_admitted_resource() {
    let (mut production, mut econ, mut market, rules) = ordinary_state();
    let before = econ.stockpile;
    let mut host = CanonicalHost {
        production: &mut production,
        econ: &mut econ,
        market: &mut market,
        rules: &rules,
        facts: ordinary_facts(),
        embargo_effects: 0,
    };

    let receipt = execute_market_speculation_trade_passes(&mut host, 1).unwrap();

    assert_eq!(receipt.scarcity, ScarcityLevel::UnderTwoHundred);
    assert_eq!(
        receipt.sell,
        [
            MarketSpeculationDecision::Traded(TradeOutcome::Done),
            MarketSpeculationDecision::Traded(TradeOutcome::Done),
            MarketSpeculationDecision::NonTradeResource,
            MarketSpeculationDecision::NonTradeResource,
            MarketSpeculationDecision::Traded(TradeOutcome::Done),
            MarketSpeculationDecision::Traded(TradeOutcome::Done),
        ]
    );
    assert!(receipt.buy.iter().enumerate().all(|(resource, decision)| {
        matches!(
            (resource, decision),
            (2 | 3, MarketSpeculationDecision::NonTradeResource)
                | (_, MarketSpeculationDecision::QuoteRejected)
                | (_, MarketSpeculationDecision::Traded(_))
        )
    }));
    assert_eq!(host.embargo_effects, 0);
    assert_eq!(host.econ.stockpile[0], before[0] - 100);
    assert_eq!(host.production.escrow[0], 700);
    assert_eq!(host.market.base_price[0], 47);

    let first_sell_quote = receipt
        .calls
        .iter()
        .position(|call| {
            matches!(
                call,
                MarketSpeculationCall::Quote {
                    side: TradeSide::Sell,
                    resource: 0,
                    ..
                }
            )
        })
        .unwrap();
    let first_sell_trade = receipt
        .calls
        .iter()
        .position(|call| {
            matches!(
                call,
                MarketSpeculationCall::Trade {
                    side: TradeSide::Sell,
                    resource: 0,
                    ..
                }
            )
        })
        .unwrap();
    assert!(first_sell_quote < first_sell_trade);
}

#[test]
fn admitted_buy_mutates_wealth_demand_bucket_and_shared_base_price_once() {
    let (mut production, mut econ, mut market, rules) = ordinary_state();
    production.econ[2] = 0;
    production.escrow[2] = 450;
    econ.stockpile[0] = 50;
    econ.stockpile[2] = 1000;
    market.base_price[0] = 45;
    market.spread[0] = 0;
    let facts = SeatFacts {
        type_available: [true, false, false, false, false, false],
        ..ordinary_facts()
    };
    let mut host = CanonicalHost {
        production: &mut production,
        econ: &mut econ,
        market: &mut market,
        rules: &rules,
        facts,
        embargo_effects: 0,
    };

    let receipt = execute_market_speculation_trade_passes(&mut host, 0).unwrap();
    assert_eq!(
        receipt.sell[0],
        MarketSpeculationDecision::InsufficientSurplus
    );
    assert_eq!(
        receipt.buy[0],
        MarketSpeculationDecision::Traded(TradeOutcome::Done)
    );
    assert_eq!(host.econ.stockpile[0], 150);
    assert_eq!(host.econ.stockpile[2], 910);
    assert_eq!(host.production.escrow[2], 350);
    assert_eq!(host.market.base_price[0], 48);
}

#[test]
fn sell_and_buy_thresholds_use_opening_scarcity_and_wrapping_surplus() {
    let (mut production, mut econ, mut market, rules) = ordinary_state();
    production.econ[0] = i32::MIN;
    econ.stockpile[0] = i32::MAX;
    econ.stockpile[1] = 1099;
    production.econ[1] = 100;
    let facts = SeatFacts {
        type_available: [true, true, false, false, false, false],
        ..ordinary_facts()
    };
    let mut host = CanonicalHost {
        production: &mut production,
        econ: &mut econ,
        market: &mut market,
        rules: &rules,
        facts,
        embargo_effects: 0,
    };

    let receipt = execute_market_speculation_trade_passes(&mut host, 2).unwrap();
    assert_eq!(
        receipt.sell[0],
        MarketSpeculationDecision::InsufficientSurplus,
        "INT_MAX - INT_MIN wraps to -1 before the signed threshold compare"
    );
    assert_eq!(
        receipt.sell[1],
        MarketSpeculationDecision::Traded(TradeOutcome::Done),
        "scarcity 2 lowers the sell surplus threshold to 500"
    );

    let snapshot = (host.production.econ, host.econ.stockpile, *host.market);
    assert!(execute_market_speculation_trade_passes(&mut host, 3).is_none());
    assert_eq!(
        (host.production.econ, host.econ.stockpile, *host.market),
        snapshot
    );
}

#[test]
fn retail_short_circuits_unlock_market_and_embargo_after_quote() {
    let (mut production, mut econ, mut market, rules) = ordinary_state();
    let facts = SeatFacts {
        type_available: [true, false, false, false, false, false],
        nubian_unlock: false,
        commerce_research: true,
        has_market: true,
        embargo: true,
        quote_gates: MarketPriceGates::default(),
    };
    let before = (production, econ, market);
    let mut host = CanonicalHost {
        production: &mut production,
        econ: &mut econ,
        market: &mut market,
        rules: &rules,
        facts,
        embargo_effects: 0,
    };

    let receipt = execute_market_speculation_trade_passes(&mut host, 1).unwrap();
    assert_eq!(receipt.sell[0], MarketSpeculationDecision::Embargoed);
    assert_eq!(host.embargo_effects, 1);
    assert_eq!((*host.production, *host.econ, *host.market), before);

    let sequence: Vec<_> = receipt
        .calls
        .iter()
        .filter(|call| match call {
            MarketSpeculationCall::Quote {
                side: TradeSide::Sell,
                resource: 0,
                ..
            }
            | MarketSpeculationCall::Nubian {
                side: TradeSide::Sell,
                resource: 0,
                ..
            }
            | MarketSpeculationCall::CommerceResearch {
                side: TradeSide::Sell,
                resource: 0,
                ..
            }
            | MarketSpeculationCall::HasMarket {
                side: TradeSide::Sell,
                resource: 0,
                ..
            }
            | MarketSpeculationCall::NukeEmbargo {
                side: TradeSide::Sell,
                resource: 0,
                ..
            }
            | MarketSpeculationCall::TellEmbargo {
                side: TradeSide::Sell,
                resource: 0,
            } => true,
            _ => false,
        })
        .collect();
    assert!(matches!(sequence[0], MarketSpeculationCall::Quote { .. }));
    assert!(matches!(
        sequence[1],
        MarketSpeculationCall::Nubian { present: false, .. }
    ));
    assert!(matches!(
        sequence[2],
        MarketSpeculationCall::CommerceResearch { present: true, .. }
    ));
    assert!(matches!(
        sequence[3],
        MarketSpeculationCall::HasMarket { present: true, .. }
    ));
    assert!(matches!(
        sequence[4],
        MarketSpeculationCall::NukeEmbargo { present: true, .. }
    ));
    assert!(matches!(
        sequence[5],
        MarketSpeculationCall::TellEmbargo { .. }
    ));
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NaturalWorld {
    production: [CanonicalProductionEconomy; 4],
    econ: [LeaderEcon; 4],
    market: MarketState,
}

fn generated_world(seed: u32) -> NaturalWorld {
    let mut value = seed;
    let mut next = || {
        value = value.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        value
    };
    let production = std::array::from_fn(|_| {
        let mut row = CanonicalProductionEconomy::default();
        for resource in 0..6 {
            row.econ[resource] = 100 + (next() % 700) as i32;
            row.escrow[resource] = 200 + (next() % 1200) as i32;
        }
        row
    });
    let econ = std::array::from_fn(|_| {
        let mut row = LeaderEcon::new();
        for resource in 0..6 {
            row.stockpile[resource] = 600 + (next() % 2200) as i32;
        }
        row
    });
    let mut market = MarketState::default();
    for resource in 0..6 {
        market.base_price[resource] = 25 + (next() % 75) as i32;
        market.spread[resource] = (next() % 21) as i32 - 10;
    }
    NaturalWorld {
        production,
        econ,
        market,
    }
}

fn generated_facts(seed: u32, seat: usize, generation: u32) -> SeatFacts {
    let mut value = seed
        .wrapping_add((seat as u32 + 1).wrapping_mul(0x9e37_79b9))
        .wrapping_add(generation.wrapping_mul(0x85eb_ca6b));
    let mut next = || {
        value = value.wrapping_mul(22_695_477).wrapping_add(1);
        value
    };
    let mut type_available = [false; 6];
    for available in &mut type_available {
        *available = next() % 5 != 0;
    }
    SeatFacts {
        type_available,
        nubian_unlock: seat == 0,
        commerce_research: true,
        has_market: true,
        embargo: false,
        quote_gates: MarketPriceGates {
            nubian: seat == 0,
            amber: seat == 1,
            super_market: seat == 2,
            ctw_stacks: (next() % 3) as u8,
            russian: seat == 3,
            age: (next() % 6) as i32,
        },
    }
}

fn execute_generation(
    world: &mut NaturalWorld,
    seed: u32,
    generation: u32,
) -> Vec<Vec<MarketSpeculationCall>> {
    let rules = EconRules::shipped();
    let mut traces = Vec::new();
    for seat in 0..4 {
        let mut host = CanonicalHost {
            production: &mut world.production[seat],
            econ: &mut world.econ[seat],
            market: &mut world.market,
            rules: &rules,
            facts: generated_facts(seed, seat, generation),
            embargo_effects: 0,
        };
        let scarcity = (seed.wrapping_add(seat as u32).wrapping_add(generation)) % 3;
        let receipt = execute_market_speculation_trade_passes(&mut host, scarcity).unwrap();
        assert_eq!(host.embargo_effects, 0);
        traces.push(receipt.calls);
    }
    traces
}

fn save_and_restore(world: &NaturalWorld, seed: u32) -> NaturalWorld {
    let mut sim = Sim::new(u64::from(seed), 4);
    for seat in 0..4 {
        sim.leaders[seat].econ = world.econ[seat];
    }
    sim.market = world.market;
    let bytes = save_sim(&sim).expect("canonical leader economy and market must save");
    let restored = load_sim(&bytes).expect("canonical leader economy and market must load");

    let production = std::array::from_fn(|seat| {
        CanonicalProductionEconomy::from_save_extension_bytes(
            &world.production[seat].save_extension_bytes(),
        )
        .unwrap()
    });
    NaturalWorld {
        production,
        econ: std::array::from_fn(|seat| restored.leaders[seat].econ),
        market: restored.market,
    }
}

#[test]
fn natural_four_seat_multi_seed_real_economy_market_save_resume_converges() {
    for seed in [0x49, 0x51a7_11ce, 0xd06f_00d5] {
        let mut uninterrupted = generated_world(seed);
        execute_generation(&mut uninterrupted, seed, 0);
        let mut resumed = save_and_restore(&uninterrupted, seed);

        let direct_trace = execute_generation(&mut uninterrupted, seed, 1);
        let resumed_trace = execute_generation(&mut resumed, seed, 1);
        assert_eq!(resumed_trace, direct_trace, "seed={seed:#x}");
        assert_eq!(resumed, uninterrupted, "seed={seed:#x}");
    }
}
