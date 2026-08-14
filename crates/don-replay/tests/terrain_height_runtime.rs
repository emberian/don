use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use don_replay::build_init_prefix::{
    apply_build_init_prefix_from_terrain, BuildInitPrefixTerrainRequest, BuildTypeInitFacts,
    SourceBackedBuildInitPrefixError, SUBOBJECT_COORD_XOR,
};
use don_replay::terrain_height_runtime::{
    TerrainCoordInfoFlagsAuthority, TerrainFractalAuthority, TerrainHeightAuthority,
    TerrainHeightError, TerrainHeightPreMountainPlane, TerrainHeightScalarAuthority,
    TerrainHeightSource, TerrainHeightWorldgenInputs, FRACTAL_GET_HEIGHT_BYTES,
    FRACTAL_GET_HEIGHT_SHA256, FRACTAL_GET_HEIGHT_VA, FRACTAL_INIT_BYTES, FRACTAL_INIT_SHA256,
    FRACTAL_INIT_VA, TERRAIN_ADD_NEW_COORD_INFO_BYTES, TERRAIN_ADD_NEW_COORD_INFO_SHA256,
    TERRAIN_ADD_NEW_COORD_INFO_VA, TERRAIN_ADJUST_FOR_MOUNTAINS_BYTES,
    TERRAIN_ADJUST_FOR_MOUNTAINS_SHA256, TERRAIN_ADJUST_FOR_MOUNTAINS_VA,
    TERRAIN_BEACH_STEEPNESS_DATA_BYTES, TERRAIN_BEACH_STEEPNESS_DATA_SHA256,
    TERRAIN_BEACH_STEEPNESS_DATA_VA, TERRAIN_COORD_INFO_CTOR_BYTES, TERRAIN_COORD_INFO_CTOR_SHA256,
    TERRAIN_COORD_INFO_CTOR_VA, TERRAIN_DETERMINE_LAND_HEIGHT_COLOR_BYTES,
    TERRAIN_DETERMINE_LAND_HEIGHT_COLOR_SHA256, TERRAIN_DETERMINE_LAND_HEIGHT_COLOR_VA,
    TERRAIN_FILL_COORD_INFO_MAPPER_BYTES, TERRAIN_FILL_COORD_INFO_MAPPER_SHA256,
    TERRAIN_FILL_COORD_INFO_MAPPER_VA, TERRAIN_FILL_MOUNTAIN_DATA_BYTES,
    TERRAIN_FILL_MOUNTAIN_DATA_SHA256, TERRAIN_FILL_MOUNTAIN_DATA_VA,
    TERRAIN_FIND_CLOSEST_COORDINFO_BYTES, TERRAIN_FIND_CLOSEST_COORDINFO_SHA256,
    TERRAIN_FIND_CLOSEST_COORDINFO_VA, TERRAIN_FIND_TCOORD_Z_BYTES, TERRAIN_FIND_TCOORD_Z_SHA256,
    TERRAIN_FIND_TCOORD_Z_VA, TERRAIN_GENERATE_LAND_BYTES, TERRAIN_GENERATE_LAND_LISTS_BYTES,
    TERRAIN_GENERATE_LAND_LISTS_SHA256, TERRAIN_GENERATE_LAND_LISTS_VA,
    TERRAIN_GENERATE_LAND_SHA256, TERRAIN_GENERATE_LAND_VA, TERRAIN_GET_VERT_CODES_BYTES,
    TERRAIN_GET_VERT_CODES_SHA256, TERRAIN_GET_VERT_CODES_VA, TERRAIN_HEIGHT_SCALAR_WRITES_BYTES,
    TERRAIN_HEIGHT_SCALAR_WRITES_SHA256, TERRAIN_HEIGHT_SCALAR_WRITES_VA, TERRAIN_INIT_BYTES,
    TERRAIN_INIT_SHA256, TERRAIN_INIT_VA, TERRAIN_REFRESH_DATA_BYTES, TERRAIN_REFRESH_DATA_SHA256,
    TERRAIN_REFRESH_DATA_VA, TERRAIN_SMOOTH_TCOORD_BYTES, TERRAIN_SMOOTH_TCOORD_SHA256,
    TERRAIN_SMOOTH_TCOORD_VA,
};
use don_replay::world_owner_frontier::sha256;
use don_sim::systems::map_terrain::{tflag, wflag, World};
use don_sim::systems::mountain_add_runtime::{
    MountainAddRuntime, MountainLocationVertex, RetailMountainArray,
};
use don_sim::systems::mountain_template_producer::{
    load_mountain_template_catalog, MOUNTAIN_RANGE_INIT_SHA256, MOUNTAIN_RANGE_INIT_SIZE,
    MOUNTAIN_RANGE_INIT_VA,
};
use don_sim::systems::production::BuildData;

