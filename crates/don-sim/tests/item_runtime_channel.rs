use don_sim::item_runtime::ItemRuntimeError;
use don_sim::rng::Random;
use don_sim::systems::items::{snap_center, GoodyRules, LeaderGoody, ITEM_WALK_BYTES};
use don_sim::systems::map_terrain::World as TerrainWorld;
use don_sim::World;

fn map(xs: i32, ys: i32) -> TerrainWorld {
    let mut world = TerrainWorld::init_default_rules(xs, ys);
    for cell in &mut world.wdata {
        cell.land = 0;
    }
    world
}

#[test]
fn absent_and_initialized_empty_channels_are_distinct() {
    let mut world = World::with_capacity(1, 0x51);
    let terrain = map(8, 6);
    assert_eq!(world.items_channel(), Err(ItemRuntimeError::Unavailable));

    world.configure_items(&terrain);
    let report = world.items_channel().unwrap();
    assert_eq!(report.checksum, 1);
    assert_eq!(report.elements, 0);
    assert_eq!(report.bytes_walked, 0);
}

#[test]
fn placement_and_reveal_mutate_items_and_world_channels_together() {
    let mut world = World::with_capacity(1, 7);
    let mut terrain = map(8, 8);
    world.configure_items(&terrain);
    let world_before = terrain.checksum_sections().full;
    world.place_goody(&mut terrain, 2, 3, 17).unwrap();
    let placed = world.items_channel().unwrap();
    assert_ne!(placed.checksum, 1);
    assert_eq!(placed.elements, 1);
    assert_eq!(placed.bytes_walked, ITEM_WALK_BYTES as u32);
    assert_ne!(terrain.checksum_sections().full, world_before);

    world.reveal_goody(&terrain, 2, 3, 4).unwrap();
    let revealed = world.items_channel().unwrap();
    assert_ne!(revealed.checksum, placed.checksum);
    assert_eq!(revealed.elements, 1);
    assert_eq!(revealed.bytes_walked, ITEM_WALK_BYTES as u32);
    assert_eq!(
        world
            .item_runtime
            .as_ref()
            .unwrap()
            .items()
            .get(0)
            .unwrap()
            .ever_seen,
        1 << 4
    );
}

#[test]
fn collection_uses_world_rng_and_updates_both_checksum_channels() {
    let seed = 0x1234_u64;
    let mut world = World::with_capacity(1, seed);
    let mut terrain = map(8, 8);
    world.configure_items(&terrain);
    world.place_goody(&mut terrain, 3, 4, 0).unwrap();
    let placed_world_checksum = terrain.checksum_sections().full;
    world.frame = 1;

    let mut expected_rng = Random::new(seed as i32);
    for _ in 0..5 {
        expected_rng.get(0, 0xffff);
    }
    let expected_next = expected_rng.get(0, 0xffff);

    let mut leader = LeaderGoody::default();
    let (x, y) = snap_center(3, 4);
    let award = world
        .collect_goody(&mut terrain, &mut leader, &GoodyRules::RETAIL, x, y)
        .unwrap()
        .unwrap();
    assert_eq!(award.slot, 0);
    assert_eq!(
        award.draws, 5,
        "knowledge is skipped, all other buckets draw"
    );
    assert_eq!(leader.goody_box_resources, award.amount);
    assert_eq!(leader.bucket[award.resource], award.amount);
    assert_eq!(world.random.get(0, 0xffff), expected_next);

    let consumed = world.items_channel().unwrap();
    assert_eq!(consumed.checksum, 1);
    assert_eq!(consumed.elements, 0);
    assert_eq!(consumed.bytes_walked, 0);
    assert_ne!(terrain.checksum_sections().full, placed_world_checksum);
    assert_eq!(terrain.wdata(3, 4).down, -1);
    assert_eq!(terrain.wdata(3, 4).down_who, -1);
}

#[test]
fn mismatched_map_and_bad_placement_fail_without_mutation() {
    let mut world = World::with_capacity(1, 3);
    let mut terrain = map(4, 4);
    world.configure_items(&terrain);
    let before = world.items_channel().unwrap();
    assert_eq!(
        world.place_goody(&mut terrain, 4, 1, 0),
        Err(ItemRuntimeError::CellOutOfBounds { wx: 4, wy: 1 })
    );
    assert_eq!(world.items_channel().unwrap(), before);

    let mut wrong = map(5, 4);
    assert_eq!(
        world.place_goody(&mut wrong, 1, 1, 0),
        Err(ItemRuntimeError::MapMismatch {
            expected_xs: 4,
            expected_ys: 4,
            actual_xs: 5,
            actual_ys: 4,
        })
    );
    assert_eq!(world.items_channel().unwrap(), before);
}

#[test]
fn live_object_head_fails_closed_before_checksum_or_rng_mutation() {
    let seed = 0x44_u64;
    let mut world = World::with_capacity(1, seed);
    let mut terrain = map(4, 4);
    world.configure_items(&terrain);
    world.place_goody(&mut terrain, 1, 1, 0).unwrap();
    terrain.wdata_mut(1, 1).down = 7;
    terrain.wdata_mut(1, 1).down_who = 2;
    world.frame = 1;
    let item_before = world.items_channel().unwrap();
    let map_before = terrain.checksum_sections().full;
    let mut expected_rng = Random::new(seed as i32);
    let expected_next = expected_rng.get(0, 0xffff);
    let mut leader = LeaderGoody::default();
    let (x, y) = snap_center(1, 1);

    assert_eq!(
        world.collect_goody(&mut terrain, &mut leader, &GoodyRules::RETAIL, x, y),
        Err(ItemRuntimeError::ObjectChainUnavailable {
            down: 7,
            down_who: 2,
        })
    );
    assert_eq!(world.items_channel().unwrap(), item_before);
    assert_eq!(terrain.checksum_sections().full, map_before);
    assert_eq!(world.random.get(0, 0xffff), expected_next);
    assert_eq!(leader, LeaderGoody::default());
}

#[test]
fn frame_zero_consumes_item_but_not_rng_or_resources() {
    let seed = 99_u64;
    let mut world = World::with_capacity(1, seed);
    let mut terrain = map(4, 4);
    world.configure_items(&terrain);
    world.place_goody(&mut terrain, 1, 1, 0).unwrap();
    let mut expected_rng = Random::new(seed as i32);
    let expected_next = expected_rng.get(0, 0xffff);

    let mut leader = LeaderGoody::default();
    let (x, y) = snap_center(1, 1);
    assert_eq!(
        world
            .collect_goody(&mut terrain, &mut leader, &GoodyRules::RETAIL, x, y)
            .unwrap(),
        None
    );
    assert_eq!(leader, LeaderGoody::default());
    assert_eq!(world.random.get(0, 0xffff), expected_next);
    assert_eq!(world.items_channel().unwrap().elements, 0);
}
