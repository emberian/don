use don_sim::systems::armies::{ST_HURRY, ST_MUSTERING};
use don_sim::tick::{Gap, Sim, StepRun};

#[test]
fn real_tick_executes_step_thirteen_over_the_preallocated_slots() {
    let mut sim = Sim::new(0x13a0, 16);
    sim.activate(0);

    let tick = sim.do_frame();

    assert_eq!(tick.steps[13], StepRun::Executed);
    assert_eq!(tick.work[13], 16);
    assert_eq!(sim.cover.gaps[Gap::ArmiesProcessAll.index()], 0);
}

#[test]
fn step_thirteen_is_vacuous_when_no_owner_passes_the_retail_gate() {
    let mut sim = Sim::new(0x13a1, 16);

    let tick = sim.do_frame();

    assert_eq!(tick.steps[13], StepRun::Vacuous);
    assert_eq!(tick.work[13], 0);
}

#[test]
fn leader_flags_two_suppresses_the_owner_exactly() {
    let mut sim = Sim::new(0x13a2, 16);
    sim.activate(0);
    sim.army_leader_flags2[0] = 0x02;

    let tick = sim.do_frame();

    assert_eq!(tick.steps[13], StepRun::Vacuous);
    assert_eq!(tick.work[13], 0);
}

#[test]
fn valid_army_without_complete_live_host_is_preserved_and_charged() {
    let mut sim = Sim::new(0x13a3, 16);
    sim.activate(0);
    sim.armies.lists[0][0].valid = 1;
    sim.armies.lists[0][0].status = ST_HURRY | ST_MUSTERING;
    let before = sim.armies.lists[0][0].clone();

    let tick = sim.do_frame();

    assert_eq!(tick.steps[13], StepRun::Executed);
    assert_eq!(tick.work[13], 16);
    assert_eq!(sim.cover.gaps[Gap::ArmiesProcessAll.index()], 1);
    assert_eq!(sim.armies.lists[0][0], before);
}
