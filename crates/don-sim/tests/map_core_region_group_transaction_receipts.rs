// SPDX-License-Identifier: GPL-3.0-or-later

//! Mutation-sensitive evidence for the enclosing RNG/world/checksum receipt of
//! `TerrainGroup::place_region_group` (`0x006a2f60`).

use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, wflag, World, WorldSection};
use don_sim::systems::mountain_add_runtime::{
    GridOffset, MountainAddRuntime, MountainAddRuntimeError, MountainTemplateRuntime,
};
use don_sim::systems::regions::Regions;
use don_sim::systems::terrain_drop_tile::{DropTileExternalRequest, DropTileExternalResolution};
use don_sim::systems::terrain_groups::TerrainGroup;
use don_sim::systems::terrain_region_continuation::{
    PlaceRegionGroupError, PlaceRegionGroupOutcome, PlaceRegionGroupOwnerReceipt,
    PlaceRegionGroupOwners,
};
use don_sim::systems::terrain_region_patterns::RegionPatternError;
use don_sim::systems::terrain_region_placement::{PlaceRegionGroupCall, RegionDropTileInvocation};
use don_sim::systems::world_oil_goods::{OilGoodMutationError, OilGoodRuntime};

fn world_with_land(xs: i32, ys: i32, land_class: i8) -> World {
    let mut world = World::init_default_rules(xs, ys);
    for cell in &mut world.wdata {
        cell.land = land_class;
    }
    world
}

fn one_coord_region(x: i32, y: i32) -> Regions {
    let mut regions = Regions::default();
    regions.list[1].coords.items.push((x, y));
    regions.list[1].coords.capacity = 1;
    regions
}

fn call(target_tiles: i32, land_subtype: i32) -> PlaceRegionGroupCall {
    PlaceRegionGroupCall {
        target_tiles,
        region_id: 1,
        land_subtype,
        oil_deposits: 0,
        place_players: 0,
        group_index: 0,
    }
}

fn one_cell_mountain_template() -> MountainTemplateRuntime {
    MountainTemplateRuntime {
        mount_tiles: vec![GridOffset::new(0, 0)],
        mount_wcoords: vec![GridOffset::new(0, 0)],
        solid_mount_wcoords: vec![GridOffset::new(0, 0)],
    }
}

fn style_twelve_mountain_runtime(world: &World) -> MountainAddRuntime {
    let mut templates = vec![None; 13];
    templates[12] = Some(one_cell_mountain_template());
    MountainAddRuntime::new(world.wdata.len(), templates)
}

#[test]
fn ordinary_place_all_entry_keeps_the_full_world_audit_opt_in() {
    let mut world = world_with_land(9, 9, land::FERTILE);
    let regions = one_coord_region(4, 4);
    let mut group = TerrainGroup {
        group_type: 4,
        ..TerrainGroup::default()
    };
    let mut random = Random::new(7);

    let receipt = group
        .apply_place_region_group(&mut world, &regions, &mut random, call(1, -1), None, &[])
        .unwrap();

    assert!(receipt.world.is_none());
    assert_eq!(receipt.rng_draws, 0);
    assert!(receipt.rng_receipt_is_coherent());
}

