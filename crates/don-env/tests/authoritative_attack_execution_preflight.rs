//! Fail-closed ATTACK combat dependencies and prepared-proof freshness.

use don_env::{
    ApplyRefusal, AttackExecutionDependencyRefusal, AuthoritativeBackend, QueuePosition,
    ScenarioSpec, ScenarioUnit, UnitActionRequest,
};
use don_sim::balance::{BalanceTable, BYTES as BALANCE_BYTES};
use don_sim::systems::external_entity_visibility_frontier::VisibilityProjectionFault;
use don_sim::systems::map_terrain::COORD_PER_WCELL;
use don_sim::world::UnitTypeStats;
use std::sync::Arc;

fn spec() -> ScenarioSpec {
    ScenarioSpec {
        seed: 0xa77a_c2026,
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

fn balance(percent: i16) -> Arc<BalanceTable> {
    let mut raw = vec![0u8; BALANCE_BYTES];
    for (attacker, defender) in [(50, 51), (51, 50)] {
        let cell = don_sim::balance_path::table_index(attacker, defender).unwrap() * 2;
        raw[cell..cell + 2].copy_from_slice(&percent.to_le_bytes());
    }
    Arc::new(BalanceTable::from_bytes(&raw).unwrap())
}

fn stats(type_id: i32) -> UnitTypeStats {
    UnitTypeStats {
        type_id,
        attack: 100,
        armor: 0,
        hits: 100,
        recharge: 15,
        max_range: COORD_PER_WCELL,
        min_range: 0,
    }
}

fn install_visibility_sources(backend: &mut AuthoritativeBackend) {
    backend.install_visibility_type_flags(50, 0).unwrap();
    backend.install_visibility_type_flags(51, 0).unwrap();
    backend
        .install_visibility_viewer_mask(0, 0b0000_0001)
        .unwrap();
    backend
        .install_visibility_viewer_mask(1, 0b0000_0010)
        .unwrap();
    // This is a captured setup/read policy, not a claim that Step 12 stamped either fog plane.
    backend.install_visibility_fog_option(3).unwrap();
}

fn visible_attack(backend: &mut AuthoritativeBackend) -> UnitActionRequest {
    backend.step_frames(34);
    backend.capture_external_visibility().unwrap();
    let actor = backend.sim().world.handle_at_row(0).unwrap();
    let target = backend.observe(0).unwrap().external_entities[0];
    UnitActionRequest {
        verb_head: don_env::generated::uv::ATTACK as u16 + 1,
        actor,
        target_x: 0,
        target_y: 0,
        target_entity: target.ordinal,
        queue: QueuePosition::Replace,
        order_flags: 0,
    }
}

#[test]
fn missing_balance_refuses_prepare_mask_and_apply_without_mutation() {
    let mut backend = AuthoritativeBackend::from_spec(spec()).unwrap();
    install_visibility_sources(&mut backend);
    let request = visible_attack(&mut backend);
    let target = backend.observe(0).unwrap().external_entities[0].identity;
    let expected = ApplyRefusal::AttackExecutionUnavailable {
        verb_index: don_env::generated::uv::ATTACK,
        target,
        dependency: AttackExecutionDependencyRefusal::BalanceTableUnavailable,
    };
    let before_digest = backend.sim().world.digest();
    let before_orders = backend.sim().world.orders(0).clone();

    assert_eq!(backend.prepare_attack_target(0, request), Err(expected));
    assert!(!backend
        .unit_verb_mask(0, request)
        .allows(don_env::generated::uv::ATTACK + 1));
    assert_eq!(backend.apply_unit(0, request), Err(expected));
    assert_eq!(backend.sim().world.digest(), before_digest);
    assert_eq!(backend.sim().world.orders(0), &before_orders);
}

#[test]
fn incomplete_type_source_names_the_exact_silent_dependency() {
    let mut backend = AuthoritativeBackend::from_spec(spec()).unwrap();
    backend
        .install_attack_execution_sources(balance(100), vec![stats(50)])
        .unwrap();
    install_visibility_sources(&mut backend);
    let request = visible_attack(&mut backend);
    let target = backend.observe(0).unwrap().external_entities[0].identity;
    let before_digest = backend.sim().world.digest();
    let before_orders = backend.sim().world.orders(0).clone();

    assert_eq!(
        backend.prepare_attack_target(0, request),
        Err(ApplyRefusal::AttackExecutionUnavailable {
            verb_index: don_env::generated::uv::ATTACK,
            target,
            dependency: AttackExecutionDependencyRefusal::DefenderTypeStatsUnavailable {
                type_id: 51,
            },
        })
    );
    assert_eq!(backend.sim().world.digest(), before_digest);
    assert_eq!(backend.sim().world.orders(0), &before_orders);
}

#[test]
fn source_replacement_stales_the_token_and_reset_retains_the_new_proof() {
    let mut backend = AuthoritativeBackend::from_spec(spec()).unwrap();
    backend
        .install_attack_execution_sources(balance(100), vec![stats(50), stats(51)])
        .unwrap();
    install_visibility_sources(&mut backend);
    let request = visible_attack(&mut backend);
    let prepared = backend.prepare_attack_target(0, request).unwrap();
    assert_eq!(prepared.execution_proof().balance_pct(), 100);
    assert!(prepared.execution_proof().predicted_damage() > 0);

    backend
        .install_attack_execution_sources(balance(200), vec![stats(50), stats(51)])
        .unwrap();
    let before_digest = backend.sim().world.digest();
    let before_orders = backend.sim().world.orders(0).clone();
    assert!(matches!(
        backend.apply_prepared_attack_target(prepared),
        Err(ApplyRefusal::TargetVisibility {
            fault: VisibilityProjectionFault::StaleBinding { .. },
            ..
        })
    ));
    assert_eq!(backend.sim().world.digest(), before_digest);
    assert_eq!(backend.sim().world.orders(0), &before_orders);

    backend.capture_external_visibility().unwrap();
    let current = backend.prepare_attack_target(0, request).unwrap();
    assert_eq!(current.execution_proof().balance_pct(), 200);

    backend.reset().unwrap();
    let reset_request = visible_attack(&mut backend);
    let reset_proof = backend
        .prepare_attack_target(0, reset_request)
        .unwrap()
        .execution_proof();
    assert_eq!(reset_proof.balance_pct(), 200);
    assert!(reset_proof.predicted_damage() > 0);
}
