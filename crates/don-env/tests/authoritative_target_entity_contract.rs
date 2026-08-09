//! Target-taking policy heads stay fail-closed until their observation identity is exact.

use don_env::authoritative_backend::{
    ApplyRefusal, FactoredApplyRefusal, FactoredUnitActionSpace, QueuePosition, UnitActionRequest,
    VerbRoute, UNIT_INTEGRATION,
};
use don_env::{decode_unit_heads, AuthoritativeBackend, ScenarioSpec, ScenarioUnit};
use don_sim::systems::map_terrain::COORD_PER_WCELL;

fn spec() -> ScenarioSpec {
    ScenarioSpec {
        seed: 0xa771_2026,
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

fn space() -> FactoredUnitActionSpace {
    FactoredUnitActionSpace {
        grid_w: 16,
        grid_h: 16,
        max_entities: 2,
    }
}

fn attack_heads(target_entity: i32) -> [i32; don_env::generated::N_UNIT_HEADS] {
    let mut heads = [0; don_env::generated::N_UNIT_HEADS];
    heads[don_env::generated::UnitHead::Verb as usize] = don_env::generated::uv::ATTACK as i32 + 1;
    heads[don_env::generated::UnitHead::TargetEntity as usize] = target_entity;
    heads[don_env::generated::UnitHead::QueuePos as usize] = 2;
    heads
}

#[test]
fn target_entity_survives_strict_factored_decode() {
    let backend = AuthoritativeBackend::from_spec(spec()).unwrap();
    let actor = backend.sim().world.handle_at_row(0).unwrap();
    let request = decode_unit_heads(actor, &attack_heads(2), space()).unwrap();

    assert_eq!(request.verb_head, don_env::generated::uv::ATTACK as u16 + 1);
    assert_eq!(request.target_entity, 2);
    assert_eq!(request.queue, QueuePosition::Replace);
}

#[test]
fn attack_route_is_real_but_mask_and_apply_stop_at_external_identity_visibility() {
    assert_eq!(
        UNIT_INTEGRATION[don_env::generated::uv::ATTACK].route,
        VerbRoute::SimAttackIssue
    );

    let mut backend = AuthoritativeBackend::from_spec(spec()).unwrap();
    let actor = backend.sim().world.handle_at_row(0).unwrap();
    let request = decode_unit_heads(actor, &attack_heads(2), space()).unwrap();
    let before_digest = backend.sim().world.digest();
    let before_orders = backend.sim().world.orders(0).clone();
    let before_paths = backend.sim().paths.clone();

    let mask = backend.unit_verb_mask(0, request);
    assert!(mask.allows(0));
    assert!(!mask.allows(don_env::generated::uv::ATTACK + 1));
    assert_eq!(backend.sim().world.digest(), before_digest);

    assert_eq!(
        backend.apply_unit_heads(0, actor, &attack_heads(2), space()),
        Err(FactoredApplyRefusal::Apply(
            ApplyRefusal::TargetIdentityVisibilityUnavailable {
                verb_index: don_env::generated::uv::ATTACK,
                target_entity: 2,
            }
        ))
    );
    assert_eq!(backend.sim().world.digest(), before_digest);
    assert_eq!(backend.sim().world.orders(0), &before_orders);
    assert_eq!(backend.sim().paths, before_paths);
}

#[test]
fn missing_target_and_reset_replay_the_same_zero_mutation_refusals() {
    let mut backend = AuthoritativeBackend::from_spec(spec()).unwrap();
    let actor = backend.sim().world.handle_at_row(0).unwrap();
    let before_digest = backend.sim().world.digest();
    let missing = UnitActionRequest {
        verb_head: don_env::generated::uv::ATTACK as u16 + 1,
        actor,
        target_x: 0,
        target_y: 0,
        target_entity: 0,
        queue: QueuePosition::Replace,
        order_flags: 0,
    };

    assert_eq!(
        backend.apply_unit(0, missing),
        Err(ApplyRefusal::MissingTargetEntity {
            verb_index: don_env::generated::uv::ATTACK,
        })
    );
    assert_eq!(backend.sim().world.digest(), before_digest);

    backend.reset().unwrap();
    let reset_actor = backend.sim().world.handle_at_row(0).unwrap();
    assert_eq!(reset_actor, actor);
    assert_eq!(backend.sim().world.digest(), before_digest);
    assert_eq!(
        backend.apply_unit_heads(0, reset_actor, &attack_heads(2), space()),
        Err(FactoredApplyRefusal::Apply(
            ApplyRefusal::TargetIdentityVisibilityUnavailable {
                verb_index: don_env::generated::uv::ATTACK,
                target_entity: 2,
            }
        ))
    );
    assert_eq!(backend.sim().world.digest(), before_digest);
}
