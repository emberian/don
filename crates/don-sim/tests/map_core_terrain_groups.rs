// SPDX-License-Identifier: GPL-3.0-or-later

//! Mutation-sensitive coverage for `TerrainGroups::fill_fertile` and the first
//! unresolved `place_all` callback.

use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, wflag, World};
use don_sim::systems::terrain_groups::{
    FertilityFractal, FillFertileError, PlaceAllError, TerrainGroup, TerrainGroups,
};

fn filled_world(xs: i32, ys: i32, terrain: i8) -> World {
    let mut world = World::init_default_rules(xs, ys);
    for cell in &mut world.wdata {
        cell.land = terrain;
        cell.land_sub = 0xee;
    }
    world
}

#[test]
fn partition_equality_advances_and_log_order_is_x_then_y() {
    let mut world = filled_world(2, 3, land::FERTILE);
    let groups = TerrainGroups {
        fractal: FertilityFractal {
            columns: vec![vec![0, 9, 10], vec![19, 20, 255]],
        },
        partitions: vec![10, 20],
        ..TerrainGroups::default()
    };
    let mut log = Vec::new();

    let receipt = groups
        .fill_fertile_with_log(&mut world, |bucket| log.push(bucket))
        .unwrap();

    assert_eq!(receipt.fertile_cells, 6);
    assert_eq!(log, vec![0, 0, 1, 1, 2, 2]);
    assert_eq!(world.wdata(0, 0).land_sub, 0);
    assert_eq!(world.wdata(0, 1).land_sub, 0);
    assert_eq!(world.wdata(0, 2).land_sub, 1);
    assert_eq!(world.wdata(1, 0).land_sub, 1);
    assert_eq!(world.wdata(1, 1).land_sub, 2);
    assert_eq!(world.wdata(1, 2).land_sub, 2);
}

#[test]
fn only_the_exact_zero_land_class_is_written_and_other_wdata_is_preserved() {
    let mut world = filled_world(3, 1, land::FERTILE);
    world.wdata_mut(1, 0).land = land::COASTAL;
    world.wdata_mut(2, 0).land = land::OCEAN;
    let fertile = world.wdata_mut(0, 0);
    fertile.flags = wflag::WATERHALF | wflag::MOUNTAINS | 0x8000;
    fertile.region = 37;
    fertile.region2 = 91;
    fertile.goods = 0x5a;
    let groups = TerrainGroups {
        fractal: FertilityFractal {
            columns: vec![vec![200], vec![0], vec![0]],
        },
        // The first matching threshold wins even when the table is non-monotonic.
        partitions: vec![201, 10],
        ..TerrainGroups::default()
    };

    let receipt = groups.fill_fertile(&mut world).unwrap();

    assert_eq!(receipt.fertile_cells, 1);
    let fertile = world.wdata(0, 0);
    assert_eq!(fertile.land, land::FERTILE);
    assert_eq!(fertile.land_sub, 0);
    assert_eq!(fertile.flags, wflag::WATERHALF | wflag::MOUNTAINS | 0x8000);
    assert_eq!(fertile.region, 37);
    assert_eq!(fertile.region2, 91);
    assert_eq!(fertile.goods, 0x5a);
    assert_eq!(world.wdata(1, 0).land_sub, 0xee);
    assert_eq!(world.wdata(2, 0).land_sub, 0xee);
}

#[test]
fn partition_count_narrows_to_the_shipped_land_sub_byte() {
    let mut world = filled_world(1, 1, land::FERTILE);
    let groups = TerrainGroups {
        fractal: FertilityFractal {
            columns: vec![vec![255]],
        },
        // No threshold is greater than 255, so the integer bucket is 260 and
        // the retail char store keeps its low byte.
        partitions: vec![0; 260],
        ..TerrainGroups::default()
    };
    let mut logged = None;

    groups
        .fill_fertile_with_log(&mut world, |bucket| logged = Some(bucket))
        .unwrap();

    assert_eq!(logged, Some(260));
    assert_eq!(world.wdata(0, 0).land_sub, 4);
}

#[test]
fn short_fractal_input_fails_before_the_first_world_write_or_log() {
    let mut world = filled_world(2, 2, land::FERTILE);
    let before = world.wdata.clone();
    let groups = TerrainGroups {
        fractal: FertilityFractal {
            columns: vec![vec![1, 2], vec![3]],
        },
        partitions: vec![128],
        ..TerrainGroups::default()
    };
    let mut logs = 0;

    let error = groups
        .fill_fertile_with_log(&mut world, |_| logs += 1)
        .unwrap_err();

    assert_eq!(
        error,
        FillFertileError::ShortFractalColumn {
            x: 1,
            required_rows: 2,
            actual_rows: 1,
        }
    );
    assert_eq!(logs, 0);
    assert_eq!(world.wdata, before);
}

#[test]
fn missing_fractal_column_and_bad_world_shape_are_typed_failures() {
    let mut world = filled_world(2, 1, land::FERTILE);
    let groups = TerrainGroups {
        fractal: FertilityFractal {
            columns: vec![vec![0]],
        },
        ..TerrainGroups::default()
    };
    assert_eq!(
        groups.fill_fertile(&mut world),
        Err(FillFertileError::MissingFractalColumn {
            x: 1,
            required_columns: 2,
            actual_columns: 1,
        })
    );

    world.size = 1;
    assert_eq!(
        groups.fill_fertile(&mut world),
        Err(FillFertileError::InvalidWorldShape {
            xs: 2,
            ys: 1,
            size: 1,
            wdata_len: 2,
        })
    );
}

#[test]
fn skipped_nonfertile_cells_do_not_require_fractal_storage() {
    let mut world = filled_world(2, 2, land::OCEAN);
    let before = world.wdata.clone();
    let groups = TerrainGroups::default();

    assert_eq!(groups.fill_fertile(&mut world).unwrap().fertile_cells, 0);
    assert_eq!(world.wdata, before);
}

#[test]
fn place_all_stops_before_the_unresolved_mountains_rng_and_list_mutation() {
    let mut world = filled_world(2, 2, land::FERTILE);
    let before_world = world.wdata.clone();
    let mut groups = TerrainGroups {
        groups: vec![TerrainGroup {
            chance: 100,
            min_clumps: 2,
            max_clumps: 5,
            ..TerrainGroup::default()
        }],
        ..TerrainGroups::default()
    };
    let before_groups = groups.clone();
    let mut random = Random::new(0x1234_5678);
    let before_random = random.state();

    assert_eq!(
        groups.place_all(&mut world, &mut random, 1, 1),
        Err(PlaceAllError::MountainsRandomizerUnavailable)
    );
    assert_eq!(random.state(), before_random);
    assert_eq!(world.wdata, before_world);
    assert_eq!(groups, before_groups);
}
