//! Public side-by-side fidelity backend contract. No compact `EnvWorld` participates.

use don_env::authoritative_backend::{
    ApplyReceipt, ApplyRefusal, QueuePosition, UnitActionRequest, PLAYER_INTEGRATION,
    UNIT_INTEGRATION,
};
use don_env::generated;
use don_env::{AuthoritativeBackend, ScenarioSpec, ScenarioUnit};
use don_sim::order::{OrderIndex, ORDER_FLEEING};
use don_sim::systems::collision::DOMAIN_LAND;
use don_sim::systems::map_terrain::COORD_PER_WCELL;
use don_sim::systems::movement_live::{LiveCollisionGuy, LiveCollisionSource};

fn spec() -> ScenarioSpec {
    ScenarioSpec {
        seed: 0xade7_2026,
        map_wcells: 4,
        active_players: vec![0, 1],
        units: vec![
            ScenarioUnit {
                who: 0,
                type_id: 50,
                x: COORD_PER_WCELL,
                y: COORD_PER_WCELL,
                los_tiles: 4,
            },
            ScenarioUnit {
                who: 1,
                type_id: 51,
                x: 2 * COORD_PER_WCELL,
                y: 2 * COORD_PER_WCELL,
                los_tiles: 4,
            },
        ],
    }
}

fn source(x: i32, y: i32, moving: bool, action: OrderIndex) -> LiveCollisionSource {
    LiveCollisionSource {
        domain: DOMAIN_LAND,
        block_radius: 1,
        big_radius: 48,
        push_size: 0,
        push_circles: 0,
        unit_flags: 0,
        unit_flags2: 0,
        attack_value: 0,
        spell_id: -1,
        unpacking: false,
        captain: false,
        moving,
        searching: false,
        action: action as i32,
        invalid_tiles: Vec::new(),
        guys: vec![LiveCollisionGuy {
            x,
            y,
            angle: 0,
            block_radius: 1,
        }],
    }
}

#[test]
fn frozen_integration_map_covers_every_generated_policy_verb() {
    assert_eq!(UNIT_INTEGRATION.len(), generated::UNIT_VERBS.len());
    for (route, generated) in UNIT_INTEGRATION.iter().zip(&generated::UNIT_VERBS) {
        assert_eq!(
            (route.name, route.opcode),
            (generated.name, generated.opcode)
        );
    }
    assert_eq!(PLAYER_INTEGRATION.len(), generated::PLAYER_VERBS.len());
    for (route, generated) in PLAYER_INTEGRATION.iter().zip(&generated::PLAYER_VERBS) {
        assert_eq!(
            (route.name, route.opcode),
            (generated.name, generated.opcode)
        );
    }

    let mut backend = AuthoritativeBackend::from_spec(spec()).unwrap();
    let before = backend.sim().world.digest();
    for verb in 1..=generated::PLAYER_VERBS.len() as u16 {
        assert!(matches!(
            backend.apply_player(verb),
            Err(ApplyRefusal::Unhosted { .. })
        ));
    }
    assert_eq!(backend.sim().world.digest(), before);
}

#[test]
fn unhosted_and_unready_actions_refuse_without_mutating_the_core() {
    let mut backend = AuthoritativeBackend::from_spec(spec()).unwrap();
    let actor = backend.sim().world.handle_at_row(0).unwrap();
    let before = backend.sim().world.digest();
    let attack = UnitActionRequest {
        verb_head: (generated::uv::ATTACK + 1) as u16,
        actor,
        target_x: 0,
        target_y: 0,
        queue: QueuePosition::Replace,
        order_flags: 0,
    };
    assert!(matches!(
        backend.apply_unit(0, attack),
        Err(ApplyRefusal::Unhosted { .. })
    ));
    assert_eq!(backend.sim().world.digest(), before);

    let movement = UnitActionRequest {
        verb_head: (generated::uv::MOVE_TO + 1) as u16,
        actor,
        target_x: COORD_PER_WCELL + 240,
        target_y: COORD_PER_WCELL,
        queue: QueuePosition::Replace,
        order_flags: 0,
    };
    assert!(matches!(
        backend.apply_unit(0, movement),
        Err(ApplyRefusal::MovementHost(_))
    ));
    assert_eq!(backend.sim().world.digest(), before);
}

