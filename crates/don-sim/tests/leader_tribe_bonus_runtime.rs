// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../src/systems/leader_market_speculation_runtime.rs"]
mod leader_market_speculation_runtime;
#[path = "../src/systems/leader_production_setup_runtime.rs"]
mod leader_production_setup_runtime;
#[path = "../src/systems/leader_tribe_bonus_runtime.rs"]
mod leader_tribe_bonus_runtime;

use leader_market_speculation_runtime::{
    execute_market_speculation_opening, CanonicalProductionEconomy, MarketBuildingRow,
    MarketSpeculationOpeningExit, MarketSpeculationOpeningInputs,
};
use leader_production_setup_runtime::{
    encode_income, encode_resource_cap, execute_production_ai_rate_cohort, ModResourceCapInputs,
    ProductionAiRateInputs, ProductionAiRateState,
};
use leader_tribe_bonus_runtime::{
    get_diff, has_tribe_bonus, table_extension_bytes, table_from_extension_bytes,
    CanonicalConquestRacialPowers, ConquestPowerCodecError, GetDiffExit, GetDiffInputs,
    TribeBonusExit, TribeBonusInputError, TribeBonusInputs, LEADER_COUNT,
};

fn bonus_inputs(powers: CanonicalConquestRacialPowers) -> TribeBonusInputs {
    TribeBonusInputs {
        no_nation_powers: false,
        victory: 1,
        city_num: 1,
        tribe: 4,
        leader_flags2: 0,
        conquest_racial_powers: powers,
        tribe_default_bonus: Some(4),
    }
}

#[test]
fn owner_codec_is_the_exact_three_payload_bytes_not_bitmask_metadata() {
    let mut powers = CanonicalConquestRacialPowers::default();
    assert!(powers.set(0, true));
    assert!(powers.set(7, true));
    assert!(powers.set(8, true));
    assert!(powers.set(23, true));
    assert!(!powers.set(24, true));
    assert_eq!(powers.payload(), [0x81, 0x01, 0x80]);
    assert_eq!(powers.decoded_mask(), 0x0080_0181);
    assert_eq!(
        CanonicalConquestRacialPowers::from_decoded_mask(powers.decoded_mask()),
        Ok(powers)
    );
    assert!(matches!(
        CanonicalConquestRacialPowers::from_decoded_mask(0x0100_0000),
        Err(ConquestPowerCodecError::BitsOutsideRetailRange(0x0100_0000))
    ));

    let rows: [CanonicalConquestRacialPowers; LEADER_COUNT] = std::array::from_fn(|slot| {
        CanonicalConquestRacialPowers::from_payload([
            slot as u8,
            (slot as u8).wrapping_mul(17),
            0x80 >> slot,
        ])
    });
    let bytes = table_extension_bytes(&rows);
    assert_eq!(bytes.len(), 24);
    assert_eq!(&bytes[0..3], &[0, 0, 0x80]);
    assert_eq!(&bytes[21..24], &[7, 119, 1]);
    assert_eq!(table_from_extension_bytes(&bytes), Ok(rows));
    assert!(table_from_extension_bytes(&bytes[..23]).is_err());
    assert!(CanonicalConquestRacialPowers::from_save_extension_bytes(&[0; 4]).is_err());
}

