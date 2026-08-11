// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../src/systems/mountain_add_runtime.rs"]
mod subject;

use don_sim::systems::map_terrain::{self, World};
use subject::{
    AddMountainCall, GridOffset, MountainAddRuntime, MountainAddRuntimeError,
    MountainLocationVertex, MountainRejection, MountainSpacingKind, MountainTemplateRuntime,
    MountainWorld, MountainWorldCell, EXCLUDING_VERIFY_MODE, LIBERR_GENERAL, LIBERR_OK,
};

impl MountainWorld for World {
    fn world_xs(&self) -> i32 {
        self.xs
    }

    fn world_ys(&self) -> i32 {
        self.ys
    }

    fn tile_xs(&self) -> i32 {
        self.tile_xs
    }

    fn tile_ys(&self) -> i32 {
        self.tile_ys
    }

    fn world_cell(&self, wx: i32, wy: i32) -> MountainWorldCell {
        let cell = self.wdata(wx, wy);
        MountainWorldCell {
            flags: cell.flags,
            land: cell.land,
        }
    }

    fn write_world_flags(&mut self, wx: i32, wy: i32, flags: u16) {
        self.wdata_mut(wx, wy).flags = flags;
    }

    fn tile_mask(&self, tx: i32, ty: i32) -> u16 {
        self.tmask(tx, ty)
    }

    fn set_mountain_tile(&mut self, tx: i32, ty: i32) {
        self.set_mountain_at(tx, ty, true);
    }

    fn set_behind_b(&mut self, tx: i32, ty: i32) {
        self.set_behind(tx, ty, true, true);
    }

    fn start_x_count(&self) -> usize {
        self.start_x.items.len()
    }

    fn start_y_count(&self) -> usize {
        self.start_y.items.len()
    }

    fn start_at(&self, index: usize) -> (i32, i32) {
        (self.start_x.items[index], self.start_y.items[index])
    }

    fn start_city_reserved(&self, wx: i32, wy: i32) -> bool {
        let index = (wy * self.xs + wx) as usize;
        self.start_city_locs[index >> 3] & (1 << (index & 7)) != 0
    }
}

fn flat_world() -> World {
    let mut world = World::init_default_rules(12, 12);
    for cell in &mut world.wdata {
        cell.land = map_terrain::land::FERTILE;
        cell.flags = 0;
    }
    world
}

fn template() -> MountainTemplateRuntime {
    MountainTemplateRuntime {
        mount_tiles: vec![GridOffset::new(0, 0)],
        mount_wcoords: vec![GridOffset::new(0, 0)],
        solid_mount_wcoords: vec![GridOffset::new(0, 0)],
    }
}

fn runtime(template: MountainTemplateRuntime) -> MountainAddRuntime {
    MountainAddRuntime::new(12 * 12, vec![Some(template)])
}

fn call(wx: i32, wy: i32) -> AddMountainCall {
    AddMountainCall {
        template: 0,
        world_x: wx,
        world_y: wy,
        verification_mode: EXCLUDING_VERIFY_MODE,
        mountain_space: 0,
        forest_space: 0,
        rock_space: 0,
        coast_space: 0,
        start_min: 0,
    }
}

fn snapshot(world: &World, runtime: &MountainAddRuntime) -> (String, MountainAddRuntime) {
    (format!("{world:?}"), runtime.clone())
}

