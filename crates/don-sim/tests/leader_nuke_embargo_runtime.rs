// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../src/systems/leader_market_speculation_runtime.rs"]
mod leader_market_speculation_runtime;
#[path = "../src/systems/leader_nuke_embargo_runtime.rs"]
mod leader_nuke_embargo_runtime;

use leader_market_speculation_runtime::{
    execute_market_speculation_opening, CanonicalProductionEconomy, MarketBuildingRow,
    MarketSpeculationOpeningExit, MarketSpeculationOpeningInputs,
};
use leader_nuke_embargo_runtime::{
    get_my_nuke_embargo, get_nuke_embargo, table_extension_bytes, table_from_extension_bytes,
    CanonicalNukeEmbargo, MyNukeEmbargoInputs, NukeEmbargoCall, NukeEmbargoInputs,
    NukeEmbargoRules, LEADER_COUNT,
};

#[test]
fn owner_codec_is_exactly_two_plain_i32_values_per_leader() {
    let rows: [CanonicalNukeEmbargo; LEADER_COUNT] =
        std::array::from_fn(|slot| CanonicalNukeEmbargo {
            nuke_stamp: i32::MIN.wrapping_add(slot as i32),
            nukes_used: i32::MAX.wrapping_sub(slot as i32),
        });
    let bytes = table_extension_bytes(&rows);
    assert_eq!(bytes.len(), 64);
    assert_eq!(&bytes[0..4], &i32::MIN.to_le_bytes());
    assert_eq!(&bytes[4..8], &i32::MAX.to_le_bytes());
    assert_eq!(&bytes[56..60], &rows[7].nuke_stamp.to_le_bytes());
    assert_eq!(&bytes[60..64], &rows[7].nukes_used.to_le_bytes());
    assert_eq!(table_from_extension_bytes(&bytes), Ok(rows));
    assert!(table_from_extension_bytes(&bytes[..63]).is_err());

    for row in rows {
        assert_eq!(
            CanonicalNukeEmbargo::from_extension_bytes(&row.extension_bytes()),
            Ok(row)
        );
    }
}

#[test]
fn complete_personal_child_preserves_early_gates_wrapping_and_signed_clamp() {
    let rules = NukeEmbargoRules::shipped();
    let (value, receipt) = get_my_nuke_embargo(MyNukeEmbargoInputs {
        state: CanonicalNukeEmbargo {
            nuke_stamp: 0,
            nukes_used: 99,
        },
        has_anti_nuke_wonder: true,
        world_nukes: 99,
        frame: -1,
        rules,
    });
    assert_eq!(value, 0);
    assert!(!receipt.read_wonder);

    let (value, receipt) = get_my_nuke_embargo(MyNukeEmbargoInputs {
        state: CanonicalNukeEmbargo {
            nuke_stamp: 100,
            nukes_used: 2,
        },
        has_anti_nuke_wonder: true,
        world_nukes: 7,
        frame: 1000,
        rules,
    });
    assert_eq!(value, 0);
    assert!(receipt.read_wonder);
    assert_eq!(receipt.raw_timer, None);

    let (value, receipt) = get_my_nuke_embargo(MyNukeEmbargoInputs {
        state: CanonicalNukeEmbargo {
            nuke_stamp: 100,
            nukes_used: 2,
        },
        has_anti_nuke_wonder: false,
        world_nukes: 7,
        frame: 1000,
        rules,
    });
    assert_eq!(value, 1800);
    assert_eq!(receipt.raw_timer, Some(1800));

    let (value, receipt) = get_my_nuke_embargo(MyNukeEmbargoInputs {
        state: CanonicalNukeEmbargo {
            nuke_stamp: i32::MAX,
            nukes_used: i32::MAX,
        },
        has_anti_nuke_wonder: false,
        world_nukes: i32::MAX,
        frame: i32::MIN,
        rules: NukeEmbargoRules {
            base: i32::MAX,
            nation: i32::MAX,
            world: i32::MAX,
        },
    });
    let expected = i32::MAX
        .wrapping_mul(i32::MAX)
        .wrapping_add(i32::MAX.wrapping_mul(i32::MAX))
        .wrapping_sub(i32::MIN)
        .wrapping_add(i32::MAX)
        .wrapping_add(i32::MAX);
    assert_eq!(receipt.raw_timer, Some(expected));
    assert_eq!(value, expected.max(0));
}

fn allied_pair(inputs: &mut NukeEmbargoInputs, a: usize, b: usize) {
    inputs.relations[a][b] = 2;
    inputs.relations[b][a] = 2;
}

