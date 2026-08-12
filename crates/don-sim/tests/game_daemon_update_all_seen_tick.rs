//! Real-tick proof for the exact all-leaders-inactive path through
//! `GameDaemon::update_all_seen` `0x00732840`.

use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::systems::step12_visibility_runtime::{
    ActiveUnitProducerFault, RevealFogNoEffectBlocker, Step12VisibilityAuthority,
    Step12VisibilityPreflightError, VisibilityConstants, VisibilityTypeProjection, UNIT_TYPE_BASE,
};
use don_sim::systems::{map_terrain::tflag, player_setup::ManualPlayerSetup};
use don_sim::tick::{Gap, Sim, StepRun};

fn advance_to_phase_33(sim: &mut Sim) {
    while sim.world.frame < 33 {
        sim.do_frame();
    }
    assert_eq!(sim.world.frame, 33);
}

fn seed_fog_planes(sim: &mut Sim) -> Vec<u8> {
    sim.map.world.seen.fill(0x51);
    sim.map.world.seen2.fill(0xa2);
    sim.map.world.seen3.fill(0xc4);
    sim.map.world.wcoord_seen.fill(0x38);
    sim.map.world.seen2.clone()
}

#[test]
fn inactive_phase33_clears_canonical_planes_and_roundtrips_the_resumed_frame() {
    let mut original = Sim::new(0x7328_4033, 8);
    advance_to_phase_33(&mut original);
    original.game_daemon.busy = 19;
    let seen2_before = seed_fog_planes(&mut original);
    let checksum_before = original.map.world.checksum();

    let trace = original.do_frame();

    assert_eq!(trace.steps[12], StepRun::Executed);
    assert_eq!(original.game_daemon.busy, 4);
    assert_eq!(original.cover.gaps[Gap::GameDaemonUpdateAllSeen.index()], 0);
    assert!(original.map.world.seen.iter().all(|&byte| byte == 0));
    assert!(original.map.world.seen3.iter().all(|&byte| byte == 0));
    assert!(original.map.world.wcoord_seen.iter().all(|&byte| byte == 0));
    assert_eq!(original.map.world.seen2, seen2_before);
    assert_ne!(original.map.world.checksum(), checksum_before);

    let saved = save_sim(&original).unwrap();
    let mut resumed = load_sim(&saved).unwrap();
    assert_eq!(resumed.map.world.checksum(), original.map.world.checksum());
    assert_eq!(save_sim(&resumed).unwrap(), saved);

    original.do_frame();
    resumed.do_frame();
    assert_eq!(resumed.channel_digest(), original.channel_digest());
    assert_eq!(resumed.map.world.seen, original.map.world.seen);
    assert_eq!(resumed.map.world.seen2, original.map.world.seen2);
    assert_eq!(resumed.map.world.seen3, original.map.world.seen3);
    assert_eq!(
        resumed.map.world.wcoord_seen,
        original.map.world.wcoord_seen
    );
    assert_eq!(save_sim(&resumed).unwrap(), save_sim(&original).unwrap());
}

#[test]
fn active_empty_leader_executes_the_exact_empty_unit_cohort() {
    let mut sim = Sim::new(0x7328_40ff, 8);
    sim.activate(0);
    sim.world.frame = 33;
    sim.game_daemon.busy = 19;
    seed_fog_planes(&mut sim);
    let seen2_before = sim.map.world.seen2.clone();

    let trace = sim.do_frame();

    assert_eq!(trace.steps[12], StepRun::Executed);
    assert_eq!(sim.game_daemon.busy, 4);
    assert_eq!(sim.cover.gaps[Gap::GameDaemonUpdateAllSeen.index()], 0);
    assert!(sim.map.world.seen.iter().all(|&byte| byte == 0));
    assert_eq!(sim.map.world.seen2, seen2_before);
    assert!(sim.map.world.seen3.iter().all(|&byte| byte == 0));
    assert!(sim.map.world.wcoord_seen.iter().all(|&byte| byte == 0));
    assert_eq!(sim.step12_visibility_error, None);
}

fn active_unit_phase33() -> Sim {
    let mut sim = Sim::new(0x7328_4133, 8);
    let mut setup = ManualPlayerSetup {
        active_mask: 1,
        local_player_setup_slot: 0,
        ..ManualPlayerSetup::default()
    };
    setup.teams[0] = 0;
    sim.start_manual_player_setup(setup).unwrap();
    advance_to_phase_33(&mut sim);

    sim.replace_step12_visibility_type_source(
        1,
        0x7328_4133,
        VisibilityConstants {
            ptolemy_los_bonus: 0,
            the_ceo_unit_los: 0,
        },
        vec![VisibilityTypeProjection {
            type_index: UNIT_TYPE_BASE,
            object_masks: 0,
            domain: 0,
            unit_flags2: 0,
            role: 0,
            is_siege: false,
        }],
    )
    .unwrap();
    let unit = sim.spawn_unit(0, UNIT_TYPE_BASE, 6_000, 6_000, 6).unwrap();
    sim.materialize_step12_object_init(unit).unwrap();

    for cell in &mut sim.map.world.wdata {
        cell.who = 0;
    }
    for tile in &mut sim.map.world.tdata {
        *tile &= !tflag::RESOURCE;
    }
    sim
}