#[test]
fn one_forest_cell_receipt_names_every_changed_plane_and_walk_section() {
    let mut world = world_with_land(9, 9, land::FERTILE);
    let before_image = world.checksum_image();
    let before_checksum = world.checksum_sections();
    let regions = one_coord_region(4, 4);
    let mut group = TerrainGroup {
        group_type: 4,
        ..TerrainGroup::default()
    };
    let mut random = Random::new(0x1234_5678);

    let receipt = group
        .apply_place_region_group_audited(&mut world, &regions, &mut random, call(1, -1), None, &[])
        .unwrap();
    let world_receipt = receipt.world.as_ref().unwrap();

    assert_eq!(receipt.outcome, PlaceRegionGroupOutcome::Returned(1));
    assert_eq!(receipt.rng_draws, 0);
    assert!(receipt.rng_receipt_is_coherent());
    assert_eq!(receipt.rng_state_before, 0x1234_5678);
    assert_eq!(receipt.rng_state_after, 0x1234_5678);
    assert_eq!(world_receipt.checksum_before, before_checksum);
    assert_eq!(world_receipt.checksum_after, world.checksum_sections());
    assert_eq!(
        world_receipt.changed_sections,
        [WorldSection::WData, WorldSection::TDataAndFog]
    );
    // `World::set_blocked_at` updates the 3x3 WData blocked counters around
    // the 4x4 tile block as well as the forest cell itself.
    assert_eq!(
        world_receipt.changed_wdata_indices,
        [30, 31, 32, 39, 40, 41, 48, 49, 50]
    );

    // The sixteen block calls also update the one-tile neighbour ring, yielding
    // the exact 6x6 physical TData footprint.
    let expected_tdata: Vec<_> = (15..21)
        .flat_map(|y| (15..21).map(move |x| (y * world.tile_xs + x) as usize))
        .collect();
    assert_eq!(world_receipt.changed_tdata_indices, expected_tdata);

    let after_image = world.checksum_image();
    let expected_bytes: Vec<_> = before_image
        .0
        .iter()
        .zip(&after_image.0)
        .enumerate()
        .filter_map(|(offset, (&before, &after))| {
            (before != after).then_some((offset, before, after))
        })
        .collect();
    let reported_bytes: Vec<_> = world_receipt
        .byte_mutations
        .iter()
        .map(|byte| (byte.offset, byte.before, byte.after))
        .collect();
    assert_eq!(reported_bytes, expected_bytes);
    assert!(!reported_bytes.is_empty());
}

#[test]
fn growth_receipt_counts_the_skipped_then_taken_base_draw_and_both_orthog_words() {
    let mut world = world_with_land(10, 10, land::FERTILE);
    let regions = one_coord_region(4, 4);
    let mut group = TerrainGroup {
        group_type: 6,
        ..TerrainGroup::default()
    };
    let mut random = Random::new(0x1234_5678);

    let receipt = group
        .apply_place_region_group_audited(&mut world, &regions, &mut random, call(3, -1), None, &[])
        .unwrap();
    let world_receipt = receipt.world.as_ref().unwrap();

    // Entry region length one: 0. Growth with one base: 0 + 2 orthog
    // words. Growth with two bases: 1 base-index + 2 orthog words.
    assert_eq!(receipt.rng_draws, 5);
    assert_eq!(receipt.growth_passes.len(), 2);
    assert_eq!(receipt.growth_passes[0].base_order.len(), 1);
    assert_eq!(receipt.growth_passes[0].base_index_draws, 0);
    // The second pass draws from a two-base list, then succeeds on its first
    // visited base. `base_order.len()` is therefore one and cannot be used as
    // a proxy for the draw predicate.
    assert_eq!(receipt.growth_passes[1].base_order.len(), 1);
    assert_eq!(receipt.growth_passes[1].base_index_draws, 1);
    assert!(receipt.rng_receipt_is_coherent());
    assert_eq!(receipt.rng_state_after, random.state());
    assert_eq!(world_receipt.changed_sections, [WorldSection::WData]);
    assert_eq!(world_receipt.changed_wdata_indices.len(), 3);
    assert_eq!(world_receipt.checksum_after, world.checksum_sections());
}

