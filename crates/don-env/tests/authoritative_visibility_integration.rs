//! The authoritative backend consumes one complete Sim-owned visibility/combat preflight
//! without admitting ATTACK past its production-proof boundary.

use don_env::authoritative_backend::{
    ApplyRefusal, IntegrationBoundary, QueuePosition, UnitActionRequest, VisibilityCaptureRefusal,
};
use don_env::{AuthoritativeBackend, ScenarioSpec, ScenarioUnit};
use don_sim::balance::{BalanceTable, BYTES as BALANCE_BYTES};
use don_sim::systems::external_entity_visibility_frontier::TYPE_UNIT_FLAG_CLOAK;
use don_sim::systems::map_terrain::COORD_PER_WCELL;
use don_sim::world::UnitTypeStats;
use std::sync::Arc;

fn spec() -> ScenarioSpec {
    ScenarioSpec {
        seed: 0x51b1_2026,
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
                x: COORD_PER_WCELL + 32,
                y: COORD_PER_WCELL + 32,
                los_tiles: 4,
            },
        ],
    }
}

fn install_non_cloaked_type_sources(backend: &mut AuthoritativeBackend) {
    let mut balance = vec![0u8; BALANCE_BYTES];
    for (attacker, defender) in [(50, 51), (51, 50)] {
        let cell = don_sim::balance_path::table_index(attacker, defender).unwrap() * 2;
        balance[cell..cell + 2].copy_from_slice(&100i16.to_le_bytes());
    }
    backend
        .install_attack_execution_sources(
            Arc::new(BalanceTable::from_bytes(&balance).unwrap()),
            vec![
                UnitTypeStats {
                    type_id: 50,
                    attack: 100,
                    armor: 0,
                    hits: 100,
                    recharge: 15,
                    max_range: COORD_PER_WCELL,
                    min_range: 0,
                },
                UnitTypeStats {
                    type_id: 51,
                    attack: 100,
                    armor: 0,
                    hits: 100,
                    recharge: 15,
                    max_range: COORD_PER_WCELL,
                    min_range: 0,
                },
            ],
        )
        .unwrap();
    backend.install_visibility_type_flags(50, 0).unwrap();
    backend.install_visibility_type_flags(51, 0).unwrap();
    backend
        .install_visibility_viewer_mask(0, 0b0000_0001)
        .unwrap();
    backend
        .install_visibility_viewer_mask(1, 0b0000_0010)
        .unwrap();
}

fn attack(actor: don_sim::Handle, target_entity: u16) -> UnitActionRequest {
    UnitActionRequest {
        verb_head: don_env::generated::uv::ATTACK as u16 + 1,
        actor,
        target_x: 0,
        target_y: 0,
        target_entity,
        queue: QueuePosition::Replace,
        order_flags: 0,
    }
}

#[test]
fn capture_refuses_zero_filled_type_defaults_as_authoritative_cloak_facts() {
    let mut backend = AuthoritativeBackend::from_spec(spec()).unwrap();
    assert_eq!(
        backend.capture_external_visibility(),
        Err(VisibilityCaptureRefusal::TypeFlagsUnavailable {
            row: 0,
            type_id: 50,
        })
    );
    backend.install_visibility_type_flags(50, 0).unwrap();
    backend.install_visibility_type_flags(51, 0).unwrap();
    assert_eq!(
        backend.capture_external_visibility(),
        Err(VisibilityCaptureRefusal::ViewerMaskUnavailable { viewer: 0 })
    );
    let observation = backend.observe(0).unwrap();
    assert!(!observation.external_entities_complete);
    assert!(observation.external_entities.is_empty());
}

#[test]
fn current_frame_capture_projects_only_visible_external_stable_identities() {
    let mut backend = AuthoritativeBackend::from_spec(spec()).unwrap();
    install_non_cloaked_type_sources(&mut backend);

    backend.capture_external_visibility().unwrap();
    let hidden = backend.observe(0).unwrap();
    assert!(hidden.external_entities_complete);
    assert!(hidden.external_entities.is_empty());

    // Without installed Step-12 authority, phase 33 correctly leaves the fog planes unchanged.
    // Fog option 3 is an explicit visibility policy fact, not a synthetic `seen` stamp.
    backend.step_frames(34);
    assert!(!backend.observe(0).unwrap().external_entities_complete);
    backend.install_visibility_fog_option(3).unwrap();
    backend.capture_external_visibility().unwrap();

    let visible = backend.observe(0).unwrap();
    assert!(visible.external_entities_complete);
    assert_eq!(visible.external_entities.len(), 1);
    let target = visible.external_entities[0];
    assert_eq!(target.ordinal, 1);
    assert_eq!(target.identity.who, 1);
    assert_eq!(
        target.identity.handle,
        backend.sim().world.handle_at_row(1).unwrap()
    );
    assert_eq!(target.public.type_id, 51);
    assert_eq!((target.public.x, target.public.y), (800, 800));
}

