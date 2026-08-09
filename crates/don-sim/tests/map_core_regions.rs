// SPDX-License-Identifier: GPL-3.0-or-later

//! Mutation-sensitive tests for the exact post-coastline region rebuild.

use don_sim::systems::map_terrain::{land, wflag, World};
use don_sim::systems::regions::{make_coastlines_and_rebuild_regions, Regions, RegionsError};

fn filled_world(xs: i32, ys: i32, terrain: i8) -> World {
    let mut world = World::init_default_rules(xs, ys);
    for cell in &mut world.wdata {
        cell.land = terrain;
        cell.land_sub = 0;
        cell.flags = 0;
        cell.region = 23;
    }
    world
}

#[test]
fn clear_all_has_retails_conditional_release_and_preserves_region2() {
    let mut world = filled_world(2, 2, land::FERTILE);
    for (index, cell) in world.wdata.iter_mut().enumerate() {
        cell.region = (40 + index) as i16;
        cell.region2 = (70 + index) as i16;
    }

    let mut regions = Regions::default();
    let occupied = &mut regions.list[5];
    occupied.common_factor = 99;
    occupied.goody_factor = 98;
    occupied.flags = 97;
    occupied.climate = 96;
    occupied.goodies = 95;
    occupied.size = 2;
    occupied.borders = 0x1234;
    occupied.border_id = 12;
    occupied.coords.items = vec![(0, 0), (1, 0)];
    occupied.coords.capacity = 7;
    occupied.coords.increment = 5;
    occupied.coords.flags = 0x41;
    occupied.coords.cur_index = 9;

    // Retail only releases these fields inside `if (size != 0)`.
    let empty = &mut regions.list[6];
    empty.size = 0;
    empty.borders = 0x5678;
    empty.border_id = 34;
    empty.coords.items = vec![(9, 9)];
    empty.coords.capacity = 3;
    empty.coords.increment = 2;
    empty.coords.flags = 0x42;
    empty.coords.cur_index = 11;

    regions.clear_all(&mut world);

    let occupied = &regions.list[5];
    assert_eq!(occupied.common_factor, 8);
    assert_eq!(occupied.goody_factor, 8);
    assert_eq!(occupied.flags, 0);
    assert_eq!(occupied.climate, 0);
    assert_eq!(occupied.goodies, 0);
    assert_eq!(occupied.size, 0);
    assert_eq!(occupied.borders, 0);
    assert_eq!(occupied.border_id, 0);
    assert!(occupied.coords.items.is_empty());
    assert_eq!(occupied.coords.capacity, 0);
    assert_eq!(occupied.coords.increment, 5);
    assert_eq!(occupied.coords.flags, 0);
    assert_eq!(occupied.coords.cur_index, 9);

    let empty = &regions.list[6];
    assert_eq!(empty.common_factor, 8);
    assert_eq!(empty.goody_factor, 8);
    assert_eq!(empty.borders, 0x5678);
    assert_eq!(empty.border_id, 34);
    assert_eq!(empty.coords.items, vec![(9, 9)]);
    assert_eq!(empty.coords.capacity, 3);
    assert_eq!(empty.coords.flags, 0x42);

    assert!(world.wdata.iter().all(|cell| cell.region == 0));
    assert_eq!(
        world
            .wdata
            .iter()
            .map(|cell| cell.region2)
            .collect::<Vec<_>>(),
        vec![70, 71, 72, 73]
    );
    assert_eq!(regions.land, 0);
    assert_eq!(regions.sea, 64);
}

