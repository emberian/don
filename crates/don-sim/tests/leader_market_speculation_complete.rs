// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../src/systems/leader_market_speculation_complete.rs"]
mod leader_market_speculation_complete;
#[path = "../src/systems/leader_market_speculation_runtime.rs"]
mod leader_market_speculation_runtime;
#[path = "../src/systems/leader_market_speculation_transaction.rs"]
mod leader_market_speculation_transaction;
#[path = "../src/systems/leader_nuke_embargo_runtime.rs"]
mod leader_nuke_embargo_runtime;

use don_sim::systems::economy::{
    self, EconRules, LeaderEcon, MarketPriceGates, MarketState, TradeResult,
};
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::tick::Sim;
use leader_market_speculation_complete::{
    execute_market_speculation_complete, CompleteMarketSpeculationFacts,
    CompleteMarketSpeculationHost,
};
use leader_market_speculation_runtime::{
    has_market, CanonicalProductionEconomy, HasMarketInputs, MarketBuildingRow,
    MarketSpeculationOpeningExit,
};
use leader_market_speculation_transaction::{
    MarketQuote, MarketSpeculationDecision, MarketSpeculationHost, TradeOutcome,
};
use leader_nuke_embargo_runtime::{
    get_nuke_embargo, table_extension_bytes, table_from_extension_bytes, CanonicalNukeEmbargo,
    NukeEmbargoInputs, NukeEmbargoRules,
};

#[derive(Clone, Debug)]
struct CompleteFacts {
    who: usize,
    nubian: bool,
    commerce_research: bool,
    market_type_count: i16,
    buildings: Vec<MarketBuildingRow>,
    starting_resources: u8,
    type_available: [bool; 6],
    quote_gates: MarketPriceGates,
    nuke: NukeEmbargoInputs,
}

struct CanonicalCompleteHost<'a> {
    production: &'a mut CanonicalProductionEconomy,
    econ: &'a mut LeaderEcon,
    market: &'a mut MarketState,
    rules: &'a EconRules,
    facts: CompleteFacts,
    embargo_effects: usize,
}

impl CanonicalCompleteHost<'_> {
    fn embargo(&self) -> bool {
        get_nuke_embargo(&self.facts.nuke).expect("canonical who").0 != 0
    }

    fn market_present(&self) -> bool {
        has_market(HasMarketInputs {
            who: self.facts.who as i32,
            market_type_count: self.facts.market_type_count,
            buildings: &self.facts.buildings,
        })
        .0
    }
}

impl MarketSpeculationHost for CanonicalCompleteHost<'_> {
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
        self.facts.nubian
    }

    fn has_commerce_research(&mut self) -> bool {
        self.facts.commerce_research
    }

    fn has_market(&mut self) -> bool {
        self.market_present()
    }

    fn has_nuke_embargo(&mut self) -> bool {
        self.embargo()
    }

    fn do_sell(&mut self, resource: usize) -> TradeOutcome {
        map_trade(economy::do_sell(
            self.rules,
            self.market,
            self.econ,
            &mut self.production.escrow[resource],
            resource,
            &self.facts.quote_gates,
        ))
    }

    fn do_buy(&mut self, resource: usize) -> TradeOutcome {
        map_trade(economy::do_buy(
            self.rules,
            self.market,
            self.econ,
            &mut self.production.escrow[economy::RES_WEALTH],
            resource,
            &self.facts.quote_gates,
        ))
    }

    fn tell_embargo(&mut self) {
        self.embargo_effects += 1;
    }
}

impl CompleteMarketSpeculationHost for CanonicalCompleteHost<'_> {
    fn production_economy(&mut self) -> &mut CanonicalProductionEconomy {
        self.production
    }

    fn opening_facts(&self) -> CompleteMarketSpeculationFacts {
        CompleteMarketSpeculationFacts {
            who: self.facts.who as i32,
            nubian: self.facts.nubian,
            commerce_research: self.facts.commerce_research,
            market_type_count: self.facts.market_type_count,
            buildings: self.facts.buildings.clone(),
            nuke_embargo: self.embargo(),
            starting_resources: self.facts.starting_resources,
            type_available: self.facts.type_available,
            buckets: self.econ.stockpile,
        }
    }
}

fn map_trade(result: TradeResult) -> TradeOutcome {
    match result {
        TradeResult::Done => TradeOutcome::Done,
        TradeResult::Refused => TradeOutcome::Refused,
    }
}

