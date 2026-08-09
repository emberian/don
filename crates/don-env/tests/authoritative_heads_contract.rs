//! Generated RL heads enter the authoritative backend without compact-world fallback.

use don_env::{
    decode_unit_heads, ApplyReceipt, ApplyRefusal, AuthoritativeBackend, EnvironmentBackend,
    FactoredApplyRefusal, FactoredUnitActionSpace, HeadDecodeRefusal, QueuePosition, ScenarioSpec,
    ScenarioUnit,
};
use don_sim::systems::map_terrain::COORD_PER_WCELL;
use don_sim::world::SUBTILE;

fn spec() -> ScenarioSpec {
    ScenarioSpec {
        seed: 0xa17_2026,
        map_wcells: 4,
        active_players: vec![0],
        units: vec![ScenarioUnit {
            who: 0,
            type_id: 50,
            x: COORD_PER_WCELL,
            y: COORD_PER_WCELL,
            los_tiles: 4,
        }],
    }
}

fn space() -> FactoredUnitActionSpace {
    FactoredUnitActionSpace {
        grid_w: 64,
        grid_h: 64,
        max_entities: 32,
    }
}

#[test]
fn generated_head_layout_decodes_cells_queue_and_order_bits_exactly() {
    let backend = AuthoritativeBackend::from_spec(spec()).unwrap();
    let actor = backend.sim().world.handle_at_row(0).unwrap();
    let mut heads = [0; don_env::generated::N_UNIT_HEADS];
    heads[don_env::generated::UnitHead::Verb as usize] = don_env::generated::uv::MOVE_TO as i32 + 1;
    heads[don_env::generated::UnitHead::TargetX as usize] = 7;
    heads[don_env::generated::UnitHead::TargetY as usize] = 11;
    heads[don_env::generated::UnitHead::QueuePos as usize] = 2;
    heads[don_env::generated::UnitHead::OrderMods as usize] = 2;

    let request = decode_unit_heads(actor, &heads, space()).unwrap();
    assert_eq!(
        request.verb_head,
        don_env::generated::uv::MOVE_TO as u16 + 1
    );
    assert_eq!(request.target_x, 7 * SUBTILE + SUBTILE / 2);
    assert_eq!(request.target_y, 11 * SUBTILE + SUBTILE / 2);
    assert_eq!(request.queue, QueuePosition::Replace);
    assert_eq!(request.order_flags, 2);
}

#[test]
fn malformed_heads_refuse_before_mutating_the_authoritative_owner() {
    let mut backend = AuthoritativeBackend::from_spec(spec()).unwrap();
    let actor = backend.sim().world.handle_at_row(0).unwrap();
    let before = backend.sim().world.digest();
    let mut heads = [0; don_env::generated::N_UNIT_HEADS];
    heads[don_env::generated::UnitHead::TargetX as usize] = -1;

    assert_eq!(
        backend.apply_unit_heads(0, actor, &heads, space()),
        Err(FactoredApplyRefusal::Decode(
            HeadDecodeRefusal::ValueOutOfRange {
                head: don_env::generated::UnitHead::TargetX as usize,
                value: -1,
                exclusive_max: 64,
            }
        ))
    );
    assert_eq!(backend.sim().world.digest(), before);
}

#[test]
fn attack_target_projection_keeps_its_typed_refusal_and_does_not_fall_back() {
    let mut backend = AuthoritativeBackend::from_spec(spec()).unwrap();
    let actor = backend.sim().world.handle_at_row(0).unwrap();
    let before = backend.sim().world.digest();
    let mut heads = [0; don_env::generated::N_UNIT_HEADS];
    heads[don_env::generated::UnitHead::Verb as usize] = don_env::generated::uv::ATTACK as i32 + 1;
    heads[don_env::generated::UnitHead::TargetEntity as usize] = 1;

    assert_eq!(
        backend.apply_unit_heads(0, actor, &heads, space()),
        Err(FactoredApplyRefusal::Apply(
            ApplyRefusal::TargetIdentityVisibilityUnavailable {
                verb_index: don_env::generated::uv::ATTACK,
                target_entity: 1,
            }
        ))
    );
    assert_eq!(backend.sim().world.digest(), before);
}

#[test]
fn noop_heads_share_the_authoritative_owner_and_leave_it_unchanged() {
    let mut selected = EnvironmentBackend::authoritative(spec()).unwrap();
    let backend = selected.as_authoritative_mut().unwrap();
    let actor = backend.sim().world.handle_at_row(0).unwrap();
    let before = backend.sim().world.digest();

    assert_eq!(
        backend.apply_unit_heads(0, actor, &[0; don_env::generated::N_UNIT_HEADS], space(),),
        Ok(ApplyReceipt::Noop { frame: 0 })
    );
    assert_eq!(backend.sim().world.digest(), before);
}
