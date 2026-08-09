use don_replay::check_all::CheckAll;
use don_replay::checksum::Channel;
use don_replay::state::{ItemProjection, SimBridge, SimState};
use don_sim::item_runtime::ItemRuntimeError;
use don_sim::systems::items::{snap_center, GoodyRules, LeaderGoody, ITEM_WALK_BYTES};
use don_sim::systems::map_terrain::World as TerrainWorld;
use don_sim::World;

fn fertile_map(xs: i32, ys: i32) -> TerrainWorld {
    let mut map = TerrainWorld::init_default_rules(xs, ys);
    for cell in &mut map.wdata {
        cell.land = 0;
    }
    map
}

fn item_report(report: &CheckAll) -> &don_replay::check_all::ChannelReport {
    &report.per[Channel::Items as usize]
}

#[test]
fn initialized_empty_is_installed_while_unavailable_is_absent() {
    let mut state = SimState::new();
    let mut world = World::with_capacity(1, 7);

    let absent = SimBridge::populate(&world, &mut state);
    let absent_check = CheckAll::of_state(&state);
    assert_eq!(absent.items, ItemProjection::Unavailable);
    assert!(!item_report(&absent_check).installed);
    assert_eq!(item_report(&absent_check).elements, 0);
    assert_eq!(item_report(&absent_check).bytes, 0);
    assert_eq!(item_report(&absent_check).value, 1);

    let map = fertile_map(4, 3);
    world.configure_items(&map);
    let empty = SimBridge::populate_with_map(&world, &map, 0, &mut state);
    let empty_check = CheckAll::of_state(&state);
    assert_eq!(empty.items, ItemProjection::Empty);
    assert!(item_report(&empty_check).installed);
    assert_eq!(item_report(&empty_check).elements, 0);
    assert_eq!(item_report(&empty_check).bytes, 0);
    assert_eq!(item_report(&empty_check).value, 1);
    assert!(item_report(&empty_check).outcome.ops_executed > 0);
}

#[test]
fn live_runtime_projects_exact_items_and_world_words() {
    let mut state = SimState::new();
    let mut world = World::with_capacity(1, 11);
    let mut map = fertile_map(6, 5);
    world.configure_items(&map);
    world.place_goody(&mut map, 2, 3, 19).unwrap();
    let runtime = world.items_channel().unwrap();

    let bridge = SimBridge::populate_with_map(&world, &map, 0, &mut state);
    let projected = CheckAll::of_state(&state);
    assert_eq!(bridge.items, ItemProjection::Live);
    assert_eq!(bridge.elements[Channel::Items as usize], 1);
    assert_eq!(item_report(&projected).elements, runtime.elements);
    assert_eq!(
        item_report(&projected).bytes,
        u64::from(runtime.bytes_walked)
    );
    assert_eq!(item_report(&projected).value, runtime.checksum);
    assert_eq!(item_report(&projected).unsourced_walked_bytes, 0);
    assert_eq!(runtime.bytes_walked, ITEM_WALK_BYTES as u32);
    assert_eq!(
        projected.channels.get(Channel::World),
        map.checksum_sections().full
    );

    let before_items = item_report(&projected).value;
    let before_world = projected.channels.get(Channel::World);
    world.reveal_goody(&map, 2, 3, 5).unwrap();
    SimBridge::populate_with_map(&world, &map, 0, &mut state);
    let revealed = CheckAll::of_state(&state);
    assert_ne!(item_report(&revealed).value, before_items);
    assert_eq!(revealed.channels.get(Channel::World), before_world);
}