fn baseline() -> (
    CanonicalProductionEconomy,
    LeaderEcon,
    MarketState,
    EconRules,
    CompleteFacts,
) {
    let production = CanonicalProductionEconomy {
        econ: [5001, 300, 0, 0, 5001, 5001],
        escrow: [700; 6],
        ..CanonicalProductionEconomy::default()
    };
    let mut econ = LeaderEcon::new();
    econ.stockpile = [50, 1800, 1400, 900, 1800, 1800];
    let market = MarketState {
        base_price: [45, 60, 70, 80, 55, 65],
        spread: [0, 5, 5, 5, 5, 5],
        ..MarketState::default()
    };
    let mut nuke = NukeEmbargoInputs {
        who: 2,
        active: [false, false, true, false, false, false, false, false],
        frame: 2_000,
        rules: NukeEmbargoRules::shipped(),
        ..NukeEmbargoInputs::default()
    };
    nuke.owners[2] = CanonicalNukeEmbargo::default();
    let facts = CompleteFacts {
        who: 2,
        nubian: true,
        commerce_research: false,
        market_type_count: 1,
        buildings: vec![MarketBuildingRow {
            object_flags: 0x0801,
            owner: 2,
        }],
        starting_resources: 0,
        type_available: [true; 6],
        quote_gates: MarketPriceGates::default(),
        nuke,
    };
    (production, econ, market, EconRules::shipped(), facts)
}

#[test]
fn full_parent_feeds_opening_clamps_and_scarcity_into_exact_trade_passes() {
    let (mut production, mut econ, mut market, rules, facts) = baseline();
    let mut host = CanonicalCompleteHost {
        production: &mut production,
        econ: &mut econ,
        market: &mut market,
        rules: &rules,
        facts,
        embargo_effects: 0,
    };

    let receipt = execute_market_speculation_complete(&mut host);

    assert_eq!(
        receipt.opening.exit,
        MarketSpeculationOpeningExit::ReadyForSellPass
    );
    assert_eq!(receipt.opening.scarcity, 2);
    assert_eq!(receipt.opening.clamped_mask, 0b11_0001);
    let trades = receipt.trades.unwrap();
    assert_eq!(
        trades.buy[0],
        MarketSpeculationDecision::Traded(TradeOutcome::Done)
    );
    assert_eq!(host.production.econ[0], 2000);
    assert_eq!(host.econ.stockpile[0], 150);
    assert_eq!(host.econ.stockpile[2], 1375);
    assert_eq!(host.market.base_price[0], 48);
    assert_eq!(host.embargo_effects, 0);
}

#[test]
fn complete_nuke_child_stops_parent_before_scarcity_or_trade_mutation() {
    let (mut production, mut econ, mut market, rules, mut facts) = baseline();
    facts.nuke.owners[2] = CanonicalNukeEmbargo {
        nuke_stamp: 1_900,
        nukes_used: 1,
    };
    let before = (production, econ, market);
    let mut host = CanonicalCompleteHost {
        production: &mut production,
        econ: &mut econ,
        market: &mut market,
        rules: &rules,
        facts,
        embargo_effects: 0,
    };

    let receipt = execute_market_speculation_complete(&mut host);
    assert_eq!(
        receipt.opening.exit,
        MarketSpeculationOpeningExit::Embargoed
    );
    assert!(receipt.trades.is_none());
    assert_eq!((*host.production, *host.econ, *host.market), before);
    assert_eq!(host.embargo_effects, 0);
}