#[test]
fn mode_four_success_commits_world_tiles_behind_row_and_walked_arrays() {
    let mut world = flat_world();
    // The success path preserves COAST/ORIG_COAST while adding MOUNTAINS and
    // clearing the other land-class bits.
    world.wdata_mut(5, 5).flags = map_terrain::wflag::COAST | map_terrain::wflag::ORIG_COAST;
    let mut runtime = runtime(template());

    let receipt = runtime.apply_add_mountain(&mut world, call(5, 5)).unwrap();

    assert_eq!(receipt.liberr, LIBERR_OK);
    assert_eq!(receipt.rejection, None);
    assert_eq!(receipt.unique_verify_cells, 1);
    assert_eq!(receipt.mountain_wcoords_written, 1);
    assert_eq!(receipt.mountain_tiles_written, 1);
    assert_eq!(receipt.behind_tiles_set, 3);
    assert_eq!(receipt.retained_index, Some(0));
    assert_eq!(receipt.rng_draws, 0);
    assert_eq!(
        world.wdata(5, 5).flags
            & (map_terrain::wflag::COAST
                | map_terrain::wflag::MOUNTAINS
                | map_terrain::wflag::ORIG_COAST),
        map_terrain::wflag::COAST | map_terrain::wflag::MOUNTAINS | map_terrain::wflag::ORIG_COAST
    );
    assert_eq!(
        world.tmask(20, 20) & map_terrain::tflag::BLOCKER_MASK,
        map_terrain::tflag::BLOCKER_MOUNTAIN
    );
    assert_ne!(world.tmask(20, 20) & map_terrain::tflag::BLOCKED, 0);
    for tx in [19, 20, 21] {
        assert_ne!(world.tmask(tx, 19) & map_terrain::tflag::BEHIND_B, 0);
    }
    assert_eq!(runtime.mountain_loc_wcoords_x.items, vec![5]);
    assert_eq!(runtime.mountain_loc_wcoords_y.items, vec![5]);
    assert_eq!(runtime.mountain_types.items, vec![0]);
    assert_eq!(
        runtime.mountain_locs.items,
        vec![MountainLocationVertex {
            x_bits: (5i32.wrapping_mul(0x300) as f32).to_bits(),
            y_bits: (5i32.wrapping_mul(0x300) as f32).to_bits(),
            z_bits: 0,
        }]
    );
    for capacity in [
        runtime.mountain_loc_wcoords_x.capacity,
        runtime.mountain_loc_wcoords_y.capacity,
        runtime.mountain_locs.capacity,
        runtime.mountain_types.capacity,
    ] {
        assert_eq!(capacity, 4);
    }
    assert!(runtime.verify_bits.iter().all(|&byte| byte == 0));
}

#[test]
fn the_four_spacing_arguments_reject_their_own_native_flag_only() {
    let cases = [
        (MountainSpacingKind::Mountain, map_terrain::wflag::MOUNTAINS),
        (MountainSpacingKind::Coast, map_terrain::wflag::COAST),
        (MountainSpacingKind::Forest, map_terrain::wflag::FOREST),
        (MountainSpacingKind::Rock, map_terrain::wflag::ROCKS),
    ];

    for (kind, flag) in cases {
        let mut world = flat_world();
        world.wdata_mut(6, 5).flags = flag;
        let mut runtime = runtime(template());
        let before = snapshot(&world, &runtime);
        let mut request = call(5, 5);
        match kind {
            MountainSpacingKind::Mountain => request.mountain_space = 1,
            MountainSpacingKind::Coast => request.coast_space = 1,
            MountainSpacingKind::Forest => request.forest_space = 1,
            MountainSpacingKind::Rock => request.rock_space = 1,
        }

        let receipt = runtime.apply_add_mountain(&mut world, request).unwrap();
        assert_eq!(receipt.liberr, LIBERR_GENERAL);
        assert!(matches!(
            receipt.rejection,
            Some(MountainRejection::Spacing { kind: actual, .. }) if actual == kind
        ));
        assert_eq!(snapshot(&world, &runtime), before);
    }
}

#[test]
fn start_distance_rejection_clears_only_bits_mode_four_touched_and_rolls_back() {
    let mut world = flat_world();
    world.start_x.items.push(6);
    world.start_y.items.push(5);
    let mut runtime = runtime(template());
    // A pre-existing bit is not owned by this invocation and must survive its
    // set-then-clear discipline.
    runtime.verify_bits[0] = 0x80;
    let before = snapshot(&world, &runtime);
    let mut request = call(5, 5);
    request.start_min = 2;

    let receipt = runtime.apply_add_mountain(&mut world, request).unwrap();

    assert_eq!(receipt.liberr, LIBERR_GENERAL);
    assert_eq!(
        receipt.rejection,
        Some(MountainRejection::StartDistance {
            mount_index: 0,
            player: 0,
            distance: 1,
        })
    );
    assert_eq!(receipt.unique_verify_cells, 1);
    assert_eq!(snapshot(&world, &runtime), before);
}

#[test]
fn mismatched_native_start_arrays_are_a_typed_transactional_stop() {
    let mut world = flat_world();
    world.start_x.items.push(6);
    let mut runtime = runtime(template());
    let before = snapshot(&world, &runtime);

    let error = runtime.apply_add_mountain(&mut world, call(5, 5));

    assert_eq!(
        error,
        Err(MountainAddRuntimeError::StartArrayLengthMismatch {
            x_count: 1,
            y_count: 0,
        })
    );
    assert_eq!(snapshot(&world, &runtime), before);
}

