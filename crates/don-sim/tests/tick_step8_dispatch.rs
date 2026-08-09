use don_sim::systems::{leaders, production, walls};
use don_sim::tick::{Gap, Sim, StepRun};

#[test]
fn real_tick_executes_the_recovered_step8_dispatcher() {
    let mut sim = Sim::new(0x8eed, 16);
    sim.activate(0);
    sim.activate(1);

    // Slot 1 is live and counts as a threat. With no mutual alliance, retail raises
    // HOSTILE_SEEN for slot 0 during the inner diplomacy scan.
    sim.step8.leaders[1].flags |= leaders::flag::COUNTS_AS_HOSTILE;

    // `GatherInputs::rares` mirrors Leader+0x6DCC. Changing it arms both stat passes in
    // this same frame through Leader::gather's exact BitMask<44> union.
    sim.leaders[0].gather_inputs.rares[3] = true;
    sim.step8.leaders[0].timers[0] = leaders::GraceTimer {
        value: -2,
        frozen: 0,
    };

    sim.spawn_unit(0, 1, 1000, 1000, 2).unwrap();
    sim.spawn_build(
        0,
        production::BuildData {
            flags: production::flag::VALID,
            ..Default::default()
        },
    );
    sim.spawn_wall(
        0,
        walls::WallState {
            flags: walls::FLAG_ALIVE,
            ..Default::default()
        },
    );

    // Supply the unresolved object-virtual answers. The tick adapter preserves these
    // answers while replacing active bits from the real owner bands.
    sim.step8_env.leaders[0].objects.units = vec![leaders::StatObject {
        stats_stale: true,
        ..Default::default()
    }];
    sim.step8_env.leaders[0].objects.band_2000 = vec![leaders::StatObject {
        construct_time_resolved: true,
        ..Default::default()
    }];
    sim.step8_env.leaders[0].objects.band_3000 = vec![leaders::StatObject {
        construct_time_resolved: true,
        ..Default::default()
    }];

    // A zero-stamped taunt table must stay quiet on frame zero.
    sim.step8.leaders[0].taunt_frame[0] = 0;
    let tick = sim.do_frame();

    assert_eq!(tick.steps[8], StepRun::Executed);
    assert_eq!(tick.work[8], 2);
    assert_ne!(sim.step8.leaders[0].flags & leaders::flag::HOSTILE_SEEN, 0);
    assert!(sim.step8.leaders[0].rare_effective.get(3));
    assert_eq!(sim.step8.leaders[0].timers[0].value, -1);
    assert_eq!(sim.cover.leader_gathers, 2);
    assert_eq!(sim.cover.leader_hostile_frames, 1);
    assert_eq!(sim.cover.leader_wall_stat_passes, 1);
    assert_eq!(sim.cover.leader_unit_stat_passes, 1);
    assert_eq!(sim.cover.leader_stat_objects_visited, 3);
    assert_eq!(sim.cover.leader_timer_creeps, 1);
    assert_eq!(sim.cover.leader_taunt_dispatches, 0);

    // The exact taunt-table scan reads the pre-increment frame. Step 20 made it 1.
    sim.step8.leaders[0].taunt_frame[2] = 1;
    sim.step8.leaders[0].taunt_kind[2] = 11;
    sim.step8.leaders[0].taunt_arg[2] = 22;
    sim.do_frame();
    assert_eq!(sim.cover.leader_taunt_dispatches, 1);
    assert_eq!(sim.cover.gaps[Gap::LeaderProcessTaunt.index()], 1);
}

#[test]
fn step8_outer_process_gate_is_not_the_in_game_bit() {
    let mut skipped = Sim::new(1, 8);
    skipped.activate(0);
    skipped.step8.leaders[0].flags &= !leaders::flag::PROCESS;
    let tick = skipped.do_frame();
    assert_eq!(tick.steps[8], StepRun::Vacuous);
    assert_eq!(skipped.cover.leader_gathers, 0);

    let mut processed = Sim::new(1, 8);
    // Retail permits exactly this distinction: bit 1 enters the outer loop while bit 0
    // controls whether this leader participates in other leaders' diplomacy scans.
    processed.step8.leaders[0].flags = leaders::flag::PROCESS;
    let tick = processed.do_frame();
    assert_eq!(tick.steps[8], StepRun::Executed);
    assert_eq!(tick.work[8], 1);
    assert_eq!(processed.cover.leader_gathers, 1);
}
