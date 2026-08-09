// SPDX-License-Identifier: GPL-3.0-or-later

//! Exact control-flow and RNG coverage for `TerrainGroups::place_all` selection.

use don_sim::rng::Random;
use don_sim::systems::terrain_groups::{TerrainGroup, TerrainGroupSelectionError, TerrainGroups};

fn group(
    group_type: i32,
    chance: i32,
    grouping: i32,
    min_clumps: i32,
    max_clumps: i32,
) -> TerrainGroup {
    TerrainGroup {
        group_type,
        chance,
        grouping,
        min_clumps,
        max_clumps,
        ..TerrainGroup::default()
    }
}

#[test]
fn grouped_percentile_carries_signed_remainder_and_short_circuits() {
    let groups = TerrainGroups {
        groups: vec![
            group(4, 100, 7, 2, 2),
            // The first selection leaves a negative remainder.  A zero chance
            // with previous_selected=true subtracts zero and selects again.
            group(4, 0, 7, 3, 3),
            // A nonzero chance now takes retail's reject-without-subtraction arm.
            group(4, 1, 7, 4, 4),
            // previous_selected was cleared, so this also rejects immediately.
            group(4, 100, 7, 5, 5),
            // grouping zero forces a fresh draw for every row.
            group(4, 50, 0, 6, 6),
        ],
        ..TerrainGroups::default()
    };
    let mut random = Random::new(0x1234_5678);

    let receipt = groups.select_groups(&mut random).unwrap();

    assert_eq!(receipt.chance_draws, 2);
    assert_eq!(receipt.clump_draws, 0);
    assert_eq!(
        receipt
            .groups
            .iter()
            .map(|row| (row.selected, row.clumps))
            .collect::<Vec<_>>(),
        vec![(true, 2), (true, 3), (false, 0), (false, 0), (false, 0)]
    );
    assert_eq!(receipt.raw_clumps_by_type, [5, 0, 0, 0, 0]);
    assert_eq!(
        receipt.normalized_clumps_by_type,
        [2, i32::MAX, i32::MAX, i32::MAX, i32::MAX]
    );

    let mut control = Random::new(0x1234_5678);
    control.get(0, 0xffff);
    control.get(0, 0xffff);
    assert_eq!(receipt.rng_state_after, control.state());
}

#[test]
fn chance_is_strict_and_clump_range_is_inclusive() {
    // The first retail draw from this seed is 10102; percentile is exactly 2.
    let rejected = TerrainGroups {
        groups: vec![group(4, 2, 0, 9, 9)],
        ..TerrainGroups::default()
    };
    let mut rejected_rng = Random::new(0x1234_5678);
    let rejected_receipt = rejected.select_groups(&mut rejected_rng).unwrap();
    assert!(!rejected_receipt.groups[0].selected);

    let selected = TerrainGroups {
        groups: vec![group(4, 3, 0, 2, 4)],
        ..TerrainGroups::default()
    };
    let mut selected_rng = Random::new(0x1234_5678);
    let selected_receipt = selected.select_groups(&mut selected_rng).unwrap();
    assert_eq!(selected_receipt.chance_draws, 1);
    assert_eq!(selected_receipt.clump_draws, 1);
    // Second draw is 24169; 24169 % ((4 - 2) + 1) + 2 == 3.
    assert_eq!(selected_receipt.groups[0].clumps, 3);
}

#[test]
fn five_type_totals_use_wrapping_add_then_half_and_evenize() {
    let groups = TerrainGroups {
        groups: vec![
            group(4, 100, 0, 0, 0),
            group(5, 100, 0, 1, 1),
            group(6, 100, 0, 2, 2),
            group(7, 100, 0, 6, 6),
            group(8, 100, 0, 9, 9),
        ],
        ..TerrainGroups::default()
    };
    let mut random = Random::new(77);

    let receipt = groups.select_groups(&mut random).unwrap();

    assert_eq!(receipt.chance_draws, 5);
    assert!(receipt.groups.iter().all(|row| row.selected));
    assert_eq!(receipt.raw_clumps_by_type, [0, 1, 2, 6, 9]);
    assert_eq!(
        receipt.normalized_clumps_by_type,
        [i32::MAX, i32::MAX, 0, 2, 4]
    );
}

#[test]
fn invalid_native_table_index_and_overflowing_divisor_fail_before_rng() {
    let bad_type = TerrainGroups {
        groups: vec![group(3, 100, 0, 1, 1)],
        ..TerrainGroups::default()
    };
    let mut random = Random::new(91);
    let before = random.state();
    assert_eq!(
        bad_type.select_groups(&mut random),
        Err(TerrainGroupSelectionError::UnsupportedGroupType {
            group_index: 0,
            group_type: 3,
        })
    );
    assert_eq!(random.state(), before);

    let bad_span = TerrainGroups {
        groups: vec![group(4, 100, 0, i32::MIN, i32::MAX)],
        ..TerrainGroups::default()
    };
    assert_eq!(
        bad_span.select_groups(&mut random),
        Err(TerrainGroupSelectionError::InvalidClumpSpan {
            group_index: 0,
            min_clumps: i32::MIN,
            max_clumps: i32::MAX,
        })
    );
    assert_eq!(random.state(), before);
}