#[test]
fn the_first_mediterranean_mountain_boundary_has_no_hidden_rng_or_world_ack() {
    let mut world = world_with_land(9, 9, land::FERTILE);
    let before = world.clone();
    let regions = one_coord_region(4, 4);
    let mut group = TerrainGroup {
        group_type: 5,
        ..TerrainGroup::default()
    };
    let mut random = Random::new(17);

    let receipt = group
        .apply_place_region_group_audited(&mut world, &regions, &mut random, call(8, 12), None, &[])
        .unwrap();
    let world_receipt = receipt.world.as_ref().unwrap();
    let expected = DropTileExternalRequest::MountainsAddMountain {
        template: 12,
        world_x: 4,
        world_y: 4,
        pattern: 4,
        mountain_space: 0,
        forest_space: 0,
        rock_space: 0,
        coast_space: 0,
        start_min: 0,
    };

    assert_eq!(
        receipt.outcome,
        PlaceRegionGroupOutcome::ExternalResolutionRequired { request: expected }
    );
    assert_eq!(receipt.external_resolutions_consumed, 0);
    assert_eq!(receipt.rng_draws, 0);
    assert!(receipt.rng_receipt_is_coherent());
    assert_eq!(world_receipt.checksum_before, world_receipt.checksum_after);
    assert!(world_receipt.changed_sections.is_empty());
    assert!(world_receipt.byte_mutations.is_empty());
    assert_eq!(world.wdata, before.wdata);
    assert_eq!(world.tdata, before.tdata);
    assert_eq!(world.checksum_image().0, before.checksum_image().0);
}

#[test]
fn oil_acknowledgement_reports_only_the_local_world_bit_not_an_invented_good() {
    let mut world = world_with_land(9, 9, land::OCEAN);
    let regions = one_coord_region(4, 4);
    let mut group = TerrainGroup {
        group_type: 7,
        ..TerrainGroup::default()
    };
    let invocation = RegionDropTileInvocation {
        world_x: 4,
        world_y: 4,
        group_type: 7,
        group_radius: 1,
        land_subtype: -1,
        target_tiles: 1,
        oil_deposits: 0,
        group_index: 0,
    };
    let request = group.drop_tile_external_request(invocation).unwrap();
    let mut random = Random::new(99);

    let receipt = group
        .apply_place_region_group_audited(
            &mut world,
            &regions,
            &mut random,
            call(1, -1),
            None,
            &[DropTileExternalResolution::OilGoodsApplied { request }],
        )
        .unwrap();
    let world_receipt = receipt.world.as_ref().unwrap();

    assert_eq!(receipt.outcome, PlaceRegionGroupOutcome::Returned(1));
    assert_eq!(receipt.external_resolutions_consumed, 1);
    assert_eq!(receipt.rng_draws, 0);
    assert_eq!(world_receipt.changed_sections, [WorldSection::WData]);
    assert_eq!(world_receipt.changed_wdata_indices, [40]);
    assert!(world_receipt.changed_tdata_indices.is_empty());
    assert_ne!(world.wdata(4, 4).flags & wflag::OIL, 0);
}

#[test]
fn owned_mediterranean_mountain_commits_world_runtime_and_both_walk_receipts() {
    let mut world = world_with_land(9, 9, land::FERTILE);
    let world_before = world.clone();
    let regions = one_coord_region(4, 4);
    let mut group = TerrainGroup {
        group_type: 5,
        ..TerrainGroup::default()
    };
    let mut random = Random::new(17);
    let mut owners = PlaceRegionGroupOwners {
        mountains: Some(style_twelve_mountain_runtime(&world)),
        oil_goods: None,
    };

    let receipt = group
        .apply_place_region_group_owned_audited(
            &mut world,
            &regions,
            &mut random,
            call(8, 12),
            None,
            &mut owners,
        )
        .unwrap();

    assert!(receipt.committed);
    assert_eq!(
        receipt.placement.outcome,
        PlaceRegionGroupOutcome::Returned(1)
    );
    assert_eq!(receipt.placement.external_resolutions_consumed, 1);
    assert_eq!(receipt.placement.rng_draws, 0);
    assert!(receipt.placement.rng_receipt_is_coherent());
    assert_eq!(random.state(), 17);
    assert_eq!(group.tiles.items, vec![(4, 4)]);
    assert_ne!(world.wdata(4, 4).flags & wflag::MOUNTAINS, 0);

    let full_world = receipt.placement.world.as_ref().unwrap();
    assert_eq!(full_world.checksum_before, world_before.checksum_sections());
    assert_eq!(full_world.checksum_after, world.checksum_sections());
    assert_eq!(
        full_world.changed_sections,
        [WorldSection::WData, WorldSection::TDataAndFog]
    );
    assert!(!full_world.byte_mutations.is_empty());

    let PlaceRegionGroupOwnerReceipt::Mountain(owner) = &receipt.owners[0] else {
        panic!("expected the mountain owner receipt");
    };
    assert_eq!(owner.execution.liberr, 0);
    assert_eq!(owner.execution.rng_draws, 0);
    assert_eq!(owner.execution.retained_index, Some(0));
    assert_eq!(owner.world.checksum_before, full_world.checksum_before);
    assert_eq!(owner.world.checksum_after, full_world.checksum_after);
    assert_ne!(
        owner.mountain_walk_adler_before,
        owner.mountain_walk_adler_after
    );
    assert!(owner.mountain_walk_bytes_after > owner.mountain_walk_bytes_before);
    assert_eq!(
        owners.mountains.as_ref().unwrap().mountain_types.items,
        [12]
    );
}

