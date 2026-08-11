#[path = "../src/region_centroid.rs"]
mod region_centroid;

use don_replay::continent::{execute_continent_prefix_with_regions_from_rng, ContinentStop};
use don_replay::fractal_boundary::resolve_tile_selection;
use don_replay::map_style::{ron_data_root_for_replay, MapStyleStaticData};
use don_replay::replay::Replay;
use don_sim::systems::regions::Regions;
use region_centroid::{
    execute_east_meets_west_centroids, execute_find_region_centroid, RegionCentroidError,
    EAST_MEETS_WEST_AFTER_CENTROID_LOOP_VA, MAP_ELIMINATE_EDGE_CANALS_VA,
    MAP_FIND_REGION_CENTROID_RETURN_VA, MAP_FIND_REGION_CENTROID_VA,
};
use std::path::{Path, PathBuf};

fn replay_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("ron-data/replays/multi")
        .join(name)
}

struct GeneratedCentroidFacts {
    sides: i32,
    sizes: Vec<i32>,
    sums: Vec<(i32, i32)>,
    x: Vec<i32>,
    y: Vec<i32>,
    stop_va: u32,
    pool_eliminations: usize,
}

fn generated_style19_centroids(name: &str) -> Option<GeneratedCentroidFacts> {
    let path = replay_path(name);
    if !path.is_file() {
        eprintln!("SKIPPED — NOT A PASS: missing {}", path.display());
        return None;
    }
    let replay = Replay::open(&path).expect("retail replay decodes");
    let root = ron_data_root_for_replay(&path).expect("replay is below ron-data");
    let style =
        MapStyleStaticData::load_from_ron_data(&root, replay.initial.info.settings.map_style)
            .expect("shipped style loads");
    assert_eq!(style.identity.ordinal, 19);
    let plan = replay
        .initial
        .reconstruct_items_with_style(style.clone())
        .expect("style matches replay selector");
    let selection = resolve_tile_selection(
        plan.inputs.seed,
        &style.default_source.path,
        &style.selected_source.path,
    )
    .expect("shipped tileset table selects");
    let mut map = replay
        .initial
        .reconstruct_world()
        .expect("ordinary replay has map dimensions");
    let receipt = execute_continent_prefix_with_regions_from_rng(
        &plan.inputs,
        &style,
        selection.main_random_state_after,
        &mut map.world,
        &mut map.generation_regions,
    )
    .expect("style-19 prefix reaches the centroid loop");
    let sides = i32::from(
        receipt
            .team_partition
            .as_ref()
            .expect("style 19 has a team partition")
            .continent_count,
    );
    let ContinentStop::EliminateEdgeCanals {
        primitive_va,
        centroids,
    } = &receipt.stop
    else {
        panic!("style 19 stopped at {:?}", receipt.stop);
    };
    Some(GeneratedCentroidFacts {
        sides,
        sizes: centroids.centroids.iter().map(|row| row.size).collect(),
        sums: centroids
            .centroids
            .iter()
            .map(|row| (row.x_sum, row.y_sum))
            .collect(),
        x: centroids.centroid_x.clone(),
        y: centroids.centroid_y.clone(),
        stop_va: *primitive_va,
        pool_eliminations: receipt.pool_eliminations.len(),
    })
}

#[test]
fn exact_body_uses_region_size_wrapping_sums_and_signed_truncation() {
    let mut regions = Regions::default();
    let row = &mut regions.list[3];
    row.size = 9;
    row.coords.capacity = 16;
    row.coords.items = vec![
        (i32::MAX, i32::MIN),
        (1, -7),
        (6, 5),
        (-3, 8),
        (2, -4),
        (4, 3),
        (-8, 11),
        (7, -9),
        (10, 2),
        // Native stops at Region::size and never reads this suffix.
        (999_999, 999_999),
    ];

    let got = execute_find_region_centroid(&regions, 3).unwrap();
    assert_eq!(got.primitive_va, MAP_FIND_REGION_CENTROID_VA);
    assert_eq!(got.return_va, MAP_FIND_REGION_CENTROID_RETURN_VA);
    assert_eq!(
        (got.vector_blocks, got.scalar_pairs, got.scalar_tail),
        (1, 0, true)
    );
    assert_eq!(
        (got.x_sum, got.y_sum),
        (-2_147_483_630, 2_147_483_657_u32 as i32)
    );
    assert_eq!((got.x, got.y), (-238_609_292, -238_609_293));
}

#[test]
fn malformed_native_storage_is_refused_before_any_read() {
    let mut regions = Regions::default();
    assert_eq!(
        execute_find_region_centroid(&regions, -1),
        Err(RegionCentroidError::InvalidRegion { region: -1 })
    );
    assert_eq!(
        execute_find_region_centroid(&regions, 128),
        Err(RegionCentroidError::InvalidRegion { region: 128 })
    );
    assert_eq!(
        execute_find_region_centroid(&regions, 4),
        Err(RegionCentroidError::NonPositiveSize { region: 4, size: 0 })
    );
    regions.list[4].size = 2;
    regions.list[4].coords.items.push((1, 2));
    assert_eq!(
        execute_find_region_centroid(&regions, 4),
        Err(RegionCentroidError::CoordinateStorageShort {
            region: 4,
            size: 2,
            coordinates: 1,
        })
    );
}

#[test]
fn both_checksum_bearing_style19_headers_execute_the_complete_centroid_loop() {
    let cases = [
        (
            "Playback___2018.12.01_18_33_16__Sat_.rcx",
            [2_777, 2_776],
            [(218_660, 138_867), (53_145, 130_347)],
            [78, 19],
            [50, 46],
        ),
        (
            "Playback___2019.03.24_11_56_19__Sun_.rcx",
            [2_778, 2_776],
            [(119_816, 58_098), (146_051, 222_666)],
            [43, 52],
            [20, 80],
        ),
    ];
    for (name, expected_sizes, expected_sums, expected_x, expected_y) in cases {
        let Some(got) = generated_style19_centroids(name) else {
            return;
        };
        assert_eq!(got.sides, 2, "{name}");
        assert_eq!(got.stop_va, MAP_ELIMINATE_EDGE_CANALS_VA, "{name}");
        assert_eq!(got.pool_eliminations, 1, "{name}");
        assert_eq!(got.sizes, expected_sizes, "{name}");
        assert_eq!(got.sums, expected_sums, "{name}");
        assert_eq!(got.x, expected_x, "{name}");
        assert_eq!(got.y, expected_y, "{name}");
    }
}

#[test]
fn caller_rejects_impossible_side_counts_before_indexing_regions() {
    let regions = Regions::default();
    for count in [-1, 0, 9] {
        assert_eq!(
            execute_east_meets_west_centroids(&regions, count),
            Err(RegionCentroidError::InvalidContinentCount { count })
        );
    }
    assert_eq!(EAST_MEETS_WEST_AFTER_CENTROID_LOOP_VA, 0x0069_6d07);
}