fn digest(byte: u8) -> [u8; 32] {
    [byte; 32]
}

fn plane(world: &World) -> TerrainHeightAuthority {
    TerrainHeightAuthority {
        master_land_height_bits: vec![0; ((world.tile_xs + 1) * (world.tile_ys + 1)) as usize],
        land_height_bits: 0,
        source: TerrainHeightSource::CompletedWorldgen,
        source_digest: digest(0x5a),
    }
}

fn set_pair(
    terrain: &mut TerrainHeightAuthority,
    world: &World,
    tx: i32,
    ty: i32,
    first: f32,
    second: f32,
) -> (usize, usize) {
    let stride = world.tile_xs as usize + 1;
    let a = (ty as usize + 1) * stride + tx as usize;
    let b = ty as usize * stride + tx as usize + 1;
    terrain.master_land_height_bits[a] = first.to_bits();
    terrain.master_land_height_bits[b] = second.to_bits();
    (a, b)
}

fn worldgen_inputs(world: &World) -> TerrainHeightWorldgenInputs {
    TerrainHeightWorldgenInputs {
        height_fractal: fractal(world, 0, 0xa1),
        height_fractal_detail: fractal(world, 0, 0xa2),
        coord_info_flags: vec![0; world.size as usize],
        land_height_bits: 30.0f32.to_bits(),
        coast_depth_bits: (-303.0f32).to_bits(),
        beach_steepness_bits: 1.0f32.to_bits(),
        coord_info_source_digest: digest(0xa5),
    }
}

fn fractal(world: &World, value: u8, source: u8) -> TerrainFractalAuthority {
    let xs = world.tile_xs + 1;
    let ys = world.tile_ys + 1;
    TerrainFractalAuthority {
        frac_columns: vec![value; ((xs + 1) * (ys + 1)) as usize],
        xs,
        ys,
        flags: 1,
        partitions: [-1; 16],
        random_seed: 0x1234_0000 | u32::from(source),
        x_inc_bits: 1.0f64.to_bits(),
        y_inc_bits: 1.0f64.to_bits(),
        initialized_source_digest: digest(source),
    }
}

fn request(x: i32, y: i32) -> BuildInitPrefixTerrainRequest {
    BuildInitPrefixTerrainRequest {
        owner: 2,
        object_id: 2000,
        type_index: 414,
        type_rows: 800,
        snapped_x: x,
        snapped_y: y,
        owner_uid_before: 77,
        max_age_source_byte: 0x31,
        type_facts: BuildTypeInitFacts {
            sets_flat_flag: true,
            sets_detector_flag: false,
        },
    }
}

static MOUNTAIN_TEMP_ID: AtomicU64 = AtomicU64::new(0);