#[test]
fn complete_tribe_child_preserves_every_short_circuit_and_fallback_read() {
    let empty = CanonicalConquestRacialPowers::default();

    let (granted, receipt) = has_tribe_bonus(
        TribeBonusInputs {
            no_nation_powers: true,
            victory: 0,
            city_num: 0,
            tribe: -1,
            leader_flags2: 0,
            conquest_racial_powers: empty,
            tribe_default_bonus: None,
        },
        4,
    )
    .unwrap();
    assert!(!granted);
    assert_eq!(receipt.exit, TribeBonusExit::NationPowersDisabled);
    assert!(!receipt.read_victory);
    assert!(!receipt.read_city_num);
    assert!(!receipt.read_tribe);

    let (granted, receipt) = has_tribe_bonus(
        TribeBonusInputs {
            no_nation_powers: false,
            victory: 0,
            city_num: 0,
            tribe: 4,
            leader_flags2: 0,
            conquest_racial_powers: empty,
            tribe_default_bonus: Some(4),
        },
        4,
    )
    .unwrap();
    assert!(!granted);
    assert_eq!(receipt.exit, TribeBonusExit::NoCityBeforeVictory);
    assert!(receipt.read_victory && receipt.read_city_num);
    assert!(!receipt.read_tribe && !receipt.read_conquest_payload);

    let (granted, receipt) = has_tribe_bonus(
        TribeBonusInputs {
            victory: 1,
            city_num: i32::MIN,
            tribe: -1,
            ..bonus_inputs(empty)
        },
        4,
    )
    .unwrap();
    assert!(!granted);
    assert_eq!(receipt.exit, TribeBonusExit::NoTribe);
    assert!(!receipt.read_city_num);
    assert!(receipt.read_tribe && !receipt.read_conquest_payload);

    let mut conquest = empty;
    conquest.set(19, true);
    let (granted, receipt) = has_tribe_bonus(
        TribeBonusInputs {
            leader_flags2: 0x40,
            tribe_default_bonus: None,
            ..bonus_inputs(conquest)
        },
        19,
    )
    .unwrap();
    assert!(granted);
    assert_eq!(receipt.exit, TribeBonusExit::GrantedByConquest);
    assert!(receipt.read_conquest_payload);
    assert!(!receipt.read_leader_flags2 && !receipt.read_tribe_default);

    let (granted, receipt) = has_tribe_bonus(
        TribeBonusInputs {
            leader_flags2: 0x40,
            tribe_default_bonus: None,
            ..bonus_inputs(empty)
        },
        4,
    )
    .unwrap();
    assert!(!granted);
    assert_eq!(receipt.exit, TribeBonusExit::TribeFallbackSuppressed);
    assert!(receipt.read_leader_flags2);
    assert!(!receipt.read_tribe_default);

    let (granted, receipt) = has_tribe_bonus(bonus_inputs(empty), 4).unwrap();
    assert!(granted);
    assert_eq!(receipt.exit, TribeBonusExit::GrantedByTribe);
    assert!(receipt.read_tribe_default);

    let (granted, receipt) = has_tribe_bonus(bonus_inputs(empty), 13).unwrap();
    assert!(!granted);
    assert_eq!(receipt.exit, TribeBonusExit::NotGranted);
    assert!(matches!(
        has_tribe_bonus(bonus_inputs(empty), 24),
        Err(TribeBonusInputError::InvalidBonus(24))
    ));
    assert!(matches!(
        has_tribe_bonus(
            TribeBonusInputs {
                tribe_default_bonus: None,
                ..bonus_inputs(empty)
            },
            4
        ),
        Err(TribeBonusInputError::MissingTribeDefault { tribe: 4 })
    ));
}

#[test]
fn complete_get_diff_child_matches_the_retail_flag_matrix_and_raw_dword_return() {
    let base = GetDiffInputs {
        match_flags_820: 0,
        match_flags_821: 0x10,
        match_flags_822: 0,
        global_difficulty: 2,
        multi_diff: 1,
    };
    let (value, receipt) = get_diff(base);
    assert_eq!(value, 1);
    assert_eq!(receipt.exit, GetDiffExit::LeaderDifficulty);
    assert!(receipt.read_multi_diff);

    for inputs in [
        GetDiffInputs {
            match_flags_821: 0,
            multi_diff: 99,
            ..base
        },
        GetDiffInputs {
            match_flags_822: 2,
            multi_diff: 99,
            ..base
        },
    ] {
        let (value, receipt) = get_diff(inputs);
        assert_eq!(value, 2);
        assert_eq!(receipt.exit, GetDiffExit::GlobalFlagFallback);
        assert!(!receipt.read_multi_diff);
    }

    let (value, receipt) = get_diff(GetDiffInputs {
        multi_diff: -1,
        ..base
    });
    assert_eq!(value, 2);
    assert_eq!(receipt.exit, GetDiffExit::NegativeLeaderFallback);
    assert!(receipt.read_multi_diff);

    let (value, receipt) = get_diff(GetDiffInputs {
        match_flags_820: 4,
        match_flags_821: 0,
        match_flags_822: 2,
        multi_diff: -7,
        ..base
    });
    assert_eq!(value, (-7_i32) as u32);
    assert_eq!(receipt.exit, GetDiffExit::ForcedLeaderDifficulty);
    assert!(receipt.read_multi_diff);
}

