// SPDX-License-Identifier: GPL-3.0-or-later

//! Step 11's two AI children, driven through the real `Sim::do_frame`.
//!
//! Every test here goes through the tick, not through the pure functions: the point is
//! that `Leader::plan_strategy` `0x006B9620` and `Leader::production_ai` `0x006C1960` now
//! execute inside `Leaders::strategy_all` `0x006ED430`, and that `Gap::LeaderPlanStrategy`
//! and `Gap::LeaderDiplomacy` are charged only where retail entered something this port
//! does not run.

use don_sim::systems::leader_production_ai::{
    flags, flags2, AiEnv, PlanArm, ProductionGate, Stage, Stall, INFINITE_RESOURCES,
    SCRIPT_BLOCK_ON_THIS, SCRIPT_DONE,
};
use don_sim::systems::leaders::{self, DiplomacyGate};
use don_sim::systems::{leaders_diplomacy_opening_frontier as diplo, victory_score};
use don_sim::tick::{Gap, Sim, StepRun};

fn one_active_leader(seed: u64) -> Sim {
    let mut sim = Sim::new(seed, 16);
    sim.activate(0);
    sim
}

/// Which arm of `Leader::plan_strategy` step 11 took for this slot on the last frame.
fn plan_arm(sim: &Sim, slot: usize) -> Option<PlanArm> {
    sim.step8.last_strategy.plan[slot]
        .as_ref()
        .map(|plan| plan.arm.clone())
}

/// Which condition of `Leader::diplomacy`'s entry gate fired for this slot.
fn diplomacy_gate(sim: &Sim, slot: usize) -> Option<DiplomacyGate> {
    sim.step8.last_strategy.diplomacy[slot]
}

/// The measurement this lane exists for.
///
/// Slot 0's planner phase is `frame % 200`. Over 200 consecutive frames retail enters the
/// 11,108-byte planning body once (`phase == 0`) and opens the `MakeList` fast lane six
/// times (`phase in {30,60,90,120,150,180}`). The other 193 calls return at `0x006B9698`
/// having touched nothing at all. Before this lane the tick charged all 200.
#[test]
fn the_planner_gap_is_charged_seven_frames_in_two_hundred() {
    let mut sim = one_active_leader(0x11d7);
    // Start at frame 1 so the run covers exactly one period, phases 1..=200.
    sim.world.frame = 1;

    let mut planner_frames = Vec::new();
    let mut last = 0u64;
    for _ in 0..200 {
        sim.do_frame();
        let charged = sim.cover.gaps[Gap::LeaderPlanStrategy.index()];
        if charged != last {
            planner_frames.push(sim.world.frame - 1);
            last = charged;
        }
    }

    assert_eq!(
        sim.cover.gaps[Gap::LeaderPlanStrategy.index()],
        7,
        "one planning body plus six fast-lane openings per 200-frame period"
    );
    assert_eq!(planner_frames, vec![30, 60, 90, 120, 150, 180, 200]);
    assert_eq!(
        sim.cover.gaps[Gap::LeaderDiplomacy.index()],
        200,
        "diplomacy's gate passes every frame here, so every call is still charged"
    );
}

/// `phase % 30 != 0` is a full return, and step 11 says so rather than charging it.
#[test]
fn a_sub_phase_miss_executes_step_eleven_without_charging_the_planner() {
    let mut sim = one_active_leader(0x11d8);
    sim.world.frame = 6;

    let tick = sim.do_frame();

    assert_eq!(tick.steps[11], StepRun::Executed);
    assert_eq!(
        tick.work[11], 3,
        "check_explore, compute_score, diplomacy — the planner reached nothing"
    );
    assert_eq!(sim.cover.gaps[Gap::LeaderPlanStrategy.index()], 0);
    assert_eq!(
        plan_arm(&sim, 0),
        Some(PlanArm::NotDue { phase: 6 }),
        "0x006B9698"
    );
}