#[test]
fn owned_mountain_native_rejection_is_a_committed_zero_not_a_typed_error() {
    let mut world = world_with_land(9, 9, land::FERTILE);
    let reserved = (4 * world.xs + 5) as usize;
    world.start_city_locs[reserved >> 3] |= 1 << (reserved & 7);
    let world_before = world.clone();
    let regions = one_coord_region(4, 4);
    let mut group = TerrainGroup {
        group_type: 5,
        ..TerrainGroup::default()
    };
    let mut random = Random::new(17);
    let mut owners = PlaceRegionGroupOwners {
        mountains: Some(style_twelve_mountain_runtime(&world)),
        oil_goods: None,
    };
    let mountains_before = owners.mountains.clone().unwrap();

    let receipt = group
        .apply_place_region_group_owned_audited(
            &mut world,
            &regions,
            &mut random,
            call(8, 12),
            None,
            &mut owners,
        )
        .unwrap();

    assert!(receipt.committed);
    assert_eq!(
        receipt.placement.outcome,
        PlaceRegionGroupOutcome::Returned(0)
    );
    assert!(group.tiles.items.is_empty());
    assert_eq!(world.wdata, world_before.wdata);
    assert_eq!(world.tdata, world_before.tdata);
    assert_eq!(owners.mountains.as_ref().unwrap(), &mountains_before);
    let PlaceRegionGroupOwnerReceipt::Mountain(owner) = &receipt.owners[0] else {
        panic!("expected the mountain owner receipt");
    };
    assert_eq!(owner.execution.liberr, 1);
    assert!(owner.execution.rejection.is_some());
    assert_eq!(owner.world.checksum_before, owner.world.checksum_after);
    assert_eq!(
        owner.mountain_walk_adler_before,
        owner.mountain_walk_adler_after
    );
}

#[test]
fn owned_great_lakes_oil_commits_a_real_good_and_goods_checksum_receipt() {
    let mut world = world_with_land(9, 9, land::OCEAN);
    let regions = one_coord_region(4, 4);
    let mut group = TerrainGroup {
        group_type: 7,
        ..TerrainGroup::default()
    };
    let mut random = Random::new(99);
    let mut owners = PlaceRegionGroupOwners {
        mountains: None,
        oil_goods: Some(OilGoodRuntime::default()),
    };

    let receipt = group
        .apply_place_region_group_owned_audited(
            &mut world,
            &regions,
            &mut random,
            call(1, -1),
            None,
            &mut owners,
        )
        .unwrap();

    assert!(receipt.committed);
    assert_eq!(
        receipt.placement.outcome,
        PlaceRegionGroupOutcome::Returned(1)
    );
    assert_eq!(receipt.placement.external_resolutions_consumed, 1);
    assert_eq!(receipt.placement.rng_draws, 0);
    assert!(receipt.placement.rng_receipt_is_coherent());
    assert_ne!(world.wdata(4, 4).flags & wflag::OIL, 0);
    assert_eq!(group.tiles.items, vec![(4, 4)]);

    let PlaceRegionGroupOwnerReceipt::OilGood(owner) = &receipt.owners[0] else {
        panic!("expected the oil/Good owner receipt");
    };
    assert_eq!(owner.rng_draws, 0);
    assert_eq!(owner.before.active_count, 0);
    assert_eq!(owner.after.active_count, 1);
    assert_ne!(owner.before.goods_checksum, owner.after.goods_checksum);
    assert_eq!(owner.after.scenario_rows.len(), 1);
    let goods = owners.oil_goods.as_ref().unwrap();
    assert_eq!(goods.goods_checksum(), owner.after.goods_checksum);
    assert_eq!(goods.scenario_rows(), owner.after.scenario_rows);
}