#[test]
fn admitted_move_installs_into_sim_and_executes_in_the_retail_tick() {
    let mut backend = AuthoritativeBackend::from_spec(spec()).unwrap();
    let actor = backend.sim().world.handle_at_row(0).unwrap();
    let blocker = backend.sim().world.handle_at_row(1).unwrap();
    let start = (
        backend.sim().world.pos_x()[0],
        backend.sim().world.pos_y()[0],
    );
    let blocker_pos = (
        backend.sim().world.pos_x()[1],
        backend.sim().world.pos_y()[1],
    );
    backend
        .install_movement_source(actor, source(start.0, start.1, true, OrderIndex::MoveTo))
        .unwrap();
    backend
        .install_movement_source(
            blocker,
            source(blocker_pos.0, blocker_pos.1, false, OrderIndex::None),
        )
        .unwrap();

    let request = UnitActionRequest {
        verb_head: (generated::uv::MOVE_TO + 1) as u16,
        actor,
        target_x: start.0 + 240,
        target_y: start.1,
        queue: QueuePosition::Replace,
        order_flags: 0,
    };
    assert!(matches!(
        backend.apply_unit(0, request),
        Ok(ApplyReceipt::OrderInstalled {
            kind: OrderIndex::MoveTo,
            queue_len: 1,
            ..
        })
    ));
    assert_eq!(
        backend.sim().world.orders(0).order_type(),
        OrderIndex::MoveTo
    );

    let tick = backend.step_frames(1);
    assert_eq!(tick.executed[14], 1);
    assert_ne!(
        (
            backend.sim().world.pos_x()[0],
            backend.sim().world.pos_y()[0]
        ),
        start
    );
}

#[test]
fn unsupported_queue_and_unprimed_flee_source_refuse_atomically() {
    let mut backend = AuthoritativeBackend::from_spec(spec()).unwrap();
    let actor = backend.sim().world.handle_at_row(0).unwrap();
    let other = backend.sim().world.handle_at_row(1).unwrap();
    let start = (
        backend.sim().world.pos_x()[0],
        backend.sim().world.pos_y()[0],
    );
    let other_pos = (
        backend.sim().world.pos_x()[1],
        backend.sim().world.pos_y()[1],
    );
    backend
        .install_movement_source(actor, source(start.0, start.1, true, OrderIndex::MoveTo))
        .unwrap();
    backend
        .install_movement_source(
            other,
            source(other_pos.0, other_pos.1, false, OrderIndex::None),
        )
        .unwrap();
    let before = backend.sim().world.digest();

    let mut request = UnitActionRequest {
        verb_head: (generated::uv::MOVE_TO + 1) as u16,
        actor,
        target_x: start.0 + 240,
        target_y: start.1,
        queue: QueuePosition::Last,
        order_flags: 0,
    };
    assert_eq!(
        backend.apply_unit(0, request),
        Err(ApplyRefusal::UnsupportedQueue(QueuePosition::Last))
    );
    assert_eq!(backend.sim().world.digest(), before);

    request.queue = QueuePosition::Replace;
    request.order_flags = ORDER_FLEEING;
    assert!(matches!(
        backend.apply_unit(0, request),
        Err(ApplyRefusal::MovementSourceState { .. })
    ));
    assert_eq!(backend.sim().world.digest(), before);
}

#[test]
fn observation_and_reward_read_sim_state_without_enemy_leakage_or_mirrors() {
    let backend = AuthoritativeBackend::from_spec(spec()).unwrap();
    let observation = backend.observe(0).unwrap();
    assert_eq!(observation.frame, backend.sim().world.frame);
    assert_eq!(observation.score, backend.sim().vic_leaders.slots[0].score);
    assert_eq!(observation.own_entities.len(), 1);
    assert_eq!(observation.own_entities[0].who, 0);
    assert!(!observation.external_entities_complete);

    let before = backend.reward_snapshot(0).unwrap();
    let reward = backend.reward_since(0, before).unwrap();
    assert_eq!(reward.score_delta, 0);
    assert_eq!(reward.economy_delta, 0);
    assert!(!reward.win);
    assert!(!reward.loss);
    assert!(reward.alive);
}
