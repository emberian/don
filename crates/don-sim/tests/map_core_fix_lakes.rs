// SPDX-License-Identifier: GPL-3.0-or-later

//! Focused executable expectations for the complete `Map::fix_lakes`
//! `0x0069c470`--`0x0069c5f6` instruction range in the supported retail image.

use don_sim::systems::map_terrain::{land, wflag, World};

fn fertile_world(xs: i32, ys: i32) -> World {
    let mut world = World::init_default_rules(xs, ys);
    for cell in &mut world.wdata {
        cell.land = land::FERTILE;
        cell.land_sub = 0x6d;
    }
    world
}

#[test]
fn exact_six_neighbour_threshold_and_waterhalf_predicate() {
    // The instruction stream compares the non-ocean count with five using JLE:
    // five stays water, six fills. WATERHALF makes a land=OCEAN cell non-ocean
    // for both the centre gate and neighbour count.
    let mut five = fertile_world(7, 7);
    for cell in &mut five.wdata {
        cell.land = land::OCEAN;
    }
    let centre = (3, 3);
    for (x, y) in [(2, 2), (3, 2), (4, 2), (4, 3), (4, 4)] {
        five.wdata_mut(x, y).land = land::FERTILE;
    }
    five.fix_lakes();
    assert_eq!(five.wdata(centre.0, centre.1).land, land::OCEAN);
    assert_eq!(five.wdata(centre.0, centre.1).land_sub, 0x6d);

    let mut six = fertile_world(7, 7);
    for cell in &mut six.wdata {
        cell.land = land::OCEAN;
    }
    for (x, y) in [(2, 2), (3, 2), (4, 2), (4, 3), (4, 4), (3, 4)] {
        six.wdata_mut(x, y).land = land::FERTILE;
    }
    six.fix_lakes();
    assert_eq!(six.wdata(centre.0, centre.1).land, land::FERTILE);
    assert_eq!(six.wdata(centre.0, centre.1).land_sub, 0);

    let mut half = fertile_world(3, 3);
    let cell = half.wdata_mut(1, 1);
    cell.land = land::OCEAN;
    cell.flags |= wflag::WATERHALF;
    half.fix_lakes();
    assert_eq!(half.wdata(1, 1).land, land::OCEAN);
    assert_eq!(half.wdata(1, 1).land_sub, 0x6d);
}

#[test]
fn exhaustive_three_by_three_ocean_truth_table() {
    // A 3x3 world has exactly one scanned cell. Exhaust all 2^9 assignments
    // of fertile/ocean to the centre and its eight neighbours, pinning the
    // retail `is_ocean(center) && non_ocean_neighbours > 5` condition without
    // later fixed-point interactions.
    for pattern in 0_u16..(1 << 9) {
        let mut world = fertile_world(3, 3);
        for bit in 0..9 {
            let x = bit % 3;
            let y = bit / 3;
            if pattern & (1 << bit) != 0 {
                world.wdata_mut(x, y).land = land::OCEAN;
            }
        }

        let centre_was_ocean = pattern & (1 << 4) != 0;
        let ocean_neighbours = (pattern & !(1 << 4)).count_ones();
        let should_fill = centre_was_ocean && 8 - ocean_neighbours > 5;
        world.fix_lakes();

        assert_eq!(
            world.wdata(1, 1).land,
            if should_fill {
                land::FERTILE
            } else if centre_was_ocean {
                land::OCEAN
            } else {
                land::FERTILE
            },
            "pattern={pattern:#05x}"
        );
        assert_eq!(
            world.wdata(1, 1).land_sub,
            if should_fill { 0 } else { 0x6d },
            "pattern={pattern:#05x}"
        );
    }
}

#[test]
fn repeats_row_major_in_place_scans_to_a_fixed_point() {
    let mut world = fertile_world(5, 5);

    // Interior land mask before the call (L=land, ~=ocean):
    //   ~~~
    //   ~~L
    //   L~~
    // Pass one fills (3,1) and (3,3). Because (2,3) was visited before
    // (3,3), it cannot fill until pass two. A one-pass transcription therefore
    // leaves it water, while the retail fixed-point loop makes it land.
    for y in 1..=3 {
        for x in 1..=3 {
            world.wdata_mut(x, y).land = land::OCEAN;
        }
    }
    world.wdata_mut(3, 2).land = land::FERTILE;
    world.wdata_mut(1, 3).land = land::FERTILE;

    let sentinel_flags = 0x805a;
    world.wdata_mut(2, 3).flags = sentinel_flags;
    world.fix_lakes();

    for (x, y) in [(3, 1), (3, 3), (2, 3)] {
        assert_eq!(world.wdata(x, y).land, land::FERTILE, "({x},{y})");
        assert_eq!(world.wdata(x, y).land_sub, 0, "({x},{y})");
    }
    assert_eq!(world.wdata(2, 3).flags, sentinel_flags);
    assert_eq!(world.wdata(1, 1).land, land::OCEAN);
    assert_eq!(world.wdata(1, 1).land_sub, 0x6d);
}

#[test]
fn boundaries_and_non_land_fields_are_preserved() {
    let mut world = fertile_world(4, 4);
    for x in 0..world.xs {
        world.wdata_mut(x, 0).land = land::OCEAN;
        world.wdata_mut(x, world.ys - 1).land = land::OCEAN;
    }
    for y in 0..world.ys {
        world.wdata_mut(0, y).land = land::OCEAN;
        world.wdata_mut(world.xs - 1, y).land = land::OCEAN;
    }

    let before = world.wdata.clone();
    world.fix_lakes();

    for x in 0..world.xs {
        assert_eq!(world.wdata(x, 0), &before[world.w_index(x, 0)]);
        assert_eq!(
            world.wdata(x, world.ys - 1),
            &before[world.w_index(x, world.ys - 1)]
        );
    }
    for y in 0..world.ys {
        assert_eq!(world.wdata(0, y), &before[world.w_index(0, y)]);
        assert_eq!(
            world.wdata(world.xs - 1, y),
            &before[world.w_index(world.xs - 1, y)]
        );
    }
}