#[test]
fn find_all_uses_eight_connectivity_region2_order_stable_ranks_and_row_major_coords() {
    let mut world = filled_world(5, 5, land::FERTILE);
    for y in 0..5 {
        world.wdata_mut(2, y).land = land::OCEAN;
    }
    let half = world.wdata_mut(1, 2);
    half.land = land::OCEAN;
    half.flags = wflag::WATERHALF;
    let preserved = world.wdata_mut(1, 3);
    preserved.flags = wflag::WATERHALF;
    preserved.region2 = 77;

    let mut regions = Regions::default();
    regions.list[127].coast.bytes[0] = 0xa5;
    regions.list[127].coast.flags = 7;
    let receipt = regions.rebuild_after_coastlines(&mut world).unwrap();

    assert_eq!(receipt.land_components_found, 2);
    assert_eq!(receipt.sea_components_found, 1);
    assert_eq!(receipt.non_input_pumps, 4);
    assert_eq!(regions.list[1].size, 10);
    assert_eq!(regions.list[2].size, 10);
    assert_eq!(regions.list[65].size, 5);
    assert_eq!(regions.get_num_land(), 2);
    assert_eq!(regions.get_num_sea(), 1);

    // Equal sizes retain identity order in the retail bubble sort.
    assert_eq!(regions.list[1].rank, 0);
    assert_eq!(regions.list[2].rank, 1);
    assert_eq!(regions.list[0].rank, 2);
    assert_eq!(regions.list[65].rank, 0);
    assert_eq!(regions.list[64].rank, 1);

    assert_eq!(world.wdata(1, 2).region, 1);
    assert_eq!(world.wdata(1, 2).region2, 65);
    assert_eq!(world.wdata(1, 3).region2, 77);
    assert_eq!(
        regions.list[1].coords.items,
        vec![
            (0, 0),
            (1, 0),
            (0, 1),
            (1, 1),
            (0, 2),
            (1, 2),
            (0, 3),
            (1, 3),
            (0, 4),
            (1, 4),
        ]
    );
    assert_eq!(regions.list[1].coords.capacity, 10);
    assert_eq!(regions.list[1].coords.increment, -1);
    assert_eq!(
        regions.list[65].coords.items,
        (0..5).map(|y| (2, y)).collect::<Vec<_>>()
    );

    assert!(regions.list[1].has_coast(65));
    assert!(regions.list[2].has_coast(65));
    assert!(regions.list[1].is_coastal_with(2));
    assert!(regions.list[2].is_coastal_with(1));
    // set_coastals clears records 0..=126 and deliberately stops before 127.
    assert_eq!(regions.list[127].coast.bytes[0], 0xa5);
    assert_eq!(regions.list[127].coast.flags, 7);
}

#[test]
fn coastal_radius_three_and_shared_sea_closure_are_both_applied() {
    let mut world = filled_world(9, 5, land::OCEAN);
    world.wdata_mut(1, 2).land = land::FERTILE;
    world.wdata_mut(4, 2).land = land::FERTILE;
    world.wdata_mut(7, 2).land = land::FERTILE;

    let mut regions = Regions::default();
    regions.rebuild_after_coastlines(&mut world).unwrap();

    assert_eq!(world.wdata(1, 2).region, 1);
    assert_eq!(world.wdata(4, 2).region, 2);
    assert_eq!(world.wdata(7, 2).region, 3);
    for land_region in 1..=3 {
        assert!(regions.list[land_region].has_coast(65));
    }

    // The binary's radius-three table directly links 1<->2 and 2<->3.
    assert!(regions.list[1].is_coastal_with(2));
    assert!(regions.list[2].is_coastal_with(3));
    // Distance six is outside that table; Region::finalize_coastals adds 1<->3
    // because the chain shares sea region 65.
    assert!(regions.list[1].is_coastal_with(3));
    assert!(regions.list[3].is_coastal_with(1));
}

