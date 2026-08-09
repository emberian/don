//! Contract for deterministic authoritative movement setup and conditional verb masks.

use don_env::authoritative_backend::{
    ActionSourceStateSetterRoute, AuthoritativeBackend, AuthoritativeScenarioSpec, QueuePosition,
    ScenarioMovementSource, ScenarioSetupError, UnitActionRequest, ACTION_SOURCE_STATE_SETTER,
    MOVE_TO_VERB_INDEX, UNIT_VERB_HEAD_COUNT,
};
use don_env::{ScenarioSpec, ScenarioUnit};
use don_sim::order::OrderIndex;
use don_sim::systems::collision::DOMAIN_LAND;
use don_sim::systems::map_terrain::COORD_PER_WCELL;
use don_sim::systems::movement_live::{LiveCollisionFault, LiveCollisionGuy, LiveCollisionSource};

fn episode_spec() -> ScenarioSpec {
    ScenarioSpec {
        seed: 0x50ce_2026,
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

fn setup(actor_action: OrderIndex) -> AuthoritativeScenarioSpec {
    let episode = episode_spec();
    AuthoritativeScenarioSpec {
        movement_sources: vec![
            ScenarioMovementSource {
                unit: 0,
                source: source(episode.units[0].x, episode.units[0].y, false, actor_action),
            },
            ScenarioMovementSource {
                unit: 1,
                source: source(
                    episode.units[1].x,
                    episode.units[1].y,
                    false,
                    OrderIndex::None,
                ),
            },
        ],
        episode,
    }
}

fn move_request(backend: &AuthoritativeBackend) -> UnitActionRequest {
    let actor = backend.sim().world.handle_at_row(0).unwrap();
    UnitActionRequest {
        verb_head: MOVE_TO_VERB_INDEX as u16 + 1,
        actor,
        target_x: COORD_PER_WCELL + 240,
        target_y: COORD_PER_WCELL,
        target_entity: 0,
        queue: QueuePosition::Replace,
        order_flags: 0,
    }
}

fn runtime_sources(backend: &AuthoritativeBackend) -> Vec<Option<LiveCollisionSource>> {
    (0..backend.sim().world.live_count() as usize)
        .map(|row| backend.sim().movement_collision.source(row).cloned())
        .collect()
}

#[test]
fn captured_sources_make_move_ready_at_construction_and_survive_reset_exactly() {
    let declared = setup(OrderIndex::None);
    let mut backend = AuthoritativeBackend::from_authoritative_scenario(declared.clone()).unwrap();
    let initial_digest = backend.sim().world.digest();
    let initial_wdata = backend.sim().map.world.wdata.clone();
    let initial_handles = (0..2)
        .map(|row| backend.sim().world.handle_at_row(row).unwrap())
        .collect::<Vec<_>>();
    let initial_sources = runtime_sources(&backend);
    let request = move_request(&backend);

    assert_eq!(
        backend.scenario_movement_sources(),
        declared.movement_sources
    );
    assert!(backend
        .unit_verb_mask(0, request)
        .allows(MOVE_TO_VERB_INDEX + 1));
    let initial_state = backend.sim().movement_source_state(request.actor).unwrap();
    assert_eq!(initial_state.revision, 0);
    assert!(!initial_state.moving);
    assert_eq!(initial_state.action, OrderIndex::None as i32);
    backend.apply_unit(0, request).unwrap();
    let applied_state = backend.sim().movement_source_state(request.actor).unwrap();
    assert_eq!(applied_state.revision, 1);
    assert!(applied_state.moving);
    assert_eq!(applied_state.action, OrderIndex::MoveTo as i32);
    backend.step_frames(1);
    assert_ne!(backend.sim().world.digest(), initial_digest);

    backend.reset().unwrap();
    let reset_handles = (0..2)
        .map(|row| backend.sim().world.handle_at_row(row).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(reset_handles, initial_handles);
    assert_eq!(backend.sim().world.digest(), initial_digest);
    assert_eq!(backend.sim().map.world.wdata, initial_wdata);
    assert_eq!(runtime_sources(&backend), initial_sources);
    assert_eq!(
        backend
            .sim()
            .movement_source_state(reset_handles[0])
            .unwrap(),
        initial_state
    );
    assert_eq!(
        backend.scenario_movement_sources(),
        declared.movement_sources
    );
    assert!(backend
        .unit_verb_mask(0, move_request(&backend))
        .allows(MOVE_TO_VERB_INDEX + 1));
}

#[test]
fn source_ordinals_duplicates_and_host_faults_are_typed() {
    let mut out_of_range = setup(OrderIndex::MoveTo);
    out_of_range.movement_sources[1].unit = 2;
    assert!(matches!(
        AuthoritativeBackend::from_authoritative_scenario(out_of_range),
        Err(ScenarioSetupError::MovementSourceUnitOutOfRange {
            source: 1,
            unit: 2,
            units: 2,
        })
    ));

    let mut duplicate = setup(OrderIndex::MoveTo);
    duplicate.movement_sources[1].unit = 0;
    assert!(matches!(
        AuthoritativeBackend::from_authoritative_scenario(duplicate),
        Err(ScenarioSetupError::DuplicateMovementSource { source: 1, unit: 0 })
    ));

    let mut invalid = setup(OrderIndex::MoveTo);
    invalid.movement_sources[0].source.block_radius = 11;
    assert!(matches!(
        AuthoritativeBackend::from_authoritative_scenario(invalid),
        Err(ScenarioSetupError::MovementSource {
            source: 0,
            unit: 0,
            fault: LiveCollisionFault::InvalidBlockRadius(11),
        })
    ));
}

#[test]
fn conditional_verb_mask_is_exact_and_observational() {
    let backend =
        AuthoritativeBackend::from_authoritative_scenario(setup(OrderIndex::None)).unwrap();
    let request = move_request(&backend);
    let before_digest = backend.sim().world.digest();
    let before_wdata = backend.sim().map.world.wdata.clone();
    let before_sources = runtime_sources(&backend);
    let before_order_state = backend.sim().movement_collision.order_state.clone();
    let before_source_state = backend.sim().movement_source_state(request.actor).unwrap();

    let mask = backend.unit_verb_mask(0, request);
    assert_eq!(mask.allowed.len(), UNIT_VERB_HEAD_COUNT);
    for (verb_head, allowed) in mask.allowed.iter().copied().enumerate() {
        assert_eq!(
            allowed,
            verb_head == 0 || verb_head == MOVE_TO_VERB_INDEX + 1,
            "unexpected mask value for verb head {verb_head}"
        );
    }

    let mut bad_queue = request;
    bad_queue.queue = QueuePosition::Last;
    assert!(!backend
        .unit_verb_mask(0, bad_queue)
        .allows(MOVE_TO_VERB_INDEX + 1));

    let mut bad_destination = request;
    bad_destination.target_x = -1;
    assert!(!backend
        .unit_verb_mask(0, bad_destination)
        .allows(MOVE_TO_VERB_INDEX + 1));

    assert!(!backend
        .unit_verb_mask(1, request)
        .allows(MOVE_TO_VERB_INDEX + 1));
    assert_eq!(backend.sim().world.digest(), before_digest);
    assert_eq!(backend.sim().map.world.wdata, before_wdata);
    assert_eq!(runtime_sources(&backend), before_sources);
    assert_eq!(
        backend.sim().movement_source_state(request.actor).unwrap(),
        before_source_state
    );
    assert_eq!(
        backend.sim().movement_collision.order_state,
        before_order_state
    );
}

#[test]
fn action_source_state_transition_is_sim_owned_and_same_value_actions_advance_revision() {
    assert_eq!(
        ACTION_SOURCE_STATE_SETTER.route,
        ActionSourceStateSetterRoute::SimCompareExchange
    );

    let mut backend =
        AuthoritativeBackend::from_authoritative_scenario(setup(OrderIndex::None)).unwrap();
    let request = move_request(&backend);
    assert!(backend
        .unit_verb_mask(0, move_request(&backend))
        .allows(MOVE_TO_VERB_INDEX + 1));
    backend.apply_unit(0, request).unwrap();
    let first = backend.sim().movement_source_state(request.actor).unwrap();
    assert_eq!(first.revision, 1);
    assert!(first.moving);
    assert_eq!(first.action, OrderIndex::MoveTo as i32);
    backend.apply_unit(0, request).unwrap();
    let second = backend.sim().movement_source_state(request.actor).unwrap();
    assert_eq!(second.revision, 2);
    assert_eq!((second.moving, second.action), (first.moving, first.action));

    let mut incomplete = setup(OrderIndex::MoveTo);
    incomplete.movement_sources.pop();
    let backend = AuthoritativeBackend::from_authoritative_scenario(incomplete).unwrap();
    assert!(!backend
        .unit_verb_mask(0, move_request(&backend))
        .allows(MOVE_TO_VERB_INDEX + 1));
}

#[test]
fn reset_invalidates_prepared_actions_even_when_handles_and_source_revision_repeat() {
    let mut backend =
        AuthoritativeBackend::from_authoritative_scenario(setup(OrderIndex::None)).unwrap();
    let request = move_request(&backend);
    let prepared = backend.prepare_unit(0, request).unwrap().unwrap();
    assert_eq!(prepared.episode_revision(), 0);
    assert_eq!(prepared.source_revision(), 0);

    backend.reset().unwrap();
    let reset_request = move_request(&backend);
    let before_digest = backend.sim().world.digest();
    let before_state = backend
        .sim()
        .movement_source_state(reset_request.actor)
        .unwrap();
    let before_source = runtime_sources(&backend);
    assert_eq!(before_state.revision, 0);
    assert_eq!(
        backend.apply_prepared_unit(prepared),
        Err(don_env::ApplyRefusal::StaleEpisodeRevision {
            expected: 0,
            observed: 1,
        })
    );
    assert_eq!(backend.sim().world.digest(), before_digest);
    assert_eq!(
        backend
            .sim()
            .movement_source_state(reset_request.actor)
            .unwrap(),
        before_state
    );
    assert_eq!(runtime_sources(&backend), before_source);
    assert!(backend
        .unit_verb_mask(0, reset_request)
        .allows(MOVE_TO_VERB_INDEX + 1));
}
