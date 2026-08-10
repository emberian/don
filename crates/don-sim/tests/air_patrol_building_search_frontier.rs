#[path = "../src/systems/air_patrol_building_search_frontier.rs"]
mod air_patrol_building_search_frontier;

use air_patrol_building_search_frontier::*;

fn key(who: i16, o: i16) -> ObjectKey {
    ObjectKey { who, o }
}

fn record(who: i16, o: i16, down: ObjectKey) -> BuildingSearchRecord {
    BuildingSearchRecord {
        key: key(who, o),
        down,
        valid_build: true,
        active: true,
        corner_x: 0,
        corner_y: 0,
        x_size: 16,
        y_size: 16,
        uid: o as u16 + 100,
        ever_seen: 1,
    }
}

fn frame() -> BuildingSearchFrame {
    let mut diplomacy = [[2; PLAYER_SLOTS]; PLAYER_SLOTS];
    diplomacy[1][0] = 0;
    BuildingSearchFrame {
        tile_xs: 12,
        tile_ys: 12,
        tile_masks: vec![BUILDING_BLOCKER_MASK; 12 * 12],
        world_cell_xs: 3,
        world_cell_ys: 3,
        world_cell_heads: vec![ObjectKey::NONE; 3 * 3],
        buildings: Vec::new(),
        diplomacy,
    }
}

fn due() -> AirPatrolBuildingGateInput {
    AirPatrolBuildingGateInput {
        air_physics_succeeded: true,
        actor_is_animal: false,
        prior_branch_returned: false,
        actor_o: 7,
        frame: 25,
    }
}

#[test]
fn shipped_addresses_offsets_and_search_order_are_frozen() {
    assert_eq!(UNIT_DO_AIR_PATROL_VA, 0x005e_a620);
    assert_eq!(AIR_PATROL_BUILDING_BRANCH_VA, 0x005e_a9aa);
    assert_eq!(FIND_BUILDING_AT_CALL_VA, 0x005e_aa84);
    assert_eq!(OBJECTS_FIND_BUILDING_AT_VA, 0x0065_ab40);
    assert_eq!(SEARCH_VALID_SEARCH_VA, 0x0067_daa0);
    assert_eq!(LEADER_IS_ENEMY_VA, 0x006e_baa0);
    assert_eq!(WALL_TILE_CORNER_VA, 0x0064_3440);
    assert_eq!((SEARCH_ENEMY, FILTER_ALL), (3, 0));
    assert_eq!((WORLD_XS_OFFSET, WORLD_YS_OFFSET), (0, 4));
    assert_eq!((WORLD_TILE_XS_OFFSET, WORLD_TILE_YS_OFFSET), (0x18, 0x1c));
    assert_eq!((WORLD_WDATA_OFFSET, WORLD_TDATA_OFFSET), (0x134, 0x138));
    assert_eq!((WDATA_DOWN_OFFSET, WDATA_DOWN_WHO_OFFSET), (8, 10));
    assert_eq!((OBJECT_DOWN_OFFSET, OBJECT_DOWN_WHO_OFFSET), (0x2c, 0x2e));
    assert_eq!(OBJECT_UID_OFFSET, 0x30);
    assert_eq!(WALL_EVER_SEEN_OFFSET, 0x62);
    assert_eq!(
        (OBJECT_TYPE_X_SIZE_OFFSET, OBJECT_TYPE_Y_SIZE_OFFSET),
        (0x234, 0x238)
    );
    assert_eq!(
        WORLD_CELL_SEARCH_OFFSETS,
        [
            (0, 0),
            (-1, -1),
            (0, -1),
            (1, -1),
            (1, 0),
            (1, 1),
            (0, 1),
            (-1, 1),
            (-1, 0),
        ]
    );
}