/// The semaphore that arms `Game::check_victory` is the same bit `Leader::diplomacy`
/// `0x006BC9AC` refuses on, so a tick that runs the victory tail runs no diplomacy at all.
#[test]
fn the_victory_semaphore_silences_every_diplomacy_body() {
    let mut sim = one_active_leader(0x11d9);
    sim.vic_match
        .set_sem(victory_score::game_sem::CHECK_VICTORY_MODE);
    sim.world.frame = 6;

    let tick = sim.do_frame();

    assert_eq!(tick.steps[11], StepRun::Executed);
    assert_eq!(
        sim.cover.gaps[Gap::LeaderDiplomacy.index()],
        0,
        "20,348 bytes that provably did not run"
    );
    assert_eq!(
        diplomacy_gate(&sim, 0),
        Some(DiplomacyGate::CheckVictoryMode)
    );
}

/// A human leader's diplomacy is refused before either global is read.
#[test]
fn a_human_leader_is_refused_by_the_diplomacy_gate() {
    let mut sim = one_active_leader(0x11da);
    sim.step8.leaders[0].flags |= diplo::LEADER_HUMAN;
    sim.world.frame = 6;

    sim.do_frame();

    assert_eq!(sim.cover.gaps[Gap::LeaderDiplomacy.index()], 0);
    assert_eq!(diplomacy_gate(&sim, 0), Some(DiplomacyGate::HumanOwner));
}

/// The global AI kill switch. `Game::action_cheat_ai_toggle` `0x005930C0` owns it in
/// retail and `command::InlineState::ai_off` owns it here; step 11 reads the mirror.
#[test]
fn the_global_ai_kill_switch_refuses_both_children() {
    let mut sim = one_active_leader(0x11db);
    sim.step8.ai_off = true;
    sim.step8.leaders[0].ai.production_step = 4;
    sim.world.frame = 6;

    sim.do_frame();

    assert_eq!(sim.cover.gaps[Gap::LeaderDiplomacy.index()], 0);
    assert_eq!(sim.cover.gaps[Gap::LeaderPlanStrategy.index()], 0);
    assert_eq!(diplomacy_gate(&sim, 0), Some(DiplomacyGate::AiOff));
    assert_eq!(
        sim.step8.leaders[0].ai.production_step, 0,
        "0x006C1B96 resets the counter on the way out"
    );
}

/// Retail's production cycle, advancing one stage per frame through the real tick.
///
/// This is the whole reason step 11 matters: `Leaders::strategy_all` `0x006ED452` is the
/// only call site of `Leader::plan_strategy`, which is the only call site of
/// `Leader::production_ai`, which is the only caller of the eight production-stage
/// functions. Nothing else in the image can start this machine.
#[test]
fn the_real_tick_walks_the_production_cycle_one_stage_per_frame() {
    let mut sim = one_active_leader(0x11dc);
    sim.step8.starting_resources = Some(0);
    sim.step8.leaders[0].ai.production_step = 2;
    sim.step8.leaders[0].ai.control = 6;
    sim.step8.leaders[0].ai.queued_units = Some(3);
    // `Leader::make_stuff` is a boundary; supply its return so step 8 can choose an arm.
    sim.step8.leaders[0].ai.make_stuff_result = Some(1);
    sim.world.frame = 1;

    let mut steps = Vec::new();
    let mut stages = Vec::new();
    for _ in 0..11 {
        sim.do_frame();
        steps.push(sim.step8.leaders[0].ai.production_step);
        if let Some(PlanArm::ProductionAi(trace)) = plan_arm(&sim, 0) {
            assert_eq!(trace.gate, ProductionGate::Entered);
            stages.extend(trace.stages);
        }
        if sim.step8.leaders[0].ai.production_step == 0 {
            break;
        }
    }

    assert_eq!(
        steps,
        vec![3, 4, 5, 6, 7, 8, 9, 10, 11, 0],
        "the 0x006C1BA8 jump table, one arm per frame"
    );
    assert_eq!(
        stages,
        vec![
            Stage::ProductionAiSetup,
            Stage::MakeListClear,
            Stage::FoundCities,
            Stage::ResearchTechs,
            Stage::UpgradeUnits,
            Stage::CreateUnits,
            Stage::CreateBuildings,
            Stage::MakeStuff,
            Stage::CreateUnits,
            Stage::CreateBuildings,
            Stage::MakeStuff,
        ]
    );
    assert_eq!(
        sim.step8.leaders[0].ai.effective_pop, 10,
        "queued_units() + control + 1 at 0x006C19A9"
    );
    assert_eq!(
        sim.cover.gaps[Gap::LeaderPlanStrategy.index()],
        10,
        "every entered arm reaches at least one unported stage body"
    );
}

