// SPDX-License-Identifier: GPL-3.0-or-later

//! Mutation-sensitive expectations for the complete shipped
//! `Map::make_coastlines` `0x006947a0`--`0x00694aa5` body.

use don_sim::systems::map_terrain::{land, wflag, World};

fn filled_world(xs: i32, ys: i32, terrain: i8) -> World {
    let mut world = World::init_default_rules(xs, ys);
    for cell in &mut world.wdata {
        cell.land = terrain;
        cell.land_sub = 0x6d;
    }
    world
}

#[test]
fn construction_chain_runs_fix_lakes_before_coast_scans() {
    let mut world = filled_world(3, 3, land::FERTILE);
    let centre = world.wdata_mut(1, 1);
    centre.land = land::OCEAN;
    centre.land_sub = 0x7a;

    world.make_coastlines();

    // All eight neighbours are non-ocean, so the embedded fix_lakes call fills
    // the cell. With no ocean left, neither coastline scan marks it afterwards.
    assert_eq!(world.wdata(1, 1).land, land::FERTILE);
    assert_eq!(world.wdata(1, 1).land_sub, 0);
    assert_eq!(world.wdata(1, 1).flags & wflag::COAST, 0);
}

#[test]
fn first_scan_marks_only_the_land_side_and_preserves_waterhalf() {
    let mut world = filled_world(7, 7, land::FERTILE);
    world.wdata_mut(0, 3).land = land::OCEAN;

    let half = world.wdata_mut(1, 3);
    half.land = land::OCEAN;
    half.flags = wflag::WATERHALF | wflag::MOUNTAINS | 0x8000;
    half.land_sub = 0x4b;

    world.make_coastlines();

    let half = world.wdata(1, 3);
    assert_eq!(half.land, land::OCEAN);
    assert_eq!(half.land_sub, 0x4b);
    assert_eq!(
        half.flags,
        wflag::WATERHALF | wflag::COAST | wflag::ORIG_COAST | 0x8000
    );

    // The pure-ocean side is not relabelled, and dry terrain outside the
    // eight-neighbour boundary remains unmarked.
    assert_eq!(world.wdata(0, 3).flags & wflag::COAST, 0);
    assert_eq!(world.wdata(3, 3).flags & wflag::COAST, 0);
}

#[test]
fn second_scan_uses_the_exact_floating_coast_mask_and_word_write() {
    let mut world = filled_world(5, 5, land::OCEAN);
    let centre = world.wdata_mut(2, 2);
    centre.flags = u16::MAX;
    centre.land = land::COASTAL;
    centre.land_sub = 0x7e;
    centre.region = 0x1234;
    centre.goods = 0x56;

    world.make_coastlines();

    let centre = world.wdata(2, 2);
    assert_eq!(centre.flags, 0xfac3);
    assert_eq!(centre.land, land::OCEAN);
    assert_eq!(centre.land_sub, 0);
    assert_eq!(centre.region, 0x1234);
    assert_eq!(centre.goods, 0x56);
}

#[test]
fn inland_support_preserves_coast_but_offmap_does_not() {
    let mut solid = filled_world(5, 5, land::FERTILE);
    solid.wdata_mut(2, 2).flags = wflag::COAST | wflag::ORIG_COAST;
    solid.make_coastlines();
    assert_eq!(
        solid.wdata(2, 2).flags & (wflag::COAST | wflag::ORIG_COAST),
        wflag::COAST | wflag::ORIG_COAST
    );
    assert_eq!(solid.wdata(2, 2).land, land::FERTILE);

    let mut edge = filled_world(3, 3, land::OCEAN);
    edge.wdata_mut(0, 0).flags = wflag::COAST | wflag::ORIG_COAST;
    edge.make_coastlines();
    assert_eq!(edge.wdata(0, 0).flags & wflag::COAST, 0);
    assert_eq!(edge.wdata(0, 0).land, land::OCEAN);
    assert_eq!(edge.wdata(0, 0).land_sub, 0);
}
