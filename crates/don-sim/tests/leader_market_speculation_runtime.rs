// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../src/systems/leader_market_speculation_runtime.rs"]
mod leader_market_speculation_runtime;

use leader_market_speculation_runtime::{
    execute_market_speculation_opening, has_market, CanonicalProductionEconomy, HasMarketInputs,
    MarketBuildingRow, MarketSpeculationOpeningExit, MarketSpeculationOpeningInputs,
    ProductionEconomyCodecError, PRODUCTION_ECONOMY_BYTES, PRODUCTION_ECONOMY_VALUES,
    RESOURCE_COUNT,
};

const OWNER_2_MARKET: MarketBuildingRow = MarketBuildingRow {
    object_flags: 0x0801,
    owner: 2,
};

#[test]
fn production_economy_codec_pins_all_33_pdb_values_in_order() {
    let state = CanonicalProductionEconomy {
        econ: [1, 2, 3, 4, 5, 6],
        escrow: [11, 12, 13, 14, 15, 16],
        escrow_rate: [21, 22, 23, 24, 25, 26],
        tributes: [31, 32, 33, 34, 35, 36],
        base_rate: [41, 42, 43, 44, 45, 46],
        worst_good: -1,
        best_good: i32::MAX,
        shortages: i32::MIN,
    };
    let values = state.save_extension_values();
    assert_eq!(PRODUCTION_ECONOMY_VALUES, 33);
    assert_eq!(PRODUCTION_ECONOMY_BYTES, 132);
    assert_eq!(&values[0..6], &state.econ);
    assert_eq!(&values[6..12], &state.escrow);
    assert_eq!(&values[12..18], &state.escrow_rate);
    assert_eq!(&values[18..24], &state.tributes);
    assert_eq!(&values[24..30], &state.base_rate);
    assert_eq!(&values[30..33], &[-1, i32::MAX, i32::MIN]);
    assert_eq!(
        CanonicalProductionEconomy::from_save_extension_values(values),
        state
    );

    let bytes = state.save_extension_bytes();
    assert_eq!(&bytes[0..4], &1_i32.to_le_bytes());
    assert_eq!(&bytes[128..132], &i32::MIN.to_le_bytes());
    assert_eq!(
        CanonicalProductionEconomy::from_save_extension_bytes(&bytes),
        Ok(state)
    );
    assert_eq!(
        CanonicalProductionEconomy::from_save_extension_bytes(&bytes[..131]),
        Err(ProductionEconomyCodecError::Length {
            expected: 132,
            actual: 131,
        })
    );
}

#[test]
fn complete_has_market_child_obeys_type_gate_list_order_and_signed_owner() {
    let rows = [
        MarketBuildingRow {
            object_flags: 0x0800,
            owner: 2,
        },
        MarketBuildingRow {
            object_flags: 0x0001,
            owner: 2,
        },
        MarketBuildingRow {
            object_flags: 0x0801,
            owner: -1,
        },
        OWNER_2_MARKET,
        OWNER_2_MARKET,
    ];

    let (present, receipt) = has_market(HasMarketInputs {
        who: 2,
        market_type_count: 1,
        buildings: &rows,
    });
    assert!(present);
    assert_eq!(receipt.visited, 4);
    assert_eq!(receipt.found_at, Some(3));

    let (present, receipt) = has_market(HasMarketInputs {
        who: 2,
        market_type_count: 0,
        buildings: &rows,
    });
    assert!(!present);
    assert_eq!(receipt.visited, 0);
    assert_eq!(receipt.found_at, None);
}

fn admitted_inputs<'a>(buildings: &'a [MarketBuildingRow]) -> MarketSpeculationOpeningInputs<'a> {
    MarketSpeculationOpeningInputs {
        who: 2,
        nubian: false,
        commerce_research: true,
        market_type_count: 1,
        buildings,
        nuke_embargo: false,
        starting_resources: 0,
        type_available: [true; RESOURCE_COUNT],
        buckets: [300; RESOURCE_COUNT],
    }
}

#[test]
fn opening_preserves_retail_short_circuit_order_without_state_writes() {
    let market = [OWNER_2_MARKET];
    let baseline = CanonicalProductionEconomy {
        econ: [5001; RESOURCE_COUNT],
        ..CanonicalProductionEconomy::default()
    };

    let cases = [
        (
            MarketSpeculationOpeningInputs {
                nubian: false,
                commerce_research: false,
                ..admitted_inputs(&market)
            },
            MarketSpeculationOpeningExit::MissingUnlock,
            true,
            0,
        ),
        (
            MarketSpeculationOpeningInputs {
                market_type_count: 0,
                ..admitted_inputs(&market)
            },
            MarketSpeculationOpeningExit::MissingMarket,
            true,
            0,
        ),
        (
            MarketSpeculationOpeningInputs {
                nuke_embargo: true,
                ..admitted_inputs(&market)
            },
            MarketSpeculationOpeningExit::Embargoed,
            true,
            1,
        ),
        (
            MarketSpeculationOpeningInputs {
                starting_resources: 8,
                ..admitted_inputs(&market)
            },
            MarketSpeculationOpeningExit::StartingResourcesEight,
            true,
            1,
        ),
    ];

    for (inputs, exit, read_preq, market_visits) in cases {
        let mut state = baseline;
        let receipt = execute_market_speculation_opening(&mut state, inputs);
        assert_eq!(receipt.exit, exit);
        assert_eq!(receipt.read_commerce_research, read_preq);
        assert_eq!(receipt.market.visited, market_visits);
        assert_eq!(receipt.visited_mask, 0);
        assert_eq!(receipt.clamped_mask, 0);
        assert_eq!(state, baseline);
    }

    let mut state = baseline;
    let receipt = execute_market_speculation_opening(
        &mut state,
        MarketSpeculationOpeningInputs {
            nubian: true,
            commerce_research: false,
            ..admitted_inputs(&market)
        },
    );
    assert!(!receipt.read_commerce_research);
    assert_eq!(receipt.exit, MarketSpeculationOpeningExit::ReadyForSellPass);
}

