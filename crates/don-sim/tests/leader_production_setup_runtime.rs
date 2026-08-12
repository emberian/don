// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../src/systems/leader_production_setup_runtime.rs"]
mod leader_production_setup_runtime;

use leader_production_setup_runtime::{
    decode_income, decode_planning_rate, decode_resource_cap, encode_income, encode_planning_rate,
    encode_resource_cap, execute_production_ai_rate_cohort, get_mod_resource_cap,
    CanonicalPlanningRates, ModResourceCapInputs, PlanningRateCodecError, ProductionAiRateInputs,
    ProductionAiRateState, RESOURCE_COUNT,
};

#[test]
fn encrypted_rows_and_decoded_save_projection_are_distinct_and_exact() {
    let rates = CanonicalPlanningRates {
        values: [i32::MIN, -31, -1, 0, 30, i32::MAX],
    };
    let encrypted = rates.to_retail_encrypted();

    assert_eq!(encrypted[0], i32::MIN as u32 ^ 0x0007_3862);
    assert_eq!(encrypted[5], i32::MAX as u32 ^ 0x0007_3862);
    assert_eq!(
        CanonicalPlanningRates::from_retail_encrypted(encrypted),
        rates
    );

    let bytes = rates.save_extension_bytes();
    assert_eq!(&bytes[0..4], &i32::MIN.to_le_bytes());
    assert_eq!(&bytes[20..24], &i32::MAX.to_le_bytes());
    assert_eq!(
        CanonicalPlanningRates::from_save_extension_bytes(&bytes),
        Ok(rates)
    );
    assert_eq!(
        CanonicalPlanningRates::from_save_extension_bytes(&bytes[..23]),
        Err(PlanningRateCodecError::Length {
            expected: 24,
            actual: 23
        })
    );

    for value in [i32::MIN, -99_999_999, -1, 0, 1, 99_999_999, i32::MAX] {
        assert_eq!(decode_resource_cap(encode_resource_cap(value)), value);
        assert_eq!(decode_income(encode_income(value)), value);
        assert_eq!(decode_planning_rate(encode_planning_rate(value)), value);
    }
}

#[test]
fn complete_get_mod_resource_cap_child_matches_every_retail_gate() {
    let encrypted = encode_resource_cap(12_003);
    let base = ModResourceCapInputs {
        starting_resources: 0,
        match_flags_820: 0,
        leader_flags: 0,
        difficulty: 0,
    };

    assert_eq!(get_mod_resource_cap(base, encrypted), 6_001);
    assert_eq!(
        get_mod_resource_cap(
            ModResourceCapInputs {
                difficulty: 1,
                ..base
            },
            encrypted
        ),
        9_002
    );
    assert_eq!(
        get_mod_resource_cap(
            ModResourceCapInputs {
                difficulty: 2,
                ..base
            },
            encrypted
        ),
        12_003
    );
    assert_eq!(
        get_mod_resource_cap(
            ModResourceCapInputs {
                match_flags_820: 4,
                ..base
            },
            encrypted
        ),
        12_003
    );
    assert_eq!(
        get_mod_resource_cap(
            ModResourceCapInputs {
                leader_flags: 4,
                ..base
            },
            encrypted
        ),
        12_003
    );
    assert_eq!(
        get_mod_resource_cap(
            ModResourceCapInputs {
                starting_resources: 8,
                ..base
            },
            encode_resource_cap(i32::MAX)
        ),
        0
    );

    // Even the nominal *1.0 path is an i32 -> binary32 -> i32 round trip in retail.
    assert_eq!(
        get_mod_resource_cap(
            ModResourceCapInputs {
                difficulty: 2,
                ..base
            },
            encode_resource_cap(16_777_217)
        ),
        16_777_216
    );
    assert_eq!(
        get_mod_resource_cap(
            ModResourceCapInputs {
                difficulty: 2,
                ..base
            },
            encode_resource_cap(i32::MAX)
        ),
        i32::MIN
    );
}

#[test]
fn rate_cohort_writes_only_its_exact_setup_fields() {
    let mut state = ProductionAiRateState {
        econ_flags: [0x40, 0x20, 0x10, 0x08, 0x04, 0x02],
        base_rate: [91, 92, 93, 94, 95, 96],
        planning_rates: CanonicalPlanningRates {
            values: [70, 71, 72, 73, 74, 75],
        },
        worst_good: 5,
        best_good: 4,
        shortages: 123,
    };
    let input = ProductionAiRateInputs {
        cap: ModResourceCapInputs {
            starting_resources: 0,
            match_flags_820: 4,
            leader_flags: 0,
            difficulty: 0,
        },
        encrypted_resource_caps: [1600, 640, 800, 3200, 480, 960].map(encode_resource_cap),
        encrypted_income: [1440, 800, 640, 4000, 960, -33].map(encode_income),
        type_available: [true, true, false, true, true, true],
    };

    let receipt = execute_production_ai_rate_cohort(&mut state, input);

    assert!(!receipt.returned_for_starting_resources);
    assert_eq!(receipt.written_mask, 0x3f);
    assert_eq!(receipt.modified_caps, [1600, 640, 800, 3200, 480, 960]);
    assert_eq!(receipt.decoded_income, [1440, 800, 640, 4000, 960, -33]);
    assert_eq!(receipt.capped_income, [1440, 640, 640, 3200, 480, -33]);
    assert_eq!(state.base_rate, [0; RESOURCE_COUNT]);
    assert_eq!(state.planning_rates.values, [90, 40, 40, 200, 30, -2]);
    assert_eq!(state.worst_good, 5);
    assert_eq!(state.best_good, 3);
    assert_eq!(state.shortages, 1);
    assert_eq!(state.econ_flags, [0x40, 0x20, 0x10, 0x08, 0x04, 0x03]);
    assert_eq!(
        receipt.encrypted_rate_writes,
        state.planning_rates.to_retail_encrypted()
    );
}

