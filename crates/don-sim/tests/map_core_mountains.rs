// SPDX-License-Identifier: GPL-3.0-or-later

//! Mutation-sensitive checks for the retail mountain range list transaction.

use don_sim::rng::Random;
use don_sim::systems::mountains::{
    MountainRangeEntry, MountainRangeList, MountainRangeSize, Mountains,
};

fn entry(data: i32, metric: u8) -> MountainRangeEntry {
    MountainRangeEntry::new(data, metric)
}

#[test]
fn randomize_uses_small_medium_large_order_and_skips_short_list_draws() {
    let mut mountains = Mountains {
        small_ranges: MountainRangeList::new(vec![entry(10, 1), entry(11, 2)]),
        medium_ranges: MountainRangeList::new(vec![entry(20, 3)]),
        large_ranges: MountainRangeList::new(vec![entry(30, 4), entry(31, 5), entry(32, 6)]),
    };
    let mut random = Random::new(0x1234_5678);
    let mut control = random;
    let expected_small = (control.get(0, 0xffff) as usize) % 2;
    let expected_large = (control.get(0, 0xffff) as usize) % 3;

    let receipt = mountains.randomize_mountains(&mut random);

    assert_eq!(
        receipt.selected_indices,
        [expected_small, 0, expected_large]
    );
    assert_eq!(receipt.draws, 2);
    assert_eq!(receipt.rng_state_after, control.state());
    assert_eq!(random.state(), control.state());
    assert_eq!(mountains.small_ranges.current_index(), Some(expected_small));
    assert_eq!(mountains.medium_ranges.current_index(), Some(0));
    assert_eq!(mountains.large_ranges.current_index(), Some(expected_large));
}

#[test]
fn mutation_of_an_earlier_length_changes_later_rng_assignment() {
    let base = Mountains {
        small_ranges: MountainRangeList::new(vec![entry(10, 0)]),
        medium_ranges: MountainRangeList::new(vec![entry(20, 0), entry(21, 0)]),
        large_ranges: MountainRangeList::new(vec![entry(30, 0), entry(31, 0), entry(32, 0)]),
    };
    let mut mutated = base.clone();
    mutated.small_ranges = MountainRangeList::new(vec![entry(10, 0), entry(11, 0)]);
    let mut base_rng = Random::new(0x1357_2468);
    let mut mutated_rng = base_rng;
    let mut normal = base;

    let normal_receipt = normal.randomize_mountains(&mut base_rng);
    let mutated_receipt = mutated.randomize_mountains(&mut mutated_rng);

    assert_eq!(normal_receipt.draws, 2);
    assert_eq!(mutated_receipt.draws, 3);
    assert_ne!(
        normal_receipt.rng_state_after,
        mutated_receipt.rng_state_after
    );
}

#[test]
fn seek_is_head_relative_and_get_range_returns_then_advances_circularly() {
    let mut mountains = Mountains {
        small_ranges: MountainRangeList::new(vec![entry(100, 7), entry(200, 8), entry(300, 9)]),
        ..Mountains::default()
    };
    let mut random = Random::new(0x2468_1357);
    let receipt = mountains.randomize_mountains(&mut random);
    let selected = receipt.selected_indices[0];
    let values = [100, 200, 300];
    let metrics = [7, 8, 9];

    assert_eq!(mountains.small_ranges.current_data(), values[selected]);
    assert_eq!(mountains.small_ranges.current_metric(), metrics[selected]);
    assert_eq!(
        mountains.get_range(MountainRangeSize::Small),
        values[selected]
    );
    assert_eq!(
        mountains.small_ranges.current_index(),
        Some((selected + 1) % 3)
    );
    assert_eq!(
        mountains.get_range(MountainRangeSize::Small),
        values[(selected + 1) % 3]
    );
}

#[test]
fn empty_and_singleton_lists_consume_no_rng_and_match_native_cursor_behavior() {
    let mut mountains = Mountains {
        small_ranges: MountainRangeList::default(),
        medium_ranges: MountainRangeList::new(vec![entry(-17, 0xfe)]),
        large_ranges: MountainRangeList::default(),
    };
    let mut random = Random::new(-12345);
    let before = random.state();

    let receipt = mountains.randomize_mountains(&mut random);

    assert_eq!(receipt.selected_indices, [0, 0, 0]);
    assert_eq!(receipt.draws, 0);
    assert_eq!(random.state(), before);
    assert_eq!(mountains.get_range(MountainRangeSize::Small), 0);
    assert_eq!(mountains.get_range(MountainRangeSize::Medium), -17);
    assert_eq!(mountains.medium_ranges.current_index(), Some(0));
    assert_eq!(mountains.medium_ranges.current_metric(), 0xfe);
}
