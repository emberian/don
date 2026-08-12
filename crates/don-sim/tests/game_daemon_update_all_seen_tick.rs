//! Real-tick proof for the exact all-leaders-inactive path through
//! `GameDaemon::update_all_seen` `0x00732840`.

use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::systems::step12_visibility_runtime::{
    Step12ProducerResiduals, Step12VisibilityPreflightError,
};
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
fn active_empty_leader_refuses_before_daemon_or_fog_mutation() {
    let mut sim = Sim::new(0x7328_40ff, 8);
    sim.activate(0);
    sim.world.frame = 33;
    sim.game_daemon.busy = 19;
    seed_fog_planes(&mut sim);
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
    assert_eq!(
        sim.step12_visibility_error,
        Some(Step12VisibilityPreflightError::IncompleteProducer {
            prepared_unit_stamps: 0,
            residuals: Step12ProducerResiduals::MISSING,
        })
    );
}
