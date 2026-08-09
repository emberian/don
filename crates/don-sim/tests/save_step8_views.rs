use don_sim::systems::leaders::{ObjectHitInputs, StatObject};
use don_sim::systems::save_load::{load_sim, save_sim, SaveError};
use don_sim::tick::Sim;

fn post_step_sim() -> Sim {
    let mut sim = Sim::new(0x5a8e, 8);
    sim.map.world.seed = 0x5a8e;
    sim.leaders[0].econ.stockpile = [1, 2, 3, 4, 5, 6];
    sim.leaders[0].last_calc_frame = -17;
    sim.leaders[0].dirty = true;
    sim.spawn_unit(0, 17, 768, 768, 4).unwrap();
    sim.do_frame();
    assert_eq!(sim.step8_env.leaders[0].objects.units.len(), 1);
    sim
}

#[test]
fn exact_post_step_views_roundtrip_without_a_shadow_owner() {
    let mut original = post_step_sim();
    let bytes = save_sim(&original).unwrap();
    let mut loaded = load_sim(&bytes).unwrap();

    assert!(loaded.step8_env.leaders[0].objects.units.is_empty());
    assert_eq!(save_sim(&loaded).unwrap(), bytes);

    original.do_frame();
    loaded.do_frame();
    assert_eq!(original.channel_digest(), loaded.channel_digest());
    assert_eq!(save_sim(&original).unwrap(), save_sim(&loaded).unwrap());
}

#[test]
fn independently_mutated_view_state_is_still_refused_atomically() {
    let mut sim = post_step_sim();
    let before = sim.channel_digest();
    sim.step8_env.leaders[0].objects.units[0].hit_inputs = Some(ObjectHitInputs {
        base_hits: 99,
        ..ObjectHitInputs::default()
    });

    assert_eq!(
        save_sim(&sim),
        Err(SaveError::Unsupported("step-8 leader state/hosts"))
    );
    assert_eq!(sim.channel_digest(), before);
}

#[test]
fn malformed_or_stale_derived_views_are_refused() {
    let mut sim = post_step_sim();
    sim.step8_env.leaders[0]
        .objects
        .units
        .push(StatObject::default());
    assert_eq!(
        save_sim(&sim),
        Err(SaveError::Unsupported("step-8 leader state/hosts"))
    );

    let mut sim = post_step_sim();
    sim.step8_env.leaders[0].objects.units[0].v160_calls = 1;
    assert_eq!(
        save_sim(&sim),
        Err(SaveError::Unsupported("step-8 leader state/hosts"))
    );
}