#[test]
fn attack_reaches_identity_visibility_preflight_but_stays_masked_at_commit_host() {
    let mut backend = AuthoritativeBackend::from_spec(spec()).unwrap();
    install_non_cloaked_type_sources(&mut backend);
    backend.install_visibility_fog_option(3).unwrap();
    backend.step_frames(34);
    backend.capture_external_visibility().unwrap();

    let actor = backend.sim().world.handle_at_row(0).unwrap();
    let target = backend.observe(0).unwrap().external_entities[0];
    let request = attack(actor, target.ordinal);
    let before_digest = backend.sim().world.digest();
    let before_orders = backend.sim().world.orders(0).clone();

    assert!(!backend
        .unit_verb_mask(0, request)
        .allows(don_env::generated::uv::ATTACK + 1));
    assert_eq!(
        backend.apply_unit(0, request),
        Err(ApplyRefusal::AttackTargetCommitUnavailable {
            verb_index: don_env::generated::uv::ATTACK,
            target: target.identity,
            boundary: IntegrationBoundary::CombatTargetHost,
        })
    );
    assert_eq!(backend.sim().world.digest(), before_digest);
    assert_eq!(backend.sim().world.orders(0), &before_orders);
}

#[test]
fn prepared_attack_retains_revisions_visibility_hostility_and_exact_queue_identity() {
    let mut backend = AuthoritativeBackend::from_spec(spec()).unwrap();
    install_non_cloaked_type_sources(&mut backend);
    backend.install_visibility_fog_option(3).unwrap();
    backend.step_frames(34);
    backend.capture_external_visibility().unwrap();

    let actor = backend.sim().world.handle_at_row(0).unwrap();
    let target = backend.observe(0).unwrap().external_entities[0];
    let request = attack(actor, target.ordinal);
    let prepared = backend.prepare_attack_target(0, request).unwrap();

    assert_eq!(prepared.actor(), actor);
    assert_eq!(prepared.target(), target.identity);
    assert_eq!(prepared.target_ordinal(), target.ordinal);
    assert_eq!(prepared.visibility_frame(), backend.sim().world.frame);
    assert_eq!(
        prepared.visibility_revision(),
        backend.external_visibility_revision()
    );
    assert_eq!(
        prepared.relation(),
        don_sim::systems::victory_score::Diplo::War
    );
    assert_eq!(prepared.target_public(), target.public);
    let execution = prepared.execution_proof();
    assert_eq!(
        (execution.actor_type_id(), execution.target_type_id()),
        (50, 51)
    );
    assert_eq!(execution.balance_pct(), 100);
    assert!(execution.predicted_damage() > 0);
    assert_eq!(execution.recharge_frames(), 15);
    assert!(!execution.uses_projectile());
    let retained = prepared.retained_order();
    assert_eq!(retained.kind, don_sim::order::OrderIndex::Attack);
    assert_eq!(
        retained.exact_target_identity(),
        Some(don_sim::order::OrderTargetIdentity {
            handle: target.identity.handle,
            who: target.identity.who as i8,
            o: target.identity.object_o,
            uid: target.identity.uid,
        })
    );

    let before_digest = backend.sim().world.digest();
    let before_orders = backend.sim().world.orders(0).clone();
    assert_eq!(
        backend.apply_prepared_attack_target(prepared),
        Err(ApplyRefusal::AttackTargetCommitUnavailable {
            verb_index: don_env::generated::uv::ATTACK,
            target: target.identity,
            boundary: IntegrationBoundary::CombatTargetHost,
        })
    );
    assert_eq!(backend.sim().world.digest(), before_digest);
    assert_eq!(backend.sim().world.orders(0), &before_orders);

    // Publishing a new image invalidates the opaque visibility revision even when the
    // source frame and canonical rows are byte-identical.
    backend.capture_external_visibility().unwrap();
    assert!(matches!(
        backend.apply_prepared_attack_target(prepared),
        Err(ApplyRefusal::TargetVisibility {
            fault: don_sim::systems::external_entity_visibility_frontier::VisibilityProjectionFault::StaleBinding { .. },
            ..
        })
    ));

    let current = backend.prepare_attack_target(0, request).unwrap();
    let expected_episode = current.episode_revision();
    backend.reset().unwrap();
    assert!(matches!(
        backend.apply_prepared_attack_target(current),
        Err(ApplyRefusal::StaleEpisodeRevision { expected, .. }) if expected == expected_episode
    ));
}

#[test]
fn tick_and_reset_invalidate_ordinals_while_type_sources_survive_reset() {
    let mut backend = AuthoritativeBackend::from_spec(spec()).unwrap();
    install_non_cloaked_type_sources(&mut backend);
    backend.capture_external_visibility().unwrap();
    let installed_revision = backend.external_visibility_revision();

    backend.step_frames(1);
    assert!(backend.external_visibility_revision() > installed_revision);
    assert!(!backend.observe(0).unwrap().external_entities_complete);

    let before_reset_revision = backend.external_visibility_revision();
    backend.reset().unwrap();
    assert!(backend.external_visibility_revision() > before_reset_revision);
    assert!(!backend.observe(0).unwrap().external_entities_complete);
    backend.capture_external_visibility().unwrap();
    assert!(backend.observe(0).unwrap().external_entities_complete);
}

#[test]
fn cloak_rows_refuse_until_the_step12_detector_producer_is_authoritative() {
    let mut backend = AuthoritativeBackend::from_spec(spec()).unwrap();
    backend.install_visibility_type_flags(50, 0).unwrap();
    backend
        .install_visibility_type_flags(51, TYPE_UNIT_FLAG_CLOAK)
        .unwrap();

    assert_eq!(
        backend.capture_external_visibility(),
        Err(VisibilityCaptureRefusal::DetectionPlaneCompletenessUnavailable { row: 1 })
    );
    assert!(!backend.observe(0).unwrap().external_entities_complete);
}