#[test]
fn overflow_uses_retail_scratch_then_fails_closed_at_its_size_mismatch() {
    fn disconnected_land(count: usize) -> (World, Vec<(i32, i32)>) {
        let mut world = filled_world(27, 24, land::OCEAN);
        let mut placed = Vec::new();
        'rows: for y in (1..24).step_by(3) {
            for x in (1..27).step_by(3) {
                world.wdata_mut(x, y).land = land::FERTILE;
                placed.push((x, y));
                if placed.len() == count {
                    break 'rows;
                }
            }
        }
        (world, placed)
    }

    // The 62nd land component goes through scratch id 62 and is moved into
    // previously-reserved id 0.  With no later component, retail stays consistent.
    let (mut world, placed) = disconnected_land(62);
    assert_eq!(placed.len(), 62);
    let mut regions = Regions::default();
    let receipt = regions.rebuild_after_coastlines(&mut world).unwrap();
    assert_eq!(receipt.land_components_found, 62);
    assert_eq!(receipt.land_consolidations, 1);
    assert_eq!(receipt.sea_components_found, 1);
    assert_eq!(regions.list[0].size, 1);
    assert_eq!(regions.list[62].size, 0);
    assert_eq!(regions.list[63].size, 0);
    let &(last_x, last_y) = placed.last().unwrap();
    assert_eq!(world.wdata(last_x, last_y).region, 0);
    assert_eq!(regions.get_num_land(), 62);

    // With later land components, retail's all-WData `0 -> 63` relabel captures
    // them before their flood and does not update region 63's size.  The shipped
    // rebuild_coords diagnostic is an explicit transactional error here.
    let (mut overflow_world, overflow_placed) = disconnected_land(64);
    assert_eq!(overflow_placed.len(), 64);
    let before_world = overflow_world.wdata.clone();
    let mut overflow_regions = Regions::default();
    overflow_regions.list[7].common_factor = 123;
    let before_regions = overflow_regions.clone();
    assert_eq!(
        overflow_regions.rebuild_after_coastlines(&mut overflow_world),
        Err(RegionsError::RegionSizeMismatch {
            region: 63,
            expected: 0,
            actual: 1,
        })
    );
    assert_eq!(overflow_world.wdata, before_world);
    assert_eq!(overflow_regions, before_regions);
}

#[test]
fn sea_overflow_uses_125_as_scratch_and_126_as_a_consistent_aggregate() {
    let mut world = filled_world(27, 24, land::FERTILE);
    let mut placed = Vec::new();
    'rows: for y in (1..24).step_by(3) {
        for x in (1..27).step_by(3) {
            world.wdata_mut(x, y).land = land::OCEAN;
            placed.push((x, y));
            if placed.len() == 63 {
                break 'rows;
            }
        }
    }
    assert_eq!(placed.len(), 63);

    let mut regions = Regions::default();
    let receipt = regions.rebuild_after_coastlines(&mut world).unwrap();

    assert_eq!(receipt.land_components_found, 1);
    assert_eq!(receipt.sea_components_found, 63);
    assert_eq!(receipt.sea_consolidations, 3);
    assert_eq!(regions.list[125].size, 0);
    assert_eq!(regions.list[126].size, 2);
    assert_eq!(regions.list[126].coords.items.len(), 2);
    assert_eq!(regions.get_num_sea(), 62);
}

#[test]
fn malformed_world_is_rejected_without_partial_clear_or_coastline_writes() {
    let mut world = filled_world(3, 3, land::FERTILE);
    world.wdata_mut(1, 1).land = land::OCEAN;
    world.size = 8;
    let before_wdata = world.wdata.clone();
    let mut regions = Regions::default();
    regions.list[4].size = 9;
    regions.list[4].common_factor = 123;
    let before_regions = regions.clone();

    let error = make_coastlines_and_rebuild_regions(&mut world, &mut regions).unwrap_err();
    assert_eq!(
        error,
        RegionsError::InvalidWorldShape {
            xs: 3,
            ys: 3,
            size: 8,
            wdata_len: 9,
        }
    );
    assert_eq!(world.wdata, before_wdata);
    assert_eq!(regions, before_regions);
}

#[test]
fn composed_construction_stage_runs_coastline_fix_before_region_floods() {
    let mut world = filled_world(3, 3, land::FERTILE);
    world.wdata_mut(1, 1).land = land::OCEAN;
    let mut regions = Regions::default();

    let receipt = make_coastlines_and_rebuild_regions(&mut world, &mut regions).unwrap();

    assert_eq!(world.wdata(1, 1).land, land::FERTILE);
    assert!(world.wdata.iter().all(|cell| cell.region == 1));
    assert_eq!(regions.list[1].size, 9);
    assert_eq!(receipt.land_components_found, 1);
    assert_eq!(receipt.sea_components_found, 0);
    assert_eq!(receipt.non_input_pumps, 2);
}