#[test]
fn every_entry_early_return_omits_the_trade_phase() {
    let (production, econ, market, rules, facts) = baseline();
    let variants = [
        CompleteFacts {
            nubian: false,
            commerce_research: false,
            ..facts.clone()
        },
        CompleteFacts {
            market_type_count: 0,
            ..facts.clone()
        },
        CompleteFacts {
            starting_resources: 8,
            ..facts.clone()
        },
    ];
    let exits = [
        MarketSpeculationOpeningExit::MissingUnlock,
        MarketSpeculationOpeningExit::MissingMarket,
        MarketSpeculationOpeningExit::StartingResourcesEight,
    ];
    for (facts, exit) in variants.into_iter().zip(exits) {
        let mut production = production;
        let mut econ = econ;
        let mut market = market;
        let mut host = CanonicalCompleteHost {
            production: &mut production,
            econ: &mut econ,
            market: &mut market,
            rules: &rules,
            facts,
            embargo_effects: 0,
        };
        let receipt = execute_market_speculation_complete(&mut host);
        assert_eq!(receipt.opening.exit, exit);
        assert!(receipt.trades.is_none());
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NaturalWorld {
    production: [CanonicalProductionEconomy; 4],
    econ: [LeaderEcon; 4],
    market: MarketState,
    nuke: [CanonicalNukeEmbargo; 8],
}

fn generated_world(seed: u32) -> NaturalWorld {
    let mut value = seed.wrapping_add(0x9e37_79b9);
    let mut next = || {
        value = value.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        value
    };
    let production = std::array::from_fn(|_| {
        let mut row = CanonicalProductionEconomy::default();
        for resource in 0..6 {
            row.econ[resource] = 100 + (next() % 5_500) as i32;
            row.escrow[resource] = 200 + (next() % 1_200) as i32;
        }
        row
    });
    let econ = std::array::from_fn(|_| {
        let mut row = LeaderEcon::new();
        for resource in 0..6 {
            row.stockpile[resource] = 50 + (next() % 2_500) as i32;
        }
        row
    });
    let mut market = MarketState::default();
    for resource in 0..6 {
        market.base_price[resource] = 25 + (next() % 75) as i32;
        market.spread[resource] = (next() % 21) as i32 - 10;
    }
    let nuke = std::array::from_fn(|slot| CanonicalNukeEmbargo {
        nuke_stamp: if slot < 4 && next() % 5 == 0 {
            100 + (next() % 700) as i32
        } else {
            0
        },
        nukes_used: (next() % 3) as i32,
    });
    NaturalWorld {
        production,
        econ,
        market,
        nuke,
    }
}

fn generated_facts(world: &NaturalWorld, seed: u32, seat: usize, generation: u32) -> CompleteFacts {
    let mut value = seed
        .wrapping_add((seat as u32 + 1).wrapping_mul(0x85eb_ca6b))
        .wrapping_add(generation.wrapping_mul(0xc2b2_ae35));
    let mut next = || {
        value = value.wrapping_mul(22_695_477).wrapping_add(1);
        value
    };
    let mut available = [false; 6];
    for value in &mut available {
        *value = next() % 5 != 0;
    }
    let mut nuke = NukeEmbargoInputs {
        who: seat as i32,
        active: [true, true, true, true, false, false, false, false],
        owners: world.nuke,
        frame: 4_000 + generation as i32 * 150,
        world_nukes: (next() % 4) as i32,
        rules: NukeEmbargoRules::shipped(),
        ..NukeEmbargoInputs::default()
    };
    nuke.relations[0][1] = 2;
    nuke.relations[1][0] = 2;
    nuke.relations[2][3] = 2;
    nuke.relations[3][2] = 2;
    CompleteFacts {
        who: seat,
        nubian: seat == 0,
        commerce_research: true,
        market_type_count: 1,
        buildings: vec![MarketBuildingRow {
            object_flags: 0x0801,
            owner: seat as i8,
        }],
        starting_resources: 0,
        type_available: available,
        quote_gates: MarketPriceGates {
            nubian: seat == 0,
            amber: seat == 1,
            super_market: seat == 2,
            ctw_stacks: (next() % 3) as u8,
            russian: seat == 3,
            age: (next() % 6) as i32,
        },
        nuke,
    }
}

fn execute_generation(
    world: &mut NaturalWorld,
    seed: u32,
    generation: u32,
) -> Vec<leader_market_speculation_complete::CompleteMarketSpeculationReceipt> {
    let rules = EconRules::shipped();
    let mut receipts = Vec::new();
    for seat in 0..4 {
        let facts = generated_facts(world, seed, seat, generation);
        let mut host = CanonicalCompleteHost {
            production: &mut world.production[seat],
            econ: &mut world.econ[seat],
            market: &mut world.market,
            rules: &rules,
            facts,
            embargo_effects: 0,
        };
        receipts.push(execute_market_speculation_complete(&mut host));
    }
    receipts
}

fn save_restore(world: &NaturalWorld, seed: u32) -> NaturalWorld {
    let mut sim = Sim::new(u64::from(seed), 4);
    for seat in 0..4 {
        sim.leaders[seat].econ = world.econ[seat];
    }
    sim.market = world.market;
    let loaded = load_sim(&save_sim(&sim).unwrap()).unwrap();
    NaturalWorld {
        production: std::array::from_fn(|seat| {
            CanonicalProductionEconomy::from_save_extension_bytes(
                &world.production[seat].save_extension_bytes(),
            )
            .unwrap()
        }),
        econ: std::array::from_fn(|seat| loaded.leaders[seat].econ),
        market: loaded.market,
        nuke: table_from_extension_bytes(&table_extension_bytes(&world.nuke)).unwrap(),
    }
}

#[test]
fn full_parent_four_seat_multi_seed_save_resume_matches_uninterrupted() {
    for seed in [0x49, 0x51a7_11ce, 0xd06f_00d5] {
        let mut direct = generated_world(seed);
        execute_generation(&mut direct, seed, 0);
        let mut resumed = save_restore(&direct, seed);
        let direct_receipts = execute_generation(&mut direct, seed, 1);
        let resumed_receipts = execute_generation(&mut resumed, seed, 1);
        assert_eq!(resumed_receipts, direct_receipts, "seed={seed:#x}");
        assert_eq!(resumed, direct, "seed={seed:#x}");
    }
}
