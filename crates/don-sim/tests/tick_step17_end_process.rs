use don_sim::schedule::{StepStatus, DO_FRAME};
use don_sim::systems::leaders;
use don_sim::tick::{Sim, StepRun};

#[test]
fn real_tick_executes_end_process_and_clears_population_warning() {
    let mut sim = Sim::new(0x17e0, 16);
    sim.activate(0);
    sim.step8.end.players[0].flags |= leaders::PLAYER_POP_CAP_WARNING;

    let tick = sim.do_frame();

    assert_eq!(tick.steps[17], StepRun::Executed);
    assert_eq!(tick.work[17], 1);
    assert_eq!(
        sim.step8.end.players[0].flags & leaders::PLAYER_POP_CAP_WARNING,
        0,
        "the step-8 reset makes the active leader take retail's zero-issues cleanup arm"
    );
    assert_ne!(
        sim.step8.end.players[0].flags & leaders::PLAYER_VALID,
        0,
        "the tick adapter refreshes the separate GameInfo::Player valid bit"
    );
    assert_eq!(DO_FRAME[17].status, StepStatus::Implemented);
}

#[test]
fn real_tick_reports_end_process_vacuous_without_processed_leaders() {
    let mut sim = Sim::new(0x17e1, 16);

    let tick = sim.do_frame();

    assert_eq!(tick.steps[17], StepRun::Vacuous);
    assert_eq!(tick.work[17], 0);
    assert_eq!(DO_FRAME[17].status, StepStatus::Implemented);
}
