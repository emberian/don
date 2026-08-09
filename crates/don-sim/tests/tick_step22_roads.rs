use don_sim::systems::{map_terrain::tflag, roads::CandidateFact};
use don_sim::tick::{Gap, Sim, StepRun};

#[test]
fn real_tick_executes_the_retail_step_twenty_two_scan_budget() {
    let mut sim = Sim::new(0x22a0, 64);

    let tick = sim.do_frame();

    assert_eq!(tick.steps[22], StepRun::Executed);
    assert_eq!(tick.work[22], 128);
    assert_eq!((sim.road_scan.curscan_x, sim.road_scan.curscan_y), (8, 0));
    assert_eq!(sim.cover.gaps[Gap::RoadsScanStray.index()], 0);
}

#[test]
fn live_road_without_renderer_candidate_fails_closed_and_is_charged() {
    let mut sim = Sim::new(0x22a1, 64);
    sim.map.world.set_road_at(4, 0, true, 0, true);

    let tick = sim.do_frame();

    assert_eq!(tick.steps[22], StepRun::Executed);
    assert_eq!(tick.work[22], 128);
    assert_eq!(
        sim.map.world.tmask(4, 0) & tflag::SURFACE_MASK,
        tflag::SURFACE_ROAD
    );
    assert!(sim.cover.gaps[Gap::RoadsScanStray.index()] > 0);
}

#[test]
fn supplied_absent_candidate_executes_the_live_road_clear() {
    let mut sim = Sim::new(0x22a2, 64);
    sim.map.world.set_road_at(4, 0, true, 0, true);
    sim.road_scan.set_candidate(4, 0, CandidateFact::Absent);

    let tick = sim.do_frame();

    assert_eq!(tick.steps[22], StepRun::Executed);
    assert_eq!(tick.work[22], 128);
    assert_ne!(
        sim.map.world.tmask(4, 0) & tflag::SURFACE_MASK,
        tflag::SURFACE_ROAD
    );
    assert_eq!(sim.road_scan.last_cleared, [(4, 0)]);
    assert_eq!(sim.cover.gaps[Gap::RoadsScanStray.index()], 0);
}