fn one_pixel_mountain_catalog(
    red: u8,
    height: &str,
) -> (
    PathBuf,
    don_sim::systems::mountain_template_producer::MountainTemplateCatalog,
) {
    let root = std::env::temp_dir().join(format!(
        "don-terrain-height-mountain-{}-{}",
        std::process::id(),
        MOUNTAIN_TEMP_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join("art")).unwrap();
    let xml = format!(
        "<ROOT><MOUNTAINS><MOUNTAIN area=\"sm\" height=\"{height}\"><TEMPLATE_TEX file=\"art/one.tga\"/><MAIN_ALPHA_TEX file=\"main\"/><RING_ALPHA_TEX file=\"ring\"/></MOUNTAIN></MOUNTAINS></ROOT>"
    );
    let xml_path = root.join("effects_graphics.xml");
    fs::write(&xml_path, xml).unwrap();
    let mut tga = vec![0u8; 18 + 36 * 36 * 4];
    tga[2] = 2;
    tga[12..14].copy_from_slice(&36u16.to_le_bytes());
    tga[14..16].copy_from_slice(&36u16.to_le_bytes());
    tga[16] = 32;
    tga[17] = 0x28;
    let sample = 18 + (2 * 36 + 2) * 4;
    tga[sample + 2] = red;
    tga[sample + 3] = 1;
    fs::write(root.join("art/one.tga"), tga).unwrap();
    let catalog = load_mountain_template_catalog(&xml_path, &root).unwrap();
    (root, catalog)
}

fn retail_array<T>(items: Vec<T>) -> RetailMountainArray<T> {
    RetailMountainArray {
        items,
        capacity: 4,
        increment: -1,
        flags: 0,
    }
}

#[test]
fn exact_diagonal_vertices_produce_saved_village_height() {
    let world = World::init_default_rules(2, 2);
    let mut terrain = plane(&world);
    let (a, b) = set_pair(&mut terrain, &world, 3, 4, 320.0, 700.0);
    terrain.master_land_height_bits[0] = 20_000.0f32.to_bits();

    let receipt = terrain.find_tcoord_z(&world, 3, 4, 1).unwrap();
    assert_eq!(receipt.height_indices, Some((a, b)));
    assert_eq!(
        receipt.height_bits,
        Some([320.0f32.to_bits(), 700.0f32.to_bits()])
    );
    assert_eq!((receipt.raw_z, receipt.returned_z), (Some(510), 510));
    assert!(!receipt.water && !receipt.uninitialized_fallback);

    let mut changed = terrain.clone();
    changed.master_land_height_bits[a] = 322.0f32.to_bits();
    assert_eq!(
        changed.find_tcoord_z(&world, 3, 4, 1).unwrap().returned_z,
        511
    );
    let mut irrelevant = terrain.clone();
    irrelevant.master_land_height_bits[0] = (-30_000.0f32).to_bits();
    assert_eq!(irrelevant.find_tcoord_z(&world, 3, 4, 1).unwrap(), receipt);
}

#[test]
fn water_negative_and_uninitialized_gates_match_the_pe() {
    let mut world = World::init_default_rules(2, 2);
    let mut terrain = plane(&world);
    set_pair(&mut terrain, &world, 2, 5, -900.0, 200.0);
    assert_eq!(
        terrain.find_tcoord_z(&world, 2, 5, 0).unwrap().returned_z,
        -350
    );
    assert_eq!(
        terrain.find_tcoord_z(&world, 2, 5, 1).unwrap().returned_z,
        0
    );

    let tile = (5 * world.tile_xs + 2) as usize;
    world.tdata[tile] = tflag::SURFACE_WATER;
    let water = terrain.find_tcoord_z(&world, 2, 5, 0).unwrap();
    assert!(water.water);
    assert_eq!((water.raw_z, water.returned_z), (None, 0));
    assert_eq!((water.height_indices, water.height_bits), (None, None));
    let mut poisoned_water = terrain.clone();
    set_pair(&mut poisoned_water, &world, 2, 5, f32::NAN, f32::INFINITY);
    assert_eq!(
        poisoned_water.find_tcoord_z(&world, 2, 5, 0).unwrap(),
        water
    );

    let fallback = TerrainHeightAuthority {
        master_land_height_bits: Vec::new(),
        land_height_bits: (-42.75f32).to_bits(),
        source: TerrainHeightSource::RetailLiveSnapshot,
        source_digest: digest(0x33),
    };
    let receipt = fallback.find_tcoord_z(&world, -999, 999, 1).unwrap();
    assert!(receipt.uninitialized_fallback);
    // The early return precedes the negative-zero gate in the native body.
    assert_eq!((receipt.raw_z, receipt.returned_z), (Some(-42), -42));
}

#[test]
fn sse_invalid_conversion_and_shape_errors_fail_or_return_exactly() {
    let world = World::init_default_rules(1, 1);
    let mut terrain = plane(&world);
    set_pair(&mut terrain, &world, 0, 0, f32::NAN, 1.0);
    assert_eq!(
        terrain.find_tcoord_z(&world, 0, 0, 0).unwrap().returned_z,
        i32::MIN
    );
    assert_eq!(
        terrain.find_tcoord_z(&world, 0, 0, 1).unwrap().returned_z,
        0
    );

    let mut short = terrain.clone();
    short.master_land_height_bits.pop();
    assert!(matches!(
        short.find_tcoord_z(&world, 0, 0, 1),
        Err(TerrainHeightError::HeightPlaneLengthMismatch { .. })
    ));
    assert!(matches!(
        terrain.find_tcoord_z(&world, 4, 0, 1),
        Err(TerrainHeightError::TcoordOutsideHeightPlane { .. })
    ));
    let mut anonymous = terrain;
    anonymous.source_digest = [0; 32];
    assert_eq!(
        anonymous.find_tcoord_z(&world, 0, 0, 1),
        Err(TerrainHeightError::MissingSourceIdentity)
    );
}

#[test]
fn build_prefix_consumes_the_height_plane_without_a_raw_z_input() {
    let world = World::init_default_rules(2, 2);
    let mut terrain = plane(&world);
    set_pair(&mut terrain, &world, 3, 4, 320.0, 700.0);
    let x = 3 * 192 + 24;
    let y = 4 * 192 + 24;
    let mut build = BuildData::default();

    let receipt =
        apply_build_init_prefix_from_terrain(&mut build, request(x, y), &world, &terrain).unwrap();
    assert_eq!(receipt.terrain.tcoord, (3, 4));
    assert_eq!(receipt.terrain.returned_z, 510);
    assert_eq!(receipt.prefix.terrain_z, 510);
    assert_eq!(
        i32::from_le_bytes(build.other[0x0c..0x10].try_into().unwrap()),
        510 ^ SUBOBJECT_COORD_XOR
    );

    let mut bad_build = BuildData::default();
    bad_build.damage = 0x1234;
    let before = bad_build.image();
    let mut bad_request = request(x, y);
    bad_request.type_index = 900;
    assert!(matches!(
        apply_build_init_prefix_from_terrain(&mut bad_build, bad_request, &world, &terrain),
        Err(SourceBackedBuildInitPrefixError::Prefix(_))
    ));
    assert_eq!(bad_build.image(), before);

    let fallback = TerrainHeightAuthority {
        master_land_height_bits: Vec::new(),
        land_height_bits: 0,
        source: TerrainHeightSource::RetailLiveSnapshot,
        source_digest: digest(0x44),
    };
    assert_eq!(
        apply_build_init_prefix_from_terrain(&mut bad_build, request(x, y), &world, &fallback,),
        Err(SourceBackedBuildInitPrefixError::UninitializedTerrainAtBuildInit)
    );
    assert_eq!(bad_build.image(), before);
}

#[test]
fn initialized_fractals_derive_the_pre_mountain_plane() {
    let world = World::init_default_rules(2, 2);
    let width = world.tile_xs as usize + 1;
    let mut inputs = worldgen_inputs(&world);
    let first = 5 * width + 3;
    inputs.height_fractal.frac_columns.fill(20);

    let (terrain, receipt) =
        TerrainHeightPreMountainPlane::from_completed_worldgen(&world, &inputs).unwrap();
    assert_eq!(receipt.vertices, width * (world.tile_ys as usize + 1));
    assert_eq!(receipt.fractal_vertices, receipt.vertices);
    assert_eq!(receipt.smoothing_vertices, 0);
    assert_eq!(terrain.master_land_height_bits[first], 180.0f32.to_bits());
    assert_eq!(receipt.fractal_get_height_va, FRACTAL_GET_HEIGHT_VA);
    assert_ne!(receipt.height_fractal_digest, [0; 32]);
    assert_ne!(receipt.height_fractal_detail_digest, [0; 32]);
    assert!(!receipt.final_query_authority);
    assert_eq!(
        (
            receipt.remaining_adjust_for_mountains_va,
            receipt.remaining_fill_mountain_data_va,
        ),
        (
            TERRAIN_ADJUST_FOR_MOUNTAINS_VA,
            TERRAIN_FILL_MOUNTAIN_DATA_VA,
        )
    );

    let mut changed_inputs = inputs;
    changed_inputs.height_fractal.frac_columns.fill(21);
    let (changed, changed_receipt) =
        TerrainHeightPreMountainPlane::from_completed_worldgen(&world, &changed_inputs).unwrap();
    assert_ne!(
        changed.master_land_height_bits,
        terrain.master_land_height_bits
    );
    assert_ne!(
        changed_receipt.derived_plane_digest,
        receipt.derived_plane_digest
    );
}

#[test]
fn installed_vertices_and_retained_placements_finish_the_query_height_plane() {
    let world = World::init_default_rules(2, 2);
    let (root, catalog) = one_pixel_mountain_catalog(63, "0.031287279");
    let location = MountainLocationVertex {
        x_bits: 768.0f32.to_bits(),
        y_bits: 768.0f32.to_bits(),
        z_bits: 0,
    };
    let runtime = MountainAddRuntime {
        templates: vec![Some(catalog.templates[0].clone())],
        verify_bits: vec![0; (world.size as usize + 7) / 8],
        mountain_loc_wcoords_x: retail_array(vec![1]),
        mountain_loc_wcoords_y: retail_array(vec![1]),
        mountain_locs: retail_array(vec![location]),
        mountain_types: retail_array(vec![0]),
    };
    let mut pre = TerrainHeightPreMountainPlane {
        master_land_height_bits: vec![
            100.0f32.to_bits();
            ((world.tile_xs + 1) * (world.tile_ys + 1)) as usize
        ],
        land_height_bits: 30.0f32.to_bits(),
        source_digest: digest(0xc1),
    };
    let before = pre.master_land_height_bits.clone();
    let expected_z = 0x3cbd_f7af;
    assert_eq!(catalog.tcoord_vertices[0][0].z_bits, expected_z);

    let (authority, receipt) = pre
        .clone()
        .finish_new_map_mountains(&world, &catalog, &runtime)
        .unwrap();
    assert_eq!(
        authority.master_land_height_bits[0],
        (100.0f32 + f32::from_bits(expected_z)).to_bits()
    );
    assert_eq!(authority.master_land_height_bits[1..], before[1..]);
    assert_eq!((receipt.placements, receipt.source_vertices), (1, 1));
    assert_eq!(
        (receipt.matched_vertices, receipt.unmatched_vertices),
        (1, 0)
    );
    assert_eq!(receipt.mountain_range_init_va, MOUNTAIN_RANGE_INIT_VA);
    assert_eq!(
        receipt.mountain_range_init_sha256,
        MOUNTAIN_RANGE_INIT_SHA256
    );
    assert!(receipt.final_query_authority && !receipt.load_rebuild_mode);
    assert_eq!(authority.source_digest, receipt.final_plane_digest);

    pre.master_land_height_bits.pop();
    assert!(matches!(
        pre.finish_new_map_mountains(&world, &catalog, &runtime),
        Err(TerrainHeightError::HeightPlaneLengthMismatch { .. })
    ));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn mountain_height_join_rejects_runtime_catalog_and_parallel_array_drift() {
    let world = World::init_default_rules(2, 2);
    let (root, catalog) = one_pixel_mountain_catalog(63, "450");
    let pre = TerrainHeightPreMountainPlane {
        master_land_height_bits: vec![0; ((world.tile_xs + 1) * (world.tile_ys + 1)) as usize],
        land_height_bits: 0,
        source_digest: digest(0xc2),
    };
    let mut runtime = MountainAddRuntime::new(world.size as usize, Vec::new());
    assert_eq!(
        pre.clone()
            .finish_new_map_mountains(&world, &catalog, &runtime),
        Err(TerrainHeightError::MountainRuntimeCatalogMismatch)
    );

    runtime.templates = vec![Some(catalog.templates[0].clone())];
    runtime.mountain_types.items.push(0);
    assert!(matches!(
        pre.finish_new_map_mountains(&world, &catalog, &runtime),
        Err(TerrainHeightError::MountainPlacementLengthMismatch { .. })
    ));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn fractal_sampler_executes_bilinear_partition_and_percentage_paths() {
    let world = World::init_default_rules(1, 1);
    let mut source = fractal(&world, 0, 0xb1);
    let stride = source.ys as usize + 1;
    source.frac_columns[stride] = 10;
    source.frac_columns[1] = 20;
    source.frac_columns[stride + 1] = 30;
    assert_eq!(source.get_height(0, 0).unwrap(), 15);

    source.partitions = [
        10, 15, 20, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    ];
    assert_eq!(source.get_height(0, 0).unwrap(), 2);
    source.partitions = [-1; 16];
    source.flags |= 2;
    assert_eq!(source.get_height(0, 0).unwrap(), 5);

    let mut bad_increment = source.clone();
    bad_increment.x_inc_bits = 0.5f64.to_bits();
    assert!(matches!(
        bad_increment.get_height(0, 0),
        Err(TerrainHeightError::InvalidFractalIncrement { .. })
    ));
    assert!(matches!(
        source.get_height(i32::MAX, 0),
        Err(TerrainHeightError::FractalSampleOutsideInitializedGrid { .. })
    ));
    let mut anonymous = source;
    anonymous.initialized_source_digest = [0; 32];
    assert_eq!(
        anonymous.get_height(0, 0),
        Err(TerrainHeightError::MissingSourceIdentity)
    );
}

#[test]
fn world_and_coordinfo_codes_drive_native_height_branches() {
    let mut world = World::init_default_rules(2, 2);
    let width = world.tile_xs as usize + 1;

    let mut mountain = worldgen_inputs(&world);
    mountain.coord_info_flags[0] = 0x1000;
    let (terrain, receipt) =
        TerrainHeightPreMountainPlane::from_completed_worldgen(&world, &mountain).unwrap();
    assert_eq!(
        terrain.master_land_height_bits[2 * width + 2],
        (-273.0f32).to_bits()
    );
    assert!(receipt.deep_water_vertices > 0);

    let mut fixed = worldgen_inputs(&world);
    fixed.coord_info_flags[0] = 0x2;
    let (terrain, receipt) =
        TerrainHeightPreMountainPlane::from_completed_worldgen(&world, &fixed).unwrap();
    assert_eq!(
        terrain.master_land_height_bits[2 * width + 2],
        100.0f32.to_bits()
    );
    assert!(receipt.fixed_height_vertices > 0);

    let mut zero = worldgen_inputs(&world);
    zero.coord_info_flags[0] = 0x4;
    let (terrain, receipt) =
        TerrainHeightPreMountainPlane::from_completed_worldgen(&world, &zero).unwrap();
    assert_eq!(terrain.master_land_height_bits[2 * width + 2], 0);
    assert!(receipt.coordinfo_zero_vertices > 0);

    world.tdata[(3 * world.tile_xs + 2) as usize] |= tflag::RIVER;
    let (terrain, receipt) =
        TerrainHeightPreMountainPlane::from_completed_worldgen(&world, &worldgen_inputs(&world))
            .unwrap();
    assert_eq!(terrain.master_land_height_bits[3 * width + 2], 0);
    assert_eq!(terrain.master_land_height_bits[4 * width + 3], 0);
    assert!(receipt.locked_zero_vertices >= 4);
}

#[test]
fn coast_distance_smoothing_and_worldgen_shape_are_fail_closed() {
    let world = World::init_default_rules(2, 2);
    let mut inputs = worldgen_inputs(&world);
    inputs.height_fractal.frac_columns.fill(20);
    inputs.coord_info_flags[0] = 0x20;
    let (terrain, receipt) =
        TerrainHeightPreMountainPlane::from_completed_worldgen(&world, &inputs).unwrap();
    assert!(receipt.smoothing_vertices > 0);
    assert!(receipt.smoothing_passes >= receipt.smoothing_vertices);
    assert!(terrain
        .master_land_height_bits
        .iter()
        .any(|&bits| bits != 180.0f32.to_bits()));

    let mut short_fractal = inputs.clone();
    short_fractal.height_fractal.frac_columns.pop();
    assert!(matches!(
        TerrainHeightPreMountainPlane::from_completed_worldgen(&world, &short_fractal),
        Err(TerrainHeightError::InvalidFractalShape { .. })
    ));
    let mut short_flags = inputs.clone();
    short_flags.coord_info_flags.pop();
    assert!(matches!(
        TerrainHeightPreMountainPlane::from_completed_worldgen(&world, &short_flags),
        Err(TerrainHeightError::CoordInfoFlagsLengthMismatch { .. })
    ));
    let mut anonymous = inputs;
    anonymous.coord_info_source_digest = [0; 32];
    assert_eq!(
        TerrainHeightPreMountainPlane::from_completed_worldgen(&world, &anonymous),
        Err(TerrainHeightError::MissingSourceIdentity)
    );
}

#[test]
fn smoothing_admission_preserves_the_retail_x_extent_for_both_axes() {
    // `determine_land_height_color` compares both x and y with `4 * world_xs` before
    // appending to `temp_smooth`, even on a rectangular map. Keep that binary quirk locked.
    let world = World::init_default_rules(2, 3);
    let mut inputs = worldgen_inputs(&world);
    inputs.coord_info_flags.fill(0x20);

    let (_, receipt) =
        TerrainHeightPreMountainPlane::from_completed_worldgen(&world, &inputs).unwrap();
    assert_eq!(receipt.smoothing_vertices, 8 * 8);
    assert_eq!(receipt.smoothing_passes, 8 * 8 + (8 * 8 - 4 * 4));
}

#[test]
fn coord_info_authority_is_derived_from_world_land_and_coast_only() {
    let mut world = World::init_default_rules(5, 5);
    world.wdata_mut(2, 2).flags = wflag::COAST;
    world.wdata_mut(1, 2).land = 0;
    let (authority, receipt) = TerrainCoordInfoFlagsAuthority::from_world(&world).unwrap();
    let at = |x: i32, y: i32| authority.flags()[(y * world.xs + x) as usize];

    assert_eq!(at(2, 2), 0x8004);
    assert_eq!(at(1, 2), 0x0020);
    assert_eq!(at(2, 0), 0x1080);
    assert_eq!(receipt.current_coast_cells, 1);
    assert_eq!(receipt.fertile_near_coast_cells, 1);
    assert_eq!(receipt.authority_digest, authority.authority_digest());
}

#[test]
fn supported_pe_scalar_authority_names_the_deep_water_inputs() {
    let (authority, receipt) = TerrainHeightScalarAuthority::supported_retail_defaults();
    assert_eq!(authority.land_height_bits(), 30.0f32.to_bits());
    assert_eq!(authority.coast_depth_bits(), (-303.0f32).to_bits());
    assert_eq!(authority.beach_steepness_bits(), 1.0f32.to_bits());
    assert_eq!(authority.source_digest(), receipt.source_digest);
}

fn pe_span(image: &[u8], va: u32, size: usize) -> &[u8] {
    let pe = u32::from_le_bytes(image[0x3c..0x40].try_into().unwrap()) as usize;
    assert_eq!(&image[pe..pe + 4], b"PE\0\0");
    let sections = u16::from_le_bytes(image[pe + 6..pe + 8].try_into().unwrap()) as usize;
    let optional_size = u16::from_le_bytes(image[pe + 20..pe + 22].try_into().unwrap()) as usize;
    let optional = pe + 24;
    let image_base = u32::from_le_bytes(image[optional + 28..optional + 32].try_into().unwrap());
    let rva = va - image_base;
    let table = optional + optional_size;
    for index in 0..sections {
        let section = table + index * 40;
        let virtual_size = u32::from_le_bytes(image[section + 8..section + 12].try_into().unwrap());
        let virtual_address =
            u32::from_le_bytes(image[section + 12..section + 16].try_into().unwrap());
        let raw_size = u32::from_le_bytes(image[section + 16..section + 20].try_into().unwrap());
        let raw = u32::from_le_bytes(image[section + 20..section + 24].try_into().unwrap());
        if virtual_address <= rva && rva < virtual_address + virtual_size.max(raw_size) {
            let offset = (raw + rva - virtual_address) as usize;
            return &image[offset..offset + size];
        }
    }
    panic!("VA {va:#x} is outside the PE")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn supported_pe_freezes_the_height_query_and_producer_bodies() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let exe = root.join("ron-bin/riseofnations.exe");
    if !exe.exists() {
        return;
    }
    let image = std::fs::read(exe).unwrap();
    for (va, bytes, expected) in [
        (
            FRACTAL_GET_HEIGHT_VA,
            FRACTAL_GET_HEIGHT_BYTES,
            FRACTAL_GET_HEIGHT_SHA256,
        ),
        (FRACTAL_INIT_VA, FRACTAL_INIT_BYTES, FRACTAL_INIT_SHA256),
        (
            TERRAIN_REFRESH_DATA_VA,
            TERRAIN_REFRESH_DATA_BYTES,
            TERRAIN_REFRESH_DATA_SHA256,
        ),
        (TERRAIN_INIT_VA, TERRAIN_INIT_BYTES, TERRAIN_INIT_SHA256),
        (
            TERRAIN_HEIGHT_SCALAR_WRITES_VA,
            TERRAIN_HEIGHT_SCALAR_WRITES_BYTES,
            TERRAIN_HEIGHT_SCALAR_WRITES_SHA256,
        ),
        (
            TERRAIN_BEACH_STEEPNESS_DATA_VA,
            TERRAIN_BEACH_STEEPNESS_DATA_BYTES,
            TERRAIN_BEACH_STEEPNESS_DATA_SHA256,
        ),
        (
            TERRAIN_GENERATE_LAND_LISTS_VA,
            TERRAIN_GENERATE_LAND_LISTS_BYTES,
            TERRAIN_GENERATE_LAND_LISTS_SHA256,
        ),
        (
            TERRAIN_ADD_NEW_COORD_INFO_VA,
            TERRAIN_ADD_NEW_COORD_INFO_BYTES,
            TERRAIN_ADD_NEW_COORD_INFO_SHA256,
        ),
        (
            TERRAIN_FILL_COORD_INFO_MAPPER_VA,
            TERRAIN_FILL_COORD_INFO_MAPPER_BYTES,
            TERRAIN_FILL_COORD_INFO_MAPPER_SHA256,
        ),
        (
            TERRAIN_COORD_INFO_CTOR_VA,
            TERRAIN_COORD_INFO_CTOR_BYTES,
            TERRAIN_COORD_INFO_CTOR_SHA256,
        ),
        (
            TERRAIN_FIND_TCOORD_Z_VA,
            TERRAIN_FIND_TCOORD_Z_BYTES,
            TERRAIN_FIND_TCOORD_Z_SHA256,
        ),
        (
            TERRAIN_GENERATE_LAND_VA,
            TERRAIN_GENERATE_LAND_BYTES,
            TERRAIN_GENERATE_LAND_SHA256,
        ),
        (
            TERRAIN_GET_VERT_CODES_VA,
            TERRAIN_GET_VERT_CODES_BYTES,
            TERRAIN_GET_VERT_CODES_SHA256,
        ),
        (
            TERRAIN_DETERMINE_LAND_HEIGHT_COLOR_VA,
            TERRAIN_DETERMINE_LAND_HEIGHT_COLOR_BYTES,
            TERRAIN_DETERMINE_LAND_HEIGHT_COLOR_SHA256,
        ),
        (
            TERRAIN_FIND_CLOSEST_COORDINFO_VA,
            TERRAIN_FIND_CLOSEST_COORDINFO_BYTES,
            TERRAIN_FIND_CLOSEST_COORDINFO_SHA256,
        ),
        (
            TERRAIN_SMOOTH_TCOORD_VA,
            TERRAIN_SMOOTH_TCOORD_BYTES,
            TERRAIN_SMOOTH_TCOORD_SHA256,
        ),
        (
            TERRAIN_ADJUST_FOR_MOUNTAINS_VA,
            TERRAIN_ADJUST_FOR_MOUNTAINS_BYTES,
            TERRAIN_ADJUST_FOR_MOUNTAINS_SHA256,
        ),
        (
            TERRAIN_FILL_MOUNTAIN_DATA_VA,
            TERRAIN_FILL_MOUNTAIN_DATA_BYTES,
            TERRAIN_FILL_MOUNTAIN_DATA_SHA256,
        ),
        (
            MOUNTAIN_RANGE_INIT_VA,
            MOUNTAIN_RANGE_INIT_SIZE,
            MOUNTAIN_RANGE_INIT_SHA256,
        ),
    ] {
        assert_eq!(hex(&sha256(pe_span(&image, va, bytes as usize))), expected);
    }
}
