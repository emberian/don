use don_sim::systems::leaders;
use don_sim::tick::{Gap, Sim, StepRun};

#[test]
fn real_tick_executes_event_frame_fold_at_step_nineteen() {
    let mut sim = Sim::new(0x19e0, 16);
    sim.activate(0);
    sim.step8.leaders[0].event_frame.deaths_current_frame = 1;

    let tick = sim.do_frame();
    let event = sim.step8.leaders[0].event_frame;

    assert_eq!(tick.frame, 0);
    assert_eq!(tick.steps[19], StepRun::Executed);
    assert_eq!(tick.work[19], 1);
    assert_eq!(event.deaths_fifteen_seconds, 1);
    assert_eq!(event.average_death_rate, 50);
    assert_eq!(event.deaths_current_frame, 0);
    assert_eq!(sim.cover.gaps[Gap::LeaderProcessEventFrame.index()], 0);
}

#[test]
fn real_tick_non_due_event_frame_returns_without_mutation() {
    let mut sim = Sim::new(0x19e1, 16);
    sim.activate(0);
    sim.world.frame = 1;
    sim.step8.leaders[0].event_frame.deaths_current_frame = 7;

    let tick = sim.do_frame();

    assert_eq!(tick.steps[19], StepRun::Executed);
    assert_eq!(tick.work[19], 1);
    assert_eq!(sim.step8.leaders[0].event_frame.deaths_current_frame, 7);
    assert_eq!(sim.step8.leaders[0].event_frame.deaths_fifteen_seconds, 0);
}

#[test]
fn real_tick_event_dispatcher_uses_in_game_not_process() {
    let mut sim = Sim::new(0x19e2, 16);
    sim.activate(0);
    sim.step8.leaders[0].flags &= !leaders::flag::IN_GAME;

    let tick = sim.do_frame();

    assert_eq!(tick.steps[19], StepRun::Vacuous);
    assert_eq!(tick.work[19], 0);
    assert_eq!(sim.cover.gaps[Gap::LeaderProcessEventFrame.index()], 0);
}