/// The cycle bypasses the planner's phase entirely: it advances on frames the planner
/// would otherwise have returned from at `0x006B9698`.
#[test]
fn a_running_cycle_advances_on_a_sub_phase_miss_frame() {
    let mut sim = one_active_leader(0x11dd);
    sim.step8.starting_resources = Some(0);
    sim.step8.leaders[0].ai.production_step = 5;
    sim.step8.leaders[0].ai.queued_units = Some(0);
    sim.world.frame = 6; // phase 6, a sub-phase miss

    sim.do_frame();

    assert_eq!(sim.step8.leaders[0].ai.production_step, 6);
    assert!(matches!(plan_arm(&sim, 0), Some(PlanArm::ProductionAi(_))));
}

/// `leader_flags2 & 4` is `disable_production_ai`, straight off the shipped script API.
#[test]
fn the_scenario_production_ai_kill_switch_refuses_the_cycle() {
    let mut sim = one_active_leader(0x11de);
    sim.step8.starting_resources = Some(0);
    sim.step8.leaders[0].ai.production_step = 5;
    sim.step8.leaders[0].ai.flags2 = flags2::PRODUCTION_AI_DISABLED;
    sim.world.frame = 6;

    sim.do_frame();

    assert_eq!(sim.step8.leaders[0].ai.production_step, 0);
    assert_eq!(sim.cover.gaps[Gap::LeaderPlanStrategy.index()], 0);
    let Some(PlanArm::ProductionAi(trace)) = plan_arm(&sim, 0) else {
        panic!("a non-zero production_step delegates whatever the flags say");
    };
    assert_eq!(trace.gate, ProductionGate::ProductionAiDisabled);
}

/// A human leader runs the production cycle only with the companion bit set.
#[test]
fn the_human_production_gate_needs_both_bits() {
    for (extra, expect_step) in [(0u32, 0), (flags::PRODUCTION_DESPITE_HUMAN, 6)] {
        let mut sim = one_active_leader(0x11df);
        sim.step8.starting_resources = Some(0);
        sim.step8.leaders[0].flags |= flags::HUMAN | extra;
        sim.step8.leaders[0].ai.production_step = 5;
        sim.step8.leaders[0].ai.queued_units = Some(0);
        sim.world.frame = 6;

        sim.do_frame();

        assert_eq!(sim.step8.leaders[0].ai.production_step, expect_step);
    }
}

/// Infinite resources is `GameInfo::starting_resources == 8`, read as `byte [game+0x2D]`.
/// It skips the BHS script outright and adds `make_stuff` + `MakeList::clear` to the tail
/// of stages 3..7 and 9.
#[test]
fn infinite_resources_changes_which_stages_run() {
    let mut sim = one_active_leader(0x11e0);
    sim.step8.starting_resources = Some(INFINITE_RESOURCES);
    sim.step8.leaders[0].ai.production_step = 1;
    sim.step8.leaders[0].ai.prod_script_run = 1;
    sim.step8.leaders[0].ai.queued_units = Some(0);
    sim.world.frame = 6;

    sim.do_frame();
    assert_eq!(
        sim.step8.leaders[0].ai.production_step, 3,
        "0x006C19C5 promotes 1 to 2 and the step-2 arm then runs"
    );

    sim.do_frame();
    let Some(PlanArm::ProductionAi(trace)) = plan_arm(&sim, 0) else {
        panic!("still delegating");
    };
    assert_eq!(
        trace.stages,
        vec![Stage::FoundCities, Stage::MakeStuff, Stage::MakeListClear],
        "the 0x006C1AE4 tail"
    );
}

