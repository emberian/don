use don_sim::systems::{leaders, leaders_process_event_frame_step19 as step19};
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
    assert_eq!(sim.cover.gaps[Gap::LeaderEventJukeBoxTail.index()], 0);
    assert_eq!(sim.cover.gaps[Gap::LeaderEventAchievementTail.index()], 0);
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
    assert_eq!(sim.cover.gaps[Gap::LeaderEventJukeBoxTail.index()], 0);
    assert_eq!(sim.cover.gaps[Gap::LeaderEventAchievementTail.index()], 0);
}

#[test]
fn real_tick_indexes_reciprocal_diplomacy_by_who_array_slot() {
    let mut sim = Sim::new(0x19e3, 16);
    sim.activate(0);
    sim.activate(3);
    // Keep step 11 from replacing the score facts before step 19.
    sim.step8.leaders[0].flags &= !leaders::flag::PROCESS;
    sim.step8.leaders[3].flags &= !leaders::flag::PROCESS;

    sim.step8.end.local_who = 1;
    sim.step8.event.current_music_mood = leaders::combat_mood::QUIET;
    sim.step8.leaders[3].slot = 1;
    sim.step8.leaders[3].event_frame.hits_current_frame = 12;
    sim.step8.leaders[3].event_frame.damage_current_frame = 16;

    // Record 0's first relation is non-hostile. Retail therefore reads the reciprocal from
    // leaders[local_who == 1], not by searching for the current record (record 3).
    sim.step8.leaders[0].diplo[1] = 1;
    sim.step8.leaders[0].event_frame.hits_current_frame = 1;
    sim.step8.leaders[1].diplo[0] = 1;
    sim.step8.leaders[3].diplo[0] = 0;
    sim.vic_leaders.slots[0].score = 1_000;
    sim.vic_leaders.slots[3].score = 100;

    let tick = sim.do_frame();

    assert_eq!(tick.steps[19], StepRun::Executed);
    assert_eq!(
        sim.step8.event.next_music_mood,
        leaders::combat_mood::WINNING
    );
    assert_eq!(sim.step8.event.last_mood_requests.len(), 1);
    let team_reads: Vec<_> = sim
        .last_event_frame_trace
        .exact
        .product_reads
        .iter()
        .filter_map(|receipt| match receipt.read {
            step19::ProductRead::TeamScore {
                owner_leader_index,
                queried_leader_index,
                ..
            } => Some((owner_leader_index, queried_leader_index)),
            step19::ProductRead::EncryptedAge { .. } => None,
        })
        .collect();
    assert_eq!(team_reads, vec![(3, 3)]);
    assert_eq!(sim.cover.gaps[Gap::LeaderEventJukeBoxTail.index()], 1);
    assert_eq!(sim.cover.gaps[Gap::LeaderEventAchievementTail.index()], 0);
}

#[test]
fn real_tick_keeps_achievement_as_an_unresolved_typed_tail() {
    let mut sim = Sim::new(0x19e4, 16);
    sim.activate(0);
    sim.step8.leaders[0].flags &= !leaders::flag::PROCESS;
    sim.step8.leaders[0].event_frame.deaths_current_frame = 4;
    sim.step8.leaders[0].event_frame.kills_current_frame = 2;

    sim.do_frame();

    assert_eq!(sim.step8.event.last_achievement_events.len(), 1);
    assert_eq!(sim.cover.gaps[Gap::LeaderEventAchievementTail.index()], 1);
    let tail = sim
        .last_event_frame_trace
        .exact
        .host_tails
        .iter()
        .find(|tail| matches!(tail.host_tail, step19::HostTail::AchieveAddEvent { .. }))
        .expect("typed achievement tail");
    let sentinel = sim
        .last_event_frame_trace
        .exact
        .local_mutations
        .iter()
        .find(|receipt| {
            matches!(
                receipt.mutation,
                step19::LocalMutation::BattleRateSentinels { .. }
            )
        })
        .expect("sentinel mutation");
    assert!(tail.sequence < sentinel.sequence);
    assert_eq!(
        sim.step8.leaders[0].event_frame.average_death_rate,
        step19::BATTLE_RATE_SENTINEL
    );
    assert_eq!(
        sim.step8.leaders[0].event_frame.average_kill_rate,
        step19::BATTLE_RATE_SENTINEL
    );
}
