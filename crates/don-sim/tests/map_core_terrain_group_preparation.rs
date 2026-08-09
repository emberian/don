// SPDX-License-Identifier: GPL-3.0-or-later

//! Host-order and clump-size coverage for the `place_all` placement prefix.

use don_sim::rng::Random;
use don_sim::systems::terrain_groups::{
    PlaceAllHostEvent, TerrainGroup, TerrainGroupSelection, TerrainGroups,
    TerrainPlacementBoundary, TerrainPlacementPreparationError,
};

fn group(group_type: i32, pattern: i32) -> TerrainGroup {
    TerrainGroup {
        group_type,
        pattern,
        ..TerrainGroup::default()
    }
}

#[test]
fn daemon_pumps_precede_reads_and_type_six_draws_primary_then_secondary() {
    let mut oil = group(6, 1);
    oil.min_size = 2;
    oil.max_size = 4;
    oil.min_oil = -2;
    oil.max_oil = 10;
    let groups = TerrainGroups {
        groups: vec![group(4, 0), oil],
        ..TerrainGroups::default()
    };
    let selections = [
        TerrainGroupSelection::default(),
        TerrainGroupSelection {
            selected: true,
            clumps: 2,
        },
    ];
    let mut random = Random::new(0x1234_5678);
    let mut callbacks = Vec::new();

    let (receipt, boundary) = groups
        .prepare_placement_prefix(&selections, &mut random, 1, 1, |event| {
            callbacks.push(event)
        })
        .unwrap();

    assert_eq!(
        boundary,
        TerrainPlacementBoundary::UnitTypeCatalogAndRegionPlacementKernel {
            group_index: 1,
            pattern: 1,
        }
    );
    assert_eq!(
        callbacks,
        vec![
            PlaceAllHostEvent::NetDaemonProcessAll { group_index: 0 },
            PlaceAllHostEvent::NetDaemonProcessAll { group_index: 1 },
            PlaceAllHostEvent::ProgressDisplay {
                group_index: 1,
                pattern: 1,
            },
        ]
    );
    assert_eq!(receipt.host_events, callbacks);
    assert_eq!(receipt.primary_size_draws, 2);
    assert_eq!(receipt.secondary_size_draws, 2);
    let plan = &receipt.prepared_groups[0];
    assert_eq!((plan.primary_min, plan.primary_max), (2, 4));
    assert_eq!((plan.secondary_min, plan.secondary_max), (0, 4));
    // Draws are primary[0], secondary[0], primary[1], secondary[1].
    assert_eq!(plan.primary_sizes, vec![3, 3]);
    assert_eq!(plan.secondary_sizes, vec![4, 2]);
}

#[test]
fn skipped_pattern_paths_continue_and_type_four_rounds_to_odd() {
    let mut rounded = group(4, 0);
    rounded.min_size = 2;
    rounded.max_size = 2;
    let mut ignored = group(5, 99);
    ignored.min_size = 8;
    ignored.max_size = 12;
    let mut player_special = group(7, 0);
    player_special.min_size = 4;
    player_special.max_size = 4;
    let groups = TerrainGroups {
        groups: vec![rounded, ignored, player_special, group(4, 0)],
        ..TerrainGroups::default()
    };
    let selections = [
        TerrainGroupSelection {
            selected: true,
            clumps: 2,
        },
        TerrainGroupSelection {
            selected: true,
            clumps: 1,
        },
        TerrainGroupSelection {
            selected: true,
            clumps: 1,
        },
        TerrainGroupSelection::default(),
    ];
    let mut random = Random::new(5);
    let before = random.state();

    let (receipt, boundary) = groups
        .prepare_placement_prefix(&selections, &mut random, 1, 0, |_| {})
        .unwrap();

    assert_eq!(boundary, TerrainPlacementBoundary::AddDoobers);
    assert_eq!(
        receipt
            .host_events
            .iter()
            .filter(|event| matches!(event, PlaceAllHostEvent::NetDaemonProcessAll { .. }))
            .count(),
        4
    );
    assert_eq!(
        receipt
            .host_events
            .iter()
            .filter(|event| matches!(event, PlaceAllHostEvent::ProgressDisplay { .. }))
            .count(),
        1
    );
    assert_eq!(receipt.prepared_groups.len(), 3);
    assert_eq!(receipt.prepared_groups[0].primary_sizes, vec![3, 3]);
    // Type 5 caps only the upper bound at three.  Since lower is eight, retail
    // does not draw and keeps the lower value.
    assert_eq!(
        (
            receipt.prepared_groups[1].primary_min,
            receipt.prepared_groups[1].primary_max,
        ),
        (8, 3)
    );
    assert_eq!(receipt.prepared_groups[1].primary_sizes, vec![8]);
    assert_eq!(random.state(), before);
}

#[test]
fn type_eight_caps_primary_upper_bound_at_five() {
    let mut capped = group(8, 99);
    capped.min_size = 1;
    capped.max_size = 10;
    let groups = TerrainGroups {
        groups: vec![capped],
        ..TerrainGroups::default()
    };
    let selections = [TerrainGroupSelection {
        selected: true,
        clumps: 3,
    }];
    let mut random = Random::new(0x2468_1357);

    let (receipt, boundary) = groups
        .prepare_placement_prefix(&selections, &mut random, 0, 0, |_| {})
        .unwrap();

    assert_eq!(boundary, TerrainPlacementBoundary::AddDoobers);
    let plan = &receipt.prepared_groups[0];
    assert_eq!((plan.primary_min, plan.primary_max), (1, 5));
    assert!(plan.primary_sizes.iter().all(|size| (1..=5).contains(size)));
    assert_eq!(receipt.primary_size_draws, 3);
}

#[test]
fn malformed_size_spans_fail_before_rng_or_host_effects() {
    let mut malformed = group(4, 1);
    malformed.min_size = i32::MIN;
    malformed.max_size = i32::MAX;
    let groups = TerrainGroups {
        groups: vec![malformed],
        ..TerrainGroups::default()
    };
    let selections = [TerrainGroupSelection {
        selected: true,
        clumps: 1,
    }];
    let mut random = Random::new(77);
    let before = random.state();
    let mut host_calls = 0;

    let error = groups
        .prepare_placement_prefix(&selections, &mut random, 1, 1, |_| host_calls += 1)
        .unwrap_err();

    assert_eq!(
        error,
        TerrainPlacementPreparationError::InvalidPrimarySizeSpan {
            group_index: 0,
            min_size: i32::MIN,
            max_size: i32::MAX,
        }
    );
    assert_eq!(random.state(), before);
    assert_eq!(host_calls, 0);

    // The same malformed row is never size-read when selection rejected it.
    let skipped = [TerrainGroupSelection::default()];
    let (receipt, boundary) = groups
        .prepare_placement_prefix(&skipped, &mut random, 0, 0, |_| host_calls += 1)
        .unwrap();
    assert_eq!(boundary, TerrainPlacementBoundary::AddDoobers);
    assert!(receipt.prepared_groups.is_empty());
    assert_eq!(host_calls, 1);
    assert_eq!(random.state(), before);
}