#[test]
fn a_preexisting_verify_bit_skips_the_start_distance_test_and_is_never_cleared() {
    let mut world = flat_world();
    world.start_x.items.push(5);
    world.start_y.items.push(5);
    let mut runtime = runtime(template());
    let index = (5 * world.xs + 5) as usize;
    runtime.verify_bits[index >> 3] |= 1 << (index & 7);
    let retained = runtime.verify_bits.clone();
    let mut request = call(5, 5);
    request.start_min = 1;

    let receipt = runtime.apply_add_mountain(&mut world, request).unwrap();

    assert_eq!(receipt.liberr, LIBERR_OK);
    assert_eq!(receipt.unique_verify_cells, 0);
    assert_eq!(runtime.verify_bits, retained);
}

#[test]
fn quick_verify_rejects_the_radius_one_offmap_edge_before_any_commit() {
    let mut world = flat_world();
    let mut runtime = runtime(template());
    let before = snapshot(&world, &runtime);

    let receipt = runtime.apply_add_mountain(&mut world, call(0, 0)).unwrap();

    assert_eq!(receipt.liberr, LIBERR_GENERAL);
    assert!(matches!(
        receipt.rejection,
        Some(MountainRejection::FootprintOutOfBounds { .. })
    ));
    assert_eq!(snapshot(&world, &runtime), before);
}

#[test]
fn quick_verify_checks_start_city_neighbors_after_the_center() {
    let mut world = flat_world();
    let reserved = (5 * world.xs + 6) as usize;
    world.start_city_locs[reserved >> 3] |= 1 << (reserved & 7);
    let mut runtime = runtime(template());

    let receipt = runtime.apply_add_mountain(&mut world, call(5, 5)).unwrap();

    assert!(matches!(
        receipt.rejection,
        Some(MountainRejection::StartCity {
            circle_index: Some(_),
            wx: 6,
            wy: 5,
            ..
        })
    ));
}

#[test]
fn late_array_metadata_failure_rolls_back_already_staged_world_writes() {
    let mut world = flat_world();
    let mut runtime = runtime(template());
    runtime.mountain_types.increment = 0;
    let before = snapshot(&world, &runtime);

    let error = runtime.apply_add_mountain(&mut world, call(5, 5));

    assert_eq!(error, Err(MountainAddRuntimeError::InvalidArrayMetadata));
    assert_eq!(snapshot(&world, &runtime), before);
}

#[test]
fn malformed_template_geometry_is_a_typed_stop_and_not_a_retail_rejection() {
    let malformed = MountainTemplateRuntime {
        mount_tiles: vec![GridOffset::new(-100, 0)],
        ..template()
    };
    let mut world = flat_world();
    let mut runtime = runtime(malformed);
    let before = snapshot(&world, &runtime);

    let error = runtime.apply_add_mountain(&mut world, call(5, 5));

    assert!(matches!(
        error,
        Err(MountainAddRuntimeError::TemplateTileOutOfBounds { .. })
    ));
    assert_eq!(snapshot(&world, &runtime), before);
}

#[test]
fn walked_array_metadata_uses_the_native_four_then_doubling_growth() {
    let mut world = flat_world();
    let mut runtime = runtime(template());
    for wx in 3..8 {
        let receipt = runtime.apply_add_mountain(&mut world, call(wx, 6)).unwrap();
        assert_eq!(receipt.liberr, LIBERR_OK);
    }

    for capacity in [
        runtime.mountain_loc_wcoords_x.capacity,
        runtime.mountain_loc_wcoords_y.capacity,
        runtime.mountain_locs.capacity,
        runtime.mountain_types.capacity,
    ] {
        assert_eq!(capacity, 8);
    }
    let bytes = runtime.walked_bytes();
    // Four non-empty arrays: each header is 4+4+2+1 bytes. Payload is
    // 5*(4+4+12+4) bytes.
    assert_eq!(bytes.len(), 4 * 11 + 5 * 24);
    assert_eq!(&bytes[0..4], &5i32.to_le_bytes());
    assert_eq!(&bytes[4..8], &8i32.to_le_bytes());
    assert_eq!(&bytes[8..10], &(-1i16).to_le_bytes());
    assert_eq!(bytes[10], 0);
}

#[test]
fn modes_not_reached_by_map_style_twelve_remain_explicitly_unsupported() {
    let mut world = flat_world();
    let mut runtime = runtime(template());
    let mut request = call(5, 5);
    request.verification_mode = 3;

    assert_eq!(
        runtime.apply_add_mountain(&mut world, request),
        Err(MountainAddRuntimeError::UnsupportedVerificationMode { mode: 3 })
    );
}
