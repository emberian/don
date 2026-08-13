use std::path::PathBuf;

use don_replay::build_init_prefix::{
    apply_build_init_prefix_from_terrain, BuildInitPrefixTerrainRequest, BuildTypeInitFacts,
    SourceBackedBuildInitPrefixError, SUBOBJECT_COORD_XOR,
};
use don_replay::terrain_height_runtime::{
    TerrainHeightAuthority, TerrainHeightError, TerrainHeightSource, TERRAIN_FIND_TCOORD_Z_BYTES,
    TERRAIN_FIND_TCOORD_Z_SHA256, TERRAIN_FIND_TCOORD_Z_VA,
};
use don_replay::world_owner_frontier::sha256;
use don_sim::systems::map_terrain::{tflag, World};
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
fn supported_pe_freezes_the_complete_height_body() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let exe = root.join("ron-bin/riseofnations.exe");
    if !exe.exists() {
        return;
    }
    let image = std::fs::read(exe).unwrap();
    let body = pe_span(
        &image,
        TERRAIN_FIND_TCOORD_Z_VA,
        TERRAIN_FIND_TCOORD_Z_BYTES as usize,
    );
    assert_eq!(hex(&sha256(body)), TERRAIN_FIND_TCOORD_Z_SHA256);
}