#[test]
fn collection_reprojects_empty_items_and_mutated_world() {
    let mut state = SimState::new();
    let mut world = World::with_capacity(1, 17);
    let mut map = fertile_map(5, 5);
    world.configure_items(&map);
    world.place_goody(&mut map, 1, 2, 0).unwrap();
    world.frame = 1;
    SimBridge::populate_with_map(&world, &map, 0, &mut state);
    let placed = CheckAll::of_state(&state);

    let mut leader = LeaderGoody::default();
    let (x, y) = snap_center(1, 2);
    world
        .collect_goody(&mut map, &mut leader, &GoodyRules::RETAIL, x, y)
        .unwrap()
        .unwrap();
    let bridge = SimBridge::populate_with_map(&world, &map, 0, &mut state);
    let consumed = CheckAll::of_state(&state);

    assert_eq!(bridge.items, ItemProjection::Empty);
    assert!(item_report(&consumed).installed);
    assert_eq!(item_report(&consumed).value, 1);
    assert_eq!(item_report(&consumed).elements, 0);
    assert_eq!(item_report(&consumed).bytes, 0);
    assert_ne!(
        consumed.channels.get(Channel::World),
        placed.channels.get(Channel::World)
    );
}

#[test]
fn mismatched_map_shape_clears_a_previously_live_projection() {
    let mut state = SimState::new();
    let mut world = World::with_capacity(1, 23);
    let mut attached = fertile_map(4, 4);
    world.configure_items(&attached);
    world.place_goody(&mut attached, 1, 1, 0).unwrap();
    SimBridge::populate_with_map(&world, &attached, 0, &mut state);
    assert!(state.channel_is_installed(Channel::Items as usize));

    let wrong = fertile_map(5, 4);
    let bridge = SimBridge::populate_with_map(&world, &wrong, 0, &mut state);
    assert_eq!(
        bridge.items,
        ItemProjection::MapMismatch {
            item_xs: 4,
            item_ys: 4,
            map_xs: 5,
            map_ys: 4,
        }
    );
    let projected = CheckAll::of_state(&state);
    assert!(!item_report(&projected).installed);
    assert_eq!(item_report(&projected).value, 1);
    assert_eq!(item_report(&projected).elements, 0);
    assert_eq!(item_report(&projected).bytes, 0);
}

#[test]
fn failed_object_chain_collection_leaves_both_projections_unchanged() {
    let mut state = SimState::new();
    let mut world = World::with_capacity(1, 29);
    let mut map = fertile_map(4, 4);
    world.configure_items(&map);
    world.place_goody(&mut map, 2, 2, 0).unwrap();
    map.wdata_mut(2, 2).down = 9;
    map.wdata_mut(2, 2).down_who = 3;
    world.frame = 1;
    SimBridge::populate_with_map(&world, &map, 0, &mut state);
    let before = CheckAll::of_state(&state);

    let mut leader = LeaderGoody::default();
    let (x, y) = snap_center(2, 2);
    assert_eq!(
        world.collect_goody(&mut map, &mut leader, &GoodyRules::RETAIL, x, y),
        Err(ItemRuntimeError::ObjectChainUnavailable {
            down: 9,
            down_who: 3,
        })
    );
    SimBridge::populate_with_map(&world, &map, 0, &mut state);
    let after = CheckAll::of_state(&state);
    assert_eq!(
        item_report(&after).value,
        item_report(&before).value,
        "failed mutation must not move channel 10"
    );
    assert_eq!(
        after.channels.get(Channel::World),
        before.channels.get(Channel::World),
        "failed mutation must not move channel 12"
    );
}

#[test]
fn repopulating_from_an_unconfigured_world_clears_stale_items() {
    let mut state = SimState::new();
    let mut configured = World::with_capacity(1, 31);
    let mut map = fertile_map(3, 3);
    configured.configure_items(&map);
    configured.place_goody(&mut map, 1, 1, 0).unwrap();
    SimBridge::populate_with_map(&configured, &map, 0, &mut state);
    assert!(state.channel_is_installed(Channel::Items as usize));

    let absent = World::with_capacity(1, 31);
    let bridge = SimBridge::populate(&absent, &mut state);
    assert_eq!(bridge.items, ItemProjection::Unavailable);
    let projected = CheckAll::of_state(&state);
    assert!(!item_report(&projected).installed);
    assert_eq!(item_report(&projected).value, 1);
    assert_eq!(item_report(&projected).bytes, 0);
}
