use don_sim::objects::BUILD_BAND_BASE;
use don_sim::systems::leaders::{ObjectHitInputs, StatObject};
use don_sim::systems::player_setup::ManualPlayerSetup;
use don_sim::systems::production::{self, BuildData};
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

fn active_post_step_sim() -> Sim {
    let mut sim = Sim::new(0x5a8e_2026, 8);
    sim.map.world.seed = 0x5a8e_2026;
    let mut setup = ManualPlayerSetup {
        active_mask: 0x01,
        local_player_setup_slot: 0,
        ..ManualPlayerSetup::default()
    };
    setup.teams[0] = 0;
    sim.start_manual_player_setup(setup).unwrap();
    sim.spawn_unit(0, 17, 768, 768, 4).unwrap();

    let mut build = BuildData {
        flags: production::flag::VALID | production::flag::STARTED | production::flag::ACTIVE,
        myhits: 100,
        construct_hits: 100,
        job_counter: 1000,
        constr_time: 1000,
        gather_down: -1,
        city: -1,
        city_down: -1,
        wonder: -1,
        dock: -1,
        attack_ox: -1,
        attack_whom: -1,
        ..BuildData::default()
    };
    build.other[0x28..0x2a].copy_from_slice(&(-1i16).to_le_bytes());
    build.other[production::off::OBJECT_ID..production::off::OBJECT_ID + 2]
        .copy_from_slice(&(BUILD_BAND_BASE as i16).to_le_bytes());
    sim.spawn_build(0, build);

    sim.do_frame();
    assert!(sim.step8_env.leaders[0].objects.units[0].owner_in_game);
    assert!(sim.step8_env.leaders[0].objects.band_2000[0].owner_in_game);
    sim
}

#[test]
fn exact_post_step_views_roundtrip_without_a_shadow_owner() {
    let mut original = post_step_sim();
    let bytes = save_sim(&original).unwrap();
    let mut loaded = load_sim(&bytes).unwrap();

    // The object views are derived rather than serialized owners.  Load rebuilds the exact
    // current-registry prefix accepted by save admission, so the reconstructed adapter must
    // equal the pre-save projection; expecting it to remain constructor-empty predates the
    // exact-prefix restoration in `save_load::step8_views`.
    let loaded_objects = &loaded.step8_env.leaders[0].objects;
    let original_objects = &original.step8_env.leaders[0].objects;
    assert_eq!(loaded_objects.units, original_objects.units);
    assert_eq!(loaded_objects.band_2000, original_objects.band_2000);
    assert_eq!(loaded_objects.band_3000, original_objects.band_3000);
    assert_eq!(save_sim(&loaded).unwrap(), bytes);

    original.do_frame();
    loaded.do_frame();
    assert_eq!(original.channel_digest(), loaded.channel_digest());
    assert_eq!(save_sim(&original).unwrap(), save_sim(&loaded).unwrap());
}

#[test]
fn active_owner_query_is_derived_for_unit_and_build_views() {
    let original = active_post_step_sim();
    let bytes = save_sim(&original).unwrap();
    let loaded = load_sim(&bytes).unwrap();

    assert_eq!(loaded.world.frame, original.world.frame);
    assert_eq!(loaded.channel_digest(), original.channel_digest());
    assert_eq!(save_sim(&loaded).unwrap(), bytes);
}

#[test]
fn active_owner_query_disagreement_is_still_refused() {
    let mut unit_mismatch = active_post_step_sim();
    unit_mismatch.step8_env.leaders[0].objects.units[0].owner_in_game = false;
    assert_eq!(
        save_sim(&unit_mismatch),
        Err(SaveError::Unsupported("step-8 leader state/hosts"))
    );

    let mut build_mismatch = active_post_step_sim();
    build_mismatch.step8_env.leaders[0].objects.band_2000[0].owner_in_game = false;
    assert_eq!(
        save_sim(&build_mismatch),
        Err(SaveError::Unsupported("step-8 leader state/hosts"))
    );

    let mut end_flag_mismatch = active_post_step_sim();
    end_flag_mismatch.step8.end.players[0].flags |= 0x4000;
    assert_eq!(
        save_sim(&end_flag_mismatch),
        Err(SaveError::Unsupported("step-8 leader state/hosts"))
    );

    let mut end_identity_mismatch = active_post_step_sim();
    end_identity_mismatch.step8.end.players[0].who = 7;
    assert_eq!(
        save_sim(&end_identity_mismatch),
        Err(SaveError::Unsupported("step-8 leader state/hosts"))
    );
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