#[test]
fn missing_mountain_template_is_a_typed_stop_and_rolls_back_every_owner() {
    let mut world = world_with_land(9, 9, land::FERTILE);
    let regions = one_coord_region(4, 4);
    let mut group = TerrainGroup {
        group_type: 5,
        ..TerrainGroup::default()
    };
    let mut random = Random::new(17);
    let mut owners = PlaceRegionGroupOwners {
        mountains: Some(MountainAddRuntime::new(world.wdata.len(), Vec::new())),
        oil_goods: Some(OilGoodRuntime::default()),
    };
    let group_before = group.clone();
    let world_before = world.clone();
    let random_before = random;
    let owners_before = owners.clone();

    let error = group
        .apply_place_region_group_owned_audited(
            &mut world,
            &regions,
            &mut random,
            call(8, 12),
            None,
            &mut owners,
        )
        .unwrap_err();

    assert_eq!(
        error,
        PlaceRegionGroupError::InvalidMountainRuntime(MountainAddRuntimeError::MissingTemplate {
            template: 12
        })
    );
    assert_eq!(group, group_before);
    assert_eq!(world.wdata, world_before.wdata);
    assert_eq!(world.tdata, world_before.tdata);
    assert_eq!(world.checksum_image().0, world_before.checksum_image().0);
    assert_eq!(random.state(), random_before.state());
    assert_eq!(owners, owners_before);

    // The enclosing error must remain clonable after it gained rich owner
    // causes; RegionPatternError deliberately no longer promises implicit Copy.
    let pattern_error = RegionPatternError::InvalidRegionPlacement(error);
    assert_eq!(pattern_error.clone(), pattern_error);
}

#[test]
fn late_oil_owner_refusal_rolls_back_the_earlier_rocks_world_transaction() {
    let mut world = world_with_land(9, 9, land::FERTILE);
    let regions = one_coord_region(4, 4);
    let mut group = TerrainGroup {
        group_type: 6,
        ..TerrainGroup::default()
    };
    let mut random = Random::new(0x1234_5678);
    let mut invalid_goods = OilGoodRuntime::default();
    invalid_goods.increment = 0;
    let mut owners = PlaceRegionGroupOwners {
        mountains: None,
        oil_goods: Some(invalid_goods),
    };
    let group_before = group.clone();
    let world_before = world.clone();
    let random_before = random;
    let owners_before = owners.clone();
    let mut oil_call = call(1, -1);
    oil_call.oil_deposits = 1;

    let error = group
        .apply_place_region_group_owned_audited(
            &mut world,
            &regions,
            &mut random,
            oil_call,
            None,
            &mut owners,
        )
        .unwrap_err();

    assert_eq!(
        error,
        PlaceRegionGroupError::InvalidOilGoodRuntime(OilGoodMutationError::ArrayCannotGrow {
            capacity: 0,
            increment: 0,
        })
    );
    assert_eq!(group, group_before);
    assert_eq!(world.wdata, world_before.wdata);
    assert_eq!(world.tdata, world_before.tdata);
    assert_eq!(world.checksum_image().0, world_before.checksum_image().0);
    assert_eq!(random.state(), random_before.state());
    assert_eq!(owners, owners_before);
}
