//! The public selector keeps compact and authoritative ownership explicit and deterministic.

use don_env::authoritative_backend::{
    ApplyReceipt, ApplyRefusal, IntegrationBoundary, QueuePosition, UnitActionRequest,
    UNIT_INTEGRATION,
};
use don_env::{
    BackendCreateError, BackendKind, EnvConfig, EnvironmentBackend, EpisodeError, ScenarioSpec,
    ScenarioUnit,
};
use don_sim::systems::map_terrain::COORD_PER_WCELL;

fn scenario() -> ScenarioSpec {
    ScenarioSpec {
        seed: 0xbac4_2026,
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

#[test]
fn compact_constructor_preserves_the_existing_vecenv_surface() {
    let selected = EnvironmentBackend::compact(1, EnvConfig::default(), None, None, 1).unwrap();

    assert_eq!(selected.kind(), BackendKind::Compact);
    assert!(selected.as_compact().is_some());
    assert!(selected.as_authoritative().is_none());
}

#[test]
fn authoritative_constructor_preserves_typed_scenario_refusals() {
    let mut invalid = scenario();
    invalid.map_wcells = 0;

    assert!(matches!(
        EnvironmentBackend::authoritative(invalid),
        Err(BackendCreateError::Authoritative(EpisodeError::EmptyMap))
    ));
}

#[test]
fn authoritative_constructor_exposes_only_the_typed_sim_surface() {
    let mut selected = EnvironmentBackend::authoritative(scenario()).unwrap();

    assert_eq!(selected.kind(), BackendKind::Authoritative);
    assert!(selected.as_compact().is_none());
    let backend = selected.as_authoritative_mut().unwrap();
    let actor = backend.sim().world.handle_at_row(0).unwrap();
    let initial_digest = backend.sim().world.digest();

    assert!(matches!(
        backend.apply_unit(
            0,
            UnitActionRequest {
                verb_head: 0,
                actor,
                target_x: 0,
                target_y: 0,
                queue: QueuePosition::Replace,
                order_flags: 0,
            }
        ),
        Ok(ApplyReceipt::Noop { frame: 0 })
    ));
    assert_eq!(backend.sim().world.digest(), initial_digest);

    let attack = UNIT_INTEGRATION
        .iter()
        .position(|route| route.name == "ATTACK")
        .unwrap();
    assert!(matches!(
        backend.apply_unit(
            0,
            UnitActionRequest {
                verb_head: attack as u16 + 1,
                actor,
                target_x: 2 * COORD_PER_WCELL,
                target_y: 2 * COORD_PER_WCELL,
                queue: QueuePosition::Replace,
                order_flags: 0,
            }
        ),
        Err(ApplyRefusal::Unhosted {
            boundary: IntegrationBoundary::CombatTargetHost,
            ..
        })
    ));
    assert_eq!(backend.sim().world.digest(), initial_digest);

    let observation = backend.observe(0).unwrap();
    let reward = backend.reward_snapshot(0).unwrap();
    assert_eq!(observation.own_entities.len(), 1);
    assert!(!observation.external_entities_complete);
    assert_eq!(reward.score, observation.score);
}

#[test]
fn authoritative_reset_reconstructs_the_declared_initial_image() {
    let mut selected = EnvironmentBackend::authoritative(scenario()).unwrap();
    let backend = selected.as_authoritative_mut().unwrap();
    let initial_digest = backend.sim().world.digest();
    let initial_observation = backend.observe(0).unwrap();
    let initial_reward = backend.reward_snapshot(0).unwrap();

    let receipt = backend.step_frames(9);
    assert_eq!((receipt.start_frame, receipt.end_frame), (0, 9));
    backend.reset().unwrap();

    assert_eq!(backend.sim().world.frame, 0);
    assert_eq!(backend.sim().world.digest(), initial_digest);
    assert_eq!(backend.observe(0).unwrap(), initial_observation);
    assert_eq!(backend.reward_snapshot(0).unwrap(), initial_reward);
}