#[test]
fn scarcity_pass_clamps_signed_econ_and_only_reads_available_buckets() {
    let market = [OWNER_2_MARKET];
    let mut state = CanonicalProductionEconomy {
        econ: [4000, 4001, i32::MAX, -1, 5000, 6000],
        escrow: [101, 102, 103, 104, 105, 106],
        escrow_rate: [201, 202, 203, 204, 205, 206],
        tributes: [301, 302, 303, 304, 305, 306],
        base_rate: [401, 402, 403, 404, 405, 406],
        worst_good: 5,
        best_good: 4,
        shortages: 3,
    };
    let unchanged_non_econ = (
        state.escrow,
        state.escrow_rate,
        state.tributes,
        state.base_rate,
    );

    let receipt = execute_market_speculation_opening(
        &mut state,
        MarketSpeculationOpeningInputs {
            type_available: [true, true, true, false, true, false],
            // Resource 3 would force scarcity 2, but unavailable rows are not read by retail.
            buckets: [250, 175, 210, 1, 99, 2],
            ..admitted_inputs(&market)
        },
    );

    assert_eq!(receipt.exit, MarketSpeculationOpeningExit::ReadyForSellPass);
    assert_eq!(receipt.visited_mask, 0b01_0111);
    assert_eq!(receipt.clamped_mask, 0b01_0110);
    assert_eq!(receipt.scarcity, 2);
    assert_eq!(state.econ, [4000, 2000, 2000, -1, 2000, 6000]);
    assert_eq!(
        (
            state.escrow,
            state.escrow_rate,
            state.tributes,
            state.base_rate
        ),
        unchanged_non_econ
    );
    assert_eq!(
        (state.worst_good, state.best_good, state.shortages),
        (5, 4, 3)
    );
}

fn seeded_state(seed: u32, seat: usize) -> CanonicalProductionEconomy {
    let mut value = seed.wrapping_add((seat as u32 + 1).wrapping_mul(0x9e37_79b9));
    let mut next = || {
        value = value.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        value as i32
    };
    let mut values = [0; PRODUCTION_ECONOMY_VALUES];
    for item in &mut values {
        *item = next();
    }
    // Natural planning targets around the retail 4000 clamp threshold.
    for item in &mut values[..RESOURCE_COUNT] {
        *item = item.unsigned_abs().wrapping_rem(6_001) as i32;
    }
    CanonicalProductionEconomy::from_save_extension_values(values)
}

fn seeded_inputs<'a>(
    seed: u32,
    seat: usize,
    generation: u32,
    market: &'a [MarketBuildingRow],
) -> MarketSpeculationOpeningInputs<'a> {
    let mut value = seed
        .wrapping_mul(0x85eb_ca6b)
        .wrapping_add(seat as u32)
        .wrapping_add(generation.wrapping_mul(0xc2b2_ae35));
    let mut next = || {
        value = value.wrapping_mul(22_695_477).wrapping_add(1);
        value
    };
    let mut buckets = [0; RESOURCE_COUNT];
    let mut available = [false; RESOURCE_COUNT];
    for resource in 0..RESOURCE_COUNT {
        buckets[resource] = 40 + (next() % 500) as i32;
        available[resource] = next() % 4 != 0;
    }
    MarketSpeculationOpeningInputs {
        who: seat as i32,
        nubian: seat == 0,
        commerce_research: true,
        market_type_count: 1,
        buildings: market,
        nuke_embargo: false,
        starting_resources: 0,
        type_available: available,
        buckets,
    }
}

#[test]
fn four_seat_multi_seed_full_owner_save_resume_matches_uninterrupted() {
    for seed in [0x49, 0xa11c_e555, 0xd06f_5eed] {
        let markets = [
            [MarketBuildingRow {
                object_flags: 0x0801,
                owner: 0,
            }],
            [MarketBuildingRow {
                object_flags: 0x0801,
                owner: 1,
            }],
            [MarketBuildingRow {
                object_flags: 0x0801,
                owner: 2,
            }],
            [MarketBuildingRow {
                object_flags: 0x0801,
                owner: 3,
            }],
        ];
        let mut uninterrupted: [CanonicalProductionEconomy; 4] =
            std::array::from_fn(|seat| seeded_state(seed, seat));
        for seat in 0..4 {
            execute_market_speculation_opening(
                &mut uninterrupted[seat],
                seeded_inputs(seed, seat, 0, &markets[seat]),
            );
        }

        let mut resumed = uninterrupted;
        for seat in 0..4 {
            resumed[seat] = CanonicalProductionEconomy::from_save_extension_bytes(
                &uninterrupted[seat].save_extension_bytes(),
            )
            .unwrap();
        }

        for seat in 0..4 {
            let inputs = seeded_inputs(seed, seat, 1, &markets[seat]);
            let direct_receipt =
                execute_market_speculation_opening(&mut uninterrupted[seat], inputs);
            let resumed_receipt = execute_market_speculation_opening(&mut resumed[seat], inputs);
            assert_eq!(
                resumed_receipt, direct_receipt,
                "seed={seed:#x} seat={seat}"
            );
            assert_eq!(
                resumed[seat], uninterrupted[seat],
                "seed={seed:#x} seat={seat}"
            );
        }
    }
}