#[test]
fn active_unit_phase33_stamps_canonical_planes_and_roundtrips_the_resumed_frame() {
    let mut original = active_unit_phase33();
    original.game_daemon.busy = 19;
    seed_fog_planes(&mut original);
    let checksum_before = original.map.world.checksum();

    let trace = original.do_frame();

    assert_eq!(trace.steps[12], StepRun::Executed);
    assert_eq!(original.game_daemon.busy, 4);
    assert_eq!(original.cover.gaps[Gap::GameDaemonUpdateAllSeen.index()], 0);
    assert_eq!(original.step12_visibility_error, None);
    assert!(original.map.world.seen.iter().any(|&byte| byte & 1 != 0));
    assert!(original.map.world.seen2.iter().any(|&byte| byte == 0xa3));
    assert!(original.map.world.seen3.iter().all(|&byte| byte == 0));
    assert!(original
        .map
        .world
        .wcoord_seen
        .iter()
        .any(|&byte| byte & 1 != 0));
    assert_ne!(original.map.world.checksum(), checksum_before);

    // This authority is an externally reinstallable rules projection and deliberately is
    // not serialized. The checksum-visible result above must survive independently.
    original.step12_visibility = Step12VisibilityAuthority::default();
    let saved = save_sim(&original).unwrap();
    let mut resumed = load_sim(&saved).unwrap();
    assert_eq!(resumed.map.world.checksum(), original.map.world.checksum());
    assert_eq!(save_sim(&resumed).unwrap(), saved);

    original.do_frame();
    resumed.do_frame();
    assert_eq!(resumed.channel_digest(), original.channel_digest());
    assert_eq!(save_sim(&resumed).unwrap(), save_sim(&original).unwrap());
}

#[test]
fn effectful_reveal_fog_refuses_before_daemon_or_plane_mutation() {
    let mut sim = active_unit_phase33();
    sim.game_daemon.busy = 19;
    seed_fog_planes(&mut sim);
    sim.map.world.tdata.fill(tflag::RESOURCE);
    let daemon_before = sim.game_daemon;
    let seen_before = sim.map.world.seen.clone();
    let seen2_before = sim.map.world.seen2.clone();
    let seen3_before = sim.map.world.seen3.clone();
    let wcoord_seen_before = sim.map.world.wcoord_seen.clone();

    let trace = sim.do_frame();

    assert_eq!(
        trace.steps[12],
        StepRun::Unimplemented(Gap::GameDaemonUpdateAllSeen)
    );
    assert_eq!(sim.game_daemon, daemon_before);
    assert_eq!(sim.map.world.seen, seen_before);
    assert_eq!(sim.map.world.seen2, seen2_before);
    assert_eq!(sim.map.world.seen3, seen3_before);
    assert_eq!(sim.map.world.wcoord_seen, wcoord_seen_before);
    assert!(matches!(
        sim.step12_visibility_error,
        Some(Step12VisibilityPreflightError::ActiveUnitCohort(
            ActiveUnitProducerFault::RevealFogEffectful {
                blocker: RevealFogNoEffectBlocker::ResourceTile,
                ..
            }
        ))
    ));
}

#[test]
fn visible_local_seen_refuses_before_daemon_or_plane_mutation() {
    let mut sim = active_unit_phase33();
    sim.game_daemon.busy = 19;
    seed_fog_planes(&mut sim);
    sim.world.units.visible_mut()[0] = 1;
    let daemon_before = sim.game_daemon;
    let planes_before = (
        sim.map.world.seen.clone(),
        sim.map.world.seen2.clone(),
        sim.map.world.seen3.clone(),
        sim.map.world.wcoord_seen.clone(),
    );

    let trace = sim.do_frame();

    assert_eq!(
        trace.steps[12],
        StepRun::Unimplemented(Gap::GameDaemonUpdateAllSeen)
    );
    assert_eq!(sim.game_daemon, daemon_before);
    assert_eq!(
        (
            &sim.map.world.seen,
            &sim.map.world.seen2,
            &sim.map.world.seen3,
            &sim.map.world.wcoord_seen,
        ),
        (
            &planes_before.0,
            &planes_before.1,
            &planes_before.2,
            &planes_before.3,
        )
    );
    assert_eq!(
        sim.step12_visibility_error,
        Some(Step12VisibilityPreflightError::ActiveUnitCohort(
            ActiveUnitProducerFault::VisibleLocalSeen { row: 0, visible: 1 }
        ))
    );
}

#[test]
fn clear_item_flag_returns_before_source_auto_explore_mask_is_read() {
    let mut sim = active_unit_phase33();
    sim.game_daemon.busy = 19;
    seed_fog_planes(&mut sim);
    sim.world.units.set_unit_masks(0, 0x100);

    let trace = sim.do_frame();

    assert_eq!(trace.steps[12], StepRun::Executed);
    assert_eq!(sim.game_daemon.busy, 4);
    assert_eq!(sim.step12_visibility_error, None);
    assert!(sim.map.world.seen.iter().any(|&byte| byte & 1 != 0));
}