#[test]
fn starting_resources_eight_preserves_stale_rates_and_base_rate() {
    let before_rates = CanonicalPlanningRates {
        values: [101, 102, 103, 104, 105, 106],
    };
    let before_base = [11, 12, 13, 14, 15, 16];
    let mut state = ProductionAiRateState {
        econ_flags: [0, 1, 2, 3, 4, 5],
        base_rate: before_base,
        planning_rates: before_rates,
        worst_good: 5,
        best_good: 4,
        shortages: 3,
    };
    let receipt = execute_production_ai_rate_cohort(
        &mut state,
        ProductionAiRateInputs {
            cap: ModResourceCapInputs {
                starting_resources: 8,
                ..ModResourceCapInputs::default()
            },
            encrypted_resource_caps: [u32::MAX; RESOURCE_COUNT],
            encrypted_income: [u32::MAX; RESOURCE_COUNT],
            type_available: [true; RESOURCE_COUNT],
        },
    );

    assert!(receipt.returned_for_starting_resources);
    assert_eq!(receipt.written_mask, 0);
    assert_eq!(receipt.encrypted_rate_writes, [0; RESOURCE_COUNT]);
    assert_eq!(state.planning_rates, before_rates);
    assert_eq!(state.base_rate, before_base);
    assert_eq!(state.econ_flags, [8, 9, 10, 11, 12, 13]);
    assert_eq!(
        (state.worst_good, state.best_good, state.shortages),
        (0, 0, 0)
    );
}

fn natural_inputs(seed: u32, seat: usize, generation: u32) -> ProductionAiRateInputs {
    let mut x = seed
        .wrapping_mul(0x9e37_79b9)
        .wrapping_add((seat as u32 + 1).wrapping_mul(0x85eb_ca6b))
        .wrapping_add(generation.wrapping_mul(0xc2b2_ae35));
    let mut next = || {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        x
    };

    let mut caps = [0; RESOURCE_COUNT];
    let mut income = [0; RESOURCE_COUNT];
    let mut available = [false; RESOURCE_COUNT];
    for resource in 0..RESOURCE_COUNT {
        // Ordinary positive economy observations; this harness grants/spends no resources.
        caps[resource] = encode_resource_cap(480 + (next() % 4_800) as i32);
        income[resource] = encode_income(320 + (next() % 5_600) as i32);
        available[resource] = next() % 5 != 0;
    }

    ProductionAiRateInputs {
        cap: ModResourceCapInputs {
            starting_resources: 0,
            match_flags_820: if seat == 3 { 4 } else { 0 },
            leader_flags: if seat == 2 { 4 } else { 0 },
            difficulty: (seat % 3) as i32,
        },
        encrypted_resource_caps: caps,
        encrypted_income: income,
        type_available: available,
    }
}

#[test]
fn four_seat_multi_seed_decoded_save_resume_matches_uninterrupted() {
    for seed in [0x0000_0049, 0x51a7_11ce, 0xd06f_00d5] {
        let mut uninterrupted = [ProductionAiRateState::default(); 4];
        for (seat, state) in uninterrupted.iter_mut().enumerate() {
            execute_production_ai_rate_cohort(state, natural_inputs(seed, seat, 0));
        }

        let mut resumed = uninterrupted;
        for (source, restored) in uninterrupted.iter().zip(resumed.iter_mut()) {
            // This is the exact proposed canonical Leader-row save projection. Only decoded
            // rate state crosses the save boundary; no resource or completion shortcut does.
            restored.planning_rates = CanonicalPlanningRates::from_save_extension_bytes(
                &source.planning_rates.save_extension_bytes(),
            )
            .unwrap();
        }

        for seat in 0..4 {
            let next = natural_inputs(seed, seat, 1);
            let uninterrupted_receipt =
                execute_production_ai_rate_cohort(&mut uninterrupted[seat], next);
            let resumed_receipt = execute_production_ai_rate_cohort(&mut resumed[seat], next);
            assert_eq!(
                resumed_receipt, uninterrupted_receipt,
                "seed={seed:#x} seat={seat}"
            );
            assert_eq!(
                resumed[seat], uninterrupted[seat],
                "seed={seed:#x} seat={seat}"
            );
        }
    }
}