#[test]
fn gate_preserves_retail_early_returns_and_signed_modulo() {
    let mut input = due();
    assert_eq!(air_patrol_building_gate(input), AirPatrolBuildingGate::Due);
    input.air_physics_succeeded = false;
    assert_eq!(
        air_patrol_building_gate(input),
        AirPatrolBuildingGate::AirPhysicsStopped
    );
    input.air_physics_succeeded = true;
    input.actor_is_animal = true;
    assert_eq!(
        air_patrol_building_gate(input),
        AirPatrolBuildingGate::AnimalOverride
    );
    input.actor_is_animal = false;
    input.prior_branch_returned = true;
    assert_eq!(
        air_patrol_building_gate(input),
        AirPatrolBuildingGate::PriorBranchReturned
    );
    input.prior_branch_returned = false;
    input.frame = 24;
    assert_eq!(
        air_patrol_building_gate(input),
        AirPatrolBuildingGate::PhaseNotDue
    );

    input.actor_o = -7;
    input.frame = -25;
    assert_eq!(air_patrol_building_gate(input), AirPatrolBuildingGate::Due);
    input.frame = -24;
    assert_eq!(
        air_patrol_building_gate(input),
        AirPatrolBuildingGate::PhaseNotDue
    );
}

#[test]
fn coord_conversion_and_fighter_bomber_home_transform_are_exact() {
    assert_eq!(coord_to_tile(0), 0);
    assert_eq!(coord_to_tile(191), 0);
    assert_eq!(coord_to_tile(192), 1);
    assert_eq!(coord_to_tile(-1), -1);
    assert_eq!(coord_to_tile(-192), -1);
    assert_eq!(coord_to_tile(-193), -2);

    assert_eq!(
        building_search_coord((100, 200), false, Some((50, 60)), 3, 2),
        (100, 200)
    );
    assert_eq!(
        building_search_coord((100, 200), true, None, 3, 2),
        (100, 200)
    );
    assert_eq!(
        building_search_coord((2200, -100), true, Some((200, 20)), 3, 2),
        (2303, 0)
    );
}

#[test]
fn search_walks_center_then_clockwise_neighbors_and_returns_first_hit() {
    let mut f = frame();
    let center_reject = record(0, 10, ObjectKey::NONE);
    let northwest_hit = record(1, 11, ObjectKey::NONE);
    let north_later = record(1, 12, ObjectKey::NONE);
    f.world_cell_heads[4] = center_reject.key; // centre (1,1)
    f.world_cell_heads[0] = northwest_hit.key; // north-west (0,0)
    f.world_cell_heads[1] = north_later.key; // north (1,0)
    f.buildings = vec![center_reject, northwest_hit, north_later];
    let mut owner = AirPatrolBuildingSearchOwner::default();
    owner.install_frame(f).unwrap();

    let run = owner
        .probe(due(), 0, (5 * COORD_PER_TILE, 5 * COORD_PER_TILE))
        .unwrap();
    assert!(matches!(
        run.decision,
        AirPatrolBuildingDecision::Target(BuildingSearchHit {
            key: ObjectKey { who: 1, o: 11 },
            ..
        })
    ));
    assert_eq!(run.visited_cells, [(1, 1), (0, 0)]);
    assert_eq!(run.visited_candidates, [key(0, 10), key(1, 11)]);
}

#[test]
fn chain_order_applies_enemy_valid_active_and_half_open_footprint_gates() {
    let mut f = frame();
    let mut friendly = record(0, 1, key(1, 2));
    let mut invalid = record(1, 2, key(1, 3));
    invalid.valid_build = false;
    let mut inactive = record(1, 3, key(1, 4));
    inactive.active = false;
    let mut outside = record(1, 4, key(1, 5));
    outside.corner_x = 6;
    outside.corner_y = 5;
    outside.x_size = 1;
    outside.y_size = 1;
    let mut hit = record(1, 5, ObjectKey::NONE);
    hit.corner_x = 4;
    hit.corner_y = 5;
    hit.x_size = 1;
    hit.y_size = 1;
    friendly.ever_seen = 0xff;
    f.world_cell_heads[4] = friendly.key;
    f.buildings = vec![friendly, invalid, inactive, outside, hit];
    let mut owner = AirPatrolBuildingSearchOwner::default();
    owner.install_frame(f).unwrap();

    let run = owner
        .probe(due(), 0, (4 * COORD_PER_TILE, 5 * COORD_PER_TILE))
        .unwrap();
    assert!(matches!(
        run.decision,
        AirPatrolBuildingDecision::Target(BuildingSearchHit {
            key: ObjectKey { o: 5, .. },
            ..
        })
    ));
    assert_eq!(
        run.visited_candidates,
        [key(0, 1), key(1, 2), key(1, 3), key(1, 4), key(1, 5)]
    );
}

