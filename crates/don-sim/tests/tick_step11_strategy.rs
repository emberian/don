use don_sim::systems::{leaders, victory_score};
use don_sim::tick::{Gap, Sim, StepRun};

#[test]
fn real_tick_executes_strategy_dispatch_and_explore_prefix() {
    let mut sim = Sim::new(0x11a7, 16);
    sim.activate(0);

    // Slot zero is due when `frame + 12 == 200`. Fill the persistent explored plane,
    // then jump to that pre-increment frame so step 11 must recount every region cell.
    sim.world.frame = 188;
    sim.map.world.seen2.fill(1);
    let expected = sim.map.world.reg_size;

    let tick = sim.do_frame();

    assert_eq!(tick.steps[11], StepRun::Executed);
    // Retail crosses four call boundaries per entered slot, but `Leader::plan_strategy`'s
    // is now executed: at frame 188 slot 0's planner phase is 188, `188 % 30 != 0`, and
    // `0x006B9698` returns without reading anything. Charging it was over-reporting.
    assert_eq!(
        tick.work[11], 3,
        "check_explore, compute_score and the reached diplomacy body"
    );
    assert_eq!(sim.step8.leaders[0].explored, expected);
    assert_eq!(sim.cover.gaps[Gap::LeaderCheckExplore.index()], 0);
    assert_eq!(sim.cover.gaps[Gap::LeaderPlanStrategy.index()], 0);
    assert_eq!(sim.cover.gaps[Gap::LeaderDiplomacy.index()], 1);
    assert_eq!(sim.step8.last_strategy.plan_calls_fully_owned(), 1);
    assert_eq!(sim.step8.last_strategy.diplomacy_calls_refused(), 0);
}

#[test]
fn electronics_populates_the_retail_explore_all_prerequisite() {
    let mut sim = Sim::new(0x11a8, 16);
    sim.activate(0);
    sim.vic_leaders.slots[0].has_tech[leaders::EXPLORE_ALL_PREREQ] = true;
    sim.map.world.seen2.fill(0);
    let expected = sim.map.world.reg_size;

    let tick = sim.do_frame();

    assert_eq!(tick.steps[11], StepRun::Executed);
    assert_eq!(sim.step8.leaders[0].explored, expected);
    assert_eq!(sim.cover.gaps[Gap::LeaderCheckExplore.index()], 0);
}

#[test]
fn strategy_victory_tail_executes_without_an_active_leader() {
    let mut sim = Sim::new(0x11b7, 16);
    sim.vic_match
        .set_sem(victory_score::game_sem::CHECK_VICTORY_MODE);

    let tick = sim.do_frame();

    assert_eq!(tick.steps[11], StepRun::Executed);
    assert_eq!(
        tick.work[11], 1,
        "the semaphore-gated tail call is outside the loop"
    );
    assert_eq!(sim.cover.gaps[Gap::LeaderCheckExplore.index()], 0);
    assert_eq!(sim.cover.gaps[Gap::LeaderPlanStrategy.index()], 0);
    assert_eq!(sim.cover.gaps[Gap::LeaderDiplomacy.index()], 0);
}

#[test]
fn strategy_requires_both_low_leader_flags() {
    let mut sim = Sim::new(0x11c7, 16);
    sim.activate(0);
    sim.step8.leaders[0].flags &= !leaders::flag::PROCESS;

    let tick = sim.do_frame();

    assert_eq!(tick.steps[11], StepRun::Vacuous);
    assert_eq!(tick.work[11], 0);
    assert_eq!(sim.cover.gaps[Gap::LeaderPlanStrategy.index()], 0);
    assert_eq!(sim.cover.gaps[Gap::LeaderDiplomacy.index()], 0);
}