#[test]
fn exact_children_drive_the_existing_rate_and_market_parent_gates() {
    let (difficulty, _) = get_diff(GetDiffInputs {
        match_flags_820: 0,
        match_flags_821: 0x10,
        match_flags_822: 0,
        global_difficulty: 2,
        multi_diff: 0,
    });
    let mut rate_state = ProductionAiRateState::default();
    let receipt = execute_production_ai_rate_cohort(
        &mut rate_state,
        ProductionAiRateInputs {
            cap: ModResourceCapInputs {
                starting_resources: 0,
                match_flags_820: 0,
                leader_flags: 0,
                difficulty: difficulty as i32,
            },
            encrypted_resource_caps: [encode_resource_cap(1_600); 6],
            encrypted_income: [encode_income(1_600); 6],
            type_available: [true; 6],
        },
    );
    assert_eq!(receipt.modified_caps, [800; 6]);
    assert_eq!(rate_state.planning_rates.values, [50; 6]);

    let mut powers = CanonicalConquestRacialPowers::default();
    powers.set(4, true);
    let (nubian, bonus_receipt) = has_tribe_bonus(
        TribeBonusInputs {
            tribe_default_bonus: Some(13),
            ..bonus_inputs(powers)
        },
        4,
    )
    .unwrap();
    assert_eq!(bonus_receipt.exit, TribeBonusExit::GrantedByConquest);

    let buildings = [MarketBuildingRow {
        object_flags: 0x0801,
        owner: 2,
    }];
    let mut production = CanonicalProductionEconomy::default();
    let opening = execute_market_speculation_opening(
        &mut production,
        MarketSpeculationOpeningInputs {
            who: 2,
            nubian,
            commerce_research: false,
            market_type_count: 1,
            buildings: &buildings,
            nuke_embargo: false,
            starting_resources: 0,
            type_available: [true; 6],
            buckets: [1_000; 6],
        },
    );
    assert_eq!(opening.exit, MarketSpeculationOpeningExit::ReadyForSellPass);
    assert!(!opening.read_commerce_research);
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NaturalSeat {
    powers: CanonicalConquestRacialPowers,
    victory: u8,
    city_num: i32,
    tribe: i32,
    default_bonus: i32,
    flags2: u32,
    multi_diff: i32,
}

fn generated_seats(seed: u32) -> [NaturalSeat; 4] {
    let mut value = seed.wrapping_add(0xa341_316c);
    let mut next = || {
        value ^= value << 13;
        value ^= value >> 17;
        value ^= value << 5;
        value
    };
    std::array::from_fn(|slot| {
        let mut powers = CanonicalConquestRacialPowers::default();
        powers.set((next() as usize) % 24, true);
        powers.set((next() as usize) % 24, true);
        NaturalSeat {
            powers,
            victory: ((next() >> 7) & 1) as u8,
            city_num: 1 + (next() % 5) as i32,
            tribe: slot as i32,
            default_bonus: (seed.wrapping_add((slot as u32).wrapping_mul(7)) % 24) as i32,
            flags2: if next() & 7 == 0 { 0x40 } else { 0 },
            multi_diff: (next() % 4) as i32,
        }
    })
}

fn future_observations(seats: &[NaturalSeat; 4], seed: u32) -> Vec<(bool, u32)> {
    let mut observations = Vec::new();
    for frame in 0..24_u32 {
        for (slot, seat) in seats.iter().enumerate() {
            let bonus = (seed
                .wrapping_add(frame.wrapping_mul(5))
                .wrapping_add((slot as u32).wrapping_mul(11))
                % 24) as i32;
            let bonus_value = has_tribe_bonus(
                TribeBonusInputs {
                    no_nation_powers: false,
                    victory: seat.victory,
                    city_num: seat.city_num,
                    tribe: seat.tribe,
                    leader_flags2: seat.flags2,
                    conquest_racial_powers: seat.powers,
                    tribe_default_bonus: Some(seat.default_bonus),
                },
                bonus,
            )
            .unwrap()
            .0;
            let difficulty = get_diff(GetDiffInputs {
                match_flags_820: 0,
                match_flags_821: if frame & 1 == 0 { 0x10 } else { 0 },
                match_flags_822: if frame % 7 == 0 { 2 } else { 0 },
                global_difficulty: (seed.wrapping_add(frame) % 4) as u8,
                multi_diff: seat.multi_diff,
            })
            .0;
            observations.push((bonus_value, difficulty));
        }
    }
    observations
}

#[test]
fn four_seat_multi_seed_save_resume_preserves_future_setup_queries() {
    for seed in [1, 0x1020_3040, 0xdead_beef, 0xffff_fffb] {
        let seats = generated_seats(seed);
        let uninterrupted = future_observations(&seats, seed);

        let mut owner_rows = [CanonicalConquestRacialPowers::default(); LEADER_COUNT];
        for slot in 0..4 {
            owner_rows[slot] = seats[slot].powers;
        }
        let bytes = table_extension_bytes(&owner_rows);
        let resumed_rows = table_from_extension_bytes(&bytes).unwrap();
        let mut resumed = seats.clone();
        for slot in 0..4 {
            resumed[slot].powers = resumed_rows[slot];
        }

        assert_eq!(future_observations(&resumed, seed), uninterrupted);
    }
}