#[test]
fn search_enemy_is_directional_or_and_never_matches_self() {
    let mut d = [[2; PLAYER_SLOTS]; PLAYER_SLOTS];
    assert!(!leader_is_enemy(&d, 1, 0));
    d[1][0] = 0;
    assert!(leader_is_enemy(&d, 1, 0));
    d[1][0] = 2;
    d[0][1] = 0;
    assert!(leader_is_enemy(&d, 1, 0));
    d[0][0] = 0;
    assert!(!leader_is_enemy(&d, 0, 0));
}

#[test]
fn tile_blocker_guard_precedes_every_world_cell_read() {
    let mut f = frame();
    f.tile_masks[5 * 12 + 5] = 2;
    f.world_cell_heads[4] = key(9, 99);
    let mut owner = AirPatrolBuildingSearchOwner::default();
    owner.install_frame(f).unwrap();
    let run = owner
        .probe(due(), 0, (5 * COORD_PER_TILE, 5 * COORD_PER_TILE))
        .unwrap();
    assert_eq!(run.decision, AirPatrolBuildingDecision::Miss);
    assert!(run.visited_cells.is_empty());
    assert!(run.visited_candidates.is_empty());
}

#[test]
fn ever_seen_is_a_post_search_build_byte_gate_and_does_not_resume_the_walk() {
    let mut f = frame();
    let mut hidden_first = record(1, 7, key(1, 8));
    hidden_first.ever_seen = 0b0000_0010;
    let mut visible_second = record(1, 8, ObjectKey::NONE);
    visible_second.ever_seen = 0b0000_0001;
    f.world_cell_heads[4] = hidden_first.key;
    f.buildings = vec![hidden_first, visible_second];
    let mut owner = AirPatrolBuildingSearchOwner::default();
    owner.install_frame(f).unwrap();

    let run = owner
        .probe(due(), 0, (5 * COORD_PER_TILE, 5 * COORD_PER_TILE))
        .unwrap();
    assert!(matches!(
        run.decision,
        AirPatrolBuildingDecision::FirstHitNotSeen(BuildingSearchHit {
            key: ObjectKey { o: 7, .. },
            ..
        })
    ));
    assert_eq!(run.visited_candidates, [key(1, 7)]);
    assert!(building_was_seen_by(0x80, 7));
    assert!(!building_was_seen_by(0xff, 8));
}

#[test]
fn negative_chain_owner_bypasses_valid_search_but_owner_eight_is_rejected() {
    let mut f = frame();
    let rejected = record(8, 1, key(-1, 2));
    let negative = record(-1, 2, ObjectKey::NONE);
    f.world_cell_heads[4] = rejected.key;
    f.buildings = vec![rejected, negative];
    let mut owner = AirPatrolBuildingSearchOwner::default();
    owner.install_frame(f).unwrap();
    let run = owner
        .probe(due(), 0, (5 * COORD_PER_TILE, 5 * COORD_PER_TILE))
        .unwrap();
    assert!(matches!(
        run.decision,
        AirPatrolBuildingDecision::Target(BuildingSearchHit {
            key: ObjectKey { who: -1, o: 2 },
            ..
        })
    ));
}

#[test]
fn missing_links_and_cycles_fail_closed_and_failed_installs_are_atomic() {
    let mut owner = AirPatrolBuildingSearchOwner::default();
    owner.install_frame(frame()).unwrap();
    let revision = owner.revision();
    let mut bad = frame();
    bad.world_cell_heads.pop();
    assert!(matches!(
        owner.install_frame(bad),
        Err(BuildingSearchInstallFault::WorldCellPlaneLength { .. })
    ));
    assert_eq!(owner.revision(), revision);

    let mut missing = frame();
    missing.world_cell_heads[4] = key(1, 77);
    owner.install_frame(missing).unwrap();
    assert_eq!(
        owner.probe(due(), 0, (5 * COORD_PER_TILE, 5 * COORD_PER_TILE)),
        Err(BuildingSearchFault::MissingObject(key(1, 77)))
    );

    let mut cycle = frame();
    let mut looping = record(1, 1, key(1, 1));
    looping.valid_build = false;
    cycle.world_cell_heads[4] = looping.key;
    cycle.buildings = vec![looping];
    owner.install_frame(cycle).unwrap();
    assert!(matches!(
        owner.probe(due(), 0, (5 * COORD_PER_TILE, 5 * COORD_PER_TILE)),
        Err(BuildingSearchFault::ChainCycle { .. })
    ));
}