#[test]
fn complete_global_child_takes_max_over_self_and_mutual_active_allies_in_slot_order() {
    let mut inputs = NukeEmbargoInputs {
        who: 1,
        active: [true, true, true, true, false, false, false, false],
        frame: 1000,
        rules: NukeEmbargoRules::shipped(),
        ..NukeEmbargoInputs::default()
    };
    allied_pair(&mut inputs, 1, 2);
    inputs.relations[1][3] = 2;
    inputs.owners[1] = CanonicalNukeEmbargo {
        nuke_stamp: 100,
        nukes_used: 1,
    };
    inputs.owners[2] = CanonicalNukeEmbargo {
        nuke_stamp: 200,
        nukes_used: 3,
    };
    inputs.owners[3] = CanonicalNukeEmbargo {
        nuke_stamp: 300,
        nukes_used: 9,
    };

    let (value, receipt) = get_nuke_embargo(&inputs).unwrap();
    assert_eq!(value, 2800);
    assert_eq!(receipt.visited_mask, 0xff);
    assert_eq!(receipt.admitted_mask, 0b0000_0110);
    assert_eq!(receipt.max_slot, Some(2));
    assert!(matches!(
        receipt.calls[0],
        NukeEmbargoCall::OuterWonder {
            slot: 1,
            present: false
        }
    ));
    assert!(receipt.calls.iter().any(|call| matches!(
        call,
        NukeEmbargoCall::IsAlly {
            slot: 3,
            allied: false
        }
    )));

    inputs.has_anti_nuke_wonder[1] = true;
    let (value, receipt) = get_nuke_embargo(&inputs).unwrap();
    assert_eq!(value, 0);
    assert_eq!(receipt.visited_mask, 0);
    assert_eq!(receipt.calls.len(), 1);
    assert!(get_nuke_embargo(&NukeEmbargoInputs { who: -1, ..inputs }).is_none());
}

#[test]
fn exact_child_result_gates_market_speculation_before_scarcity_mutation() {
    let mut embargo = NukeEmbargoInputs {
        who: 2,
        active: [false, false, true, false, false, false, false, false],
        frame: 1_000,
        rules: NukeEmbargoRules::shipped(),
        ..NukeEmbargoInputs::default()
    };
    embargo.owners[2] = CanonicalNukeEmbargo {
        nuke_stamp: 100,
        nukes_used: 2,
    };
    let (embargo_value, _) = get_nuke_embargo(&embargo).unwrap();
    assert_eq!(embargo_value, 1800);

    let market = [MarketBuildingRow {
        object_flags: 0x0801,
        owner: 2,
    }];
    let mut production = CanonicalProductionEconomy {
        econ: [5001; 6],
        ..CanonicalProductionEconomy::default()
    };
    let before = production;
    let receipt = execute_market_speculation_opening(
        &mut production,
        MarketSpeculationOpeningInputs {
            who: 2,
            nubian: true,
            commerce_research: false,
            market_type_count: 1,
            buildings: &market,
            nuke_embargo: embargo_value != 0,
            starting_resources: 0,
            type_available: [true; 6],
            buckets: [50; 6],
        },
    );
    assert_eq!(receipt.exit, MarketSpeculationOpeningExit::Embargoed);
    assert_eq!(production, before);
}

fn natural_inputs(seed: u32, who: usize, frame: i32) -> NukeEmbargoInputs {
    let mut value = seed.wrapping_add(0x9e37_79b9);
    let mut next = || {
        value = value.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        value
    };
    let mut inputs = NukeEmbargoInputs {
        who: who as i32,
        active: [true, true, true, true, false, false, false, false],
        frame,
        world_nukes: (next() % 6) as i32,
        rules: NukeEmbargoRules::shipped(),
        ..NukeEmbargoInputs::default()
    };
    allied_pair(&mut inputs, 0, 1);
    allied_pair(&mut inputs, 2, 3);
    for slot in 0..4 {
        inputs.owners[slot] = CanonicalNukeEmbargo {
            nuke_stamp: 100 + (next() % 2_400) as i32,
            nukes_used: (next() % 5) as i32,
        };
        inputs.has_anti_nuke_wonder[slot] = next() % 11 == 0;
    }
    inputs
}

#[test]
fn four_seat_multi_seed_owner_save_resume_preserves_future_embargo_behavior() {
    for seed in [0x49, 0x51a7_11ce, 0xd06f_00d5] {
        let direct: [NukeEmbargoInputs; 4] =
            std::array::from_fn(|who| natural_inputs(seed, who, 1_200));
        let mut resumed = direct;
        let saved = table_extension_bytes(&direct[0].owners);
        let restored_owners = table_from_extension_bytes(&saved).unwrap();
        for input in &mut resumed {
            input.owners = restored_owners;
            input.frame = 1_350;
        }

        for who in 0..4 {
            let mut uninterrupted = direct[who];
            uninterrupted.frame = 1_350;
            assert_eq!(
                get_nuke_embargo(&resumed[who]),
                get_nuke_embargo(&uninterrupted),
                "seed={seed:#x} seat={who}"
            );
        }
    }
}