/// The BHS return codes, through the tick. 1 is `BLOCK_ON_THIS` and starves stages 2..11;
/// 3 is `SCRIPT_DONE` and retires the script for the rest of the match.
#[test]
fn the_script_return_codes_drive_the_cycle_through_the_tick() {
    let mut sim = one_active_leader(0x11e1);
    sim.step8.starting_resources = Some(0);
    sim.step8.leaders[0].ai.production_step = 1;
    sim.step8.leaders[0].ai.prod_script_run = 1;
    sim.step8.leaders[0].ai.queued_units = Some(0);
    sim.step8.leaders[0].ai.script_result = Some(SCRIPT_BLOCK_ON_THIS);
    sim.world.frame = 6;

    sim.do_frame();
    assert_eq!(sim.step8.leaders[0].ai.production_step, 0);
    assert_eq!(sim.step8.leaders[0].ai.prod_script_run, 1);

    sim.step8.leaders[0].ai.production_step = 1;
    sim.step8.leaders[0].ai.script_result = Some(SCRIPT_DONE);
    sim.do_frame();
    assert_eq!(sim.step8.leaders[0].ai.production_step, 2);
    assert_eq!(
        sim.step8.leaders[0].ai.prod_script_run, 0,
        "the one-way latch at 0x006C1A9F"
    );
}

/// Facts the host does not answer stall the machine instead of choosing a branch.
#[test]
fn an_unanswered_starting_resources_stalls_and_is_charged() {
    let mut sim = one_active_leader(0x11e2);
    assert_eq!(
        sim.step8.starting_resources, None,
        "don-sim has no producer for GameInfo::starting_resources at step 11 yet"
    );
    sim.step8.leaders[0].ai.production_step = 3;
    sim.step8.leaders[0].ai.queued_units = Some(0);
    sim.world.frame = 6;

    sim.do_frame();

    let Some(PlanArm::ProductionAi(trace)) = plan_arm(&sim, 0) else {
        panic!("still delegating");
    };
    assert_eq!(
        trace.stalls,
        vec![Stall::StartingResources { at_va: 0x006c_1ae9 }]
    );
    assert_eq!(sim.cover.gaps[Gap::LeaderPlanStrategy.index()], 1);
}

/// `AiEnv`'s default period is retail's, and the phase is not `check_explore`'s.
#[test]
fn the_two_step_eleven_children_are_on_different_phases() {
    let default = AiEnv::default();
    assert_eq!(default.ai_speed, 1);
    // `check_explore` is due for slot 0 at frame 188 (`+12` bias); the planner is not.
    let mut sim = one_active_leader(0x11e3);
    sim.map.world.seen2.fill(1);
    sim.world.frame = 188;

    sim.do_frame();

    assert_eq!(
        sim.step8.leaders[0].explored, sim.map.world.reg_size,
        "check_explore recounted on its phase"
    );
    assert_eq!(
        plan_arm(&sim, 0),
        Some(PlanArm::NotDue { phase: 188 }),
        "and the planner returned at 0x006B9698 on the same frame"
    );
}

/// Nothing about this runs when the leader is not in the loop at all.
#[test]
fn an_inactive_leader_reaches_neither_child() {
    let mut sim = Sim::new(0x11e4, 16);
    sim.step8.leaders[0].ai.production_step = 3;
    sim.world.frame = 6;

    let tick = sim.do_frame();

    assert_eq!(tick.steps[11], StepRun::Vacuous);
    assert_eq!(sim.step8.leaders[0].ai.production_step, 3);
    assert_eq!(plan_arm(&sim, 0), None);
    assert_eq!(diplomacy_gate(&sim, 0), None);
}

/// The exported retail geometry, so a rename cannot quietly move a boundary.
#[test]
fn the_named_boundaries_keep_their_retail_addresses() {
    use don_sim::systems::leader_production_ai::va;
    assert_eq!(va::PLAN_STRATEGY, 0x006b_9620);
    assert_eq!(va::PRODUCTION_AI, 0x006c_1960);
    assert_eq!(va::PRODUCTION_STEP_TABLE, 0x006c_1ba8);
    assert_eq!(diplo::LEADER_DIPLOMACY_VA, 0x006b_c950);
    assert_eq!(diplo::LEADER_HUMAN, flags::HUMAN);
    assert_eq!(
        leaders::EXPLORE_PHASE_BIAS,
        12,
        "check_explore biases its dividend and plan_strategy does not"
    );
}
