use don_sim::objects::{Band, ObjectRegistry};
use don_sim::systems::leader_production_ai::strategy_runtime::*;
use don_sim::systems::leader_production_ai::{PlanArm, Stage};
use don_sim::systems::leaders::{self, ExploreWorld, StrategyCall, StrategyInputs};
use don_sim::systems::production::runtime::{LiveProductionRuntime, LiveProductionType};
use don_sim::systems::production::{self, BuildData, BuildQueue, BuildQueueEntry};

fn queue_entry(type_index: i16) -> BuildQueueEntry {
    BuildQueueEntry {
        type_index,
        ..BuildQueueEntry::default()
    }
}

fn active_build(owner: u8, queued: &[i16]) -> BuildData {
    BuildData {
        flags: production::flag::VALID | production::flag::STARTED | production::flag::ACTIVE,
        who: owner,
        queue: BuildQueue {
            queued: queued.len() as u8,
            entries: queued.iter().copied().map(queue_entry).collect(),
        },
        ..BuildData::default()
    }
}

fn inputs(frame: i32, seen2: &[u8]) -> StrategyCanonicalInputs<'_> {
    StrategyCanonicalInputs {
        dispatcher: StrategyInputs {
            frame,
            ai_speed: 1,
            world: ExploreWorld {
                reg_xs: 1,
                reg_ys: 1,
                reg_size: 1,
                fog_xs: 8,
                seen2,
            },
            has_explore_preq: [Some(false); leaders::NUM_LEADER_SLOTS],
            check_victory_mode: false,
        },
        ai_off: false,
        starting_resources: 0,
    }
}

#[test]
fn queued_units_walks_the_canonical_build_band_and_exact_unit_gate() {
    let mut registry = ObjectRegistry::new();
    let mut builds = vec![
        active_build(0, &[60, 61, 62, 63, 64]),
        active_build(0, &[60]),
        active_build(1, &[60]),
    ];
    // The second row is valid but unfinished: retail's second virtual predicate refuses it.
    builds[1].flags &= !production::flag::ACTIVE;
    registry.insert(0, Band::Build, 0);
    registry.insert(0, Band::Build, 1);
    registry.insert(1, Band::Build, 2);

    let mut runtime = LiveProductionRuntime::default();
    runtime.install_type(LiveProductionType::ordinary_unit(60, 100, 3));
    let mut unavailable = LiveProductionType::ordinary_unit(61, 100, 99);
    unavailable.can_make = false;
    runtime.install_type(unavailable);
    runtime.install_type(LiveProductionType::research(62, 100));
    let mut locked = LiveProductionType::ordinary_unit(63, 100, 77);
    locked.prerequisites = vec![500];
    runtime.install_type(locked);
    let mut unlocked = LiveProductionType::ordinary_unit(64, 100, 5);
    unlocked.prerequisites = vec![501];
    runtime.install_type(unlocked);
    runtime.leaders[0].tech.tech.set(501, true);

    let receipt = queued_units(&registry, &builds, &runtime, 0).unwrap();
    assert_eq!(receipt.builds_visited, vec![2000, 2001]);
    assert_eq!(receipt.active_builds, 1);
    assert_eq!(receipt.logical_entries, 5);
    assert_eq!(receipt.total_control_cost, 8);
    assert_eq!(receipt.admitted.len(), 2);
    assert_eq!(receipt.admitted[0].type_index, 60);
    assert_eq!(receipt.admitted[1].type_index, 64);
}

#[test]
fn preflight_failure_is_atomic() {
    let mut registry = ObjectRegistry::new();
    let builds = vec![active_build(0, &[77])];
    registry.insert(0, Band::Build, 0);
    let runtime = LiveProductionRuntime::default();
    let mut leaders = leaders::Leaders::new();
    leaders.leaders[0].activate();
    leaders.leaders[0].ai.production_step = 2;
    let before = StrategySnapshot::capture(&leaders);
    let seen2 = vec![1; 64];

    assert!(matches!(
        execute_strategy_all(
            &mut leaders,
            &registry,
            &builds,
            &runtime,
            inputs(7, &seen2)
        ),
        Err(StrategyRuntimeError::MissingType { type_index: 77, .. })
    ));
    assert_eq!(StrategySnapshot::capture(&leaders), before);
}

#[test]
fn dispatcher_executes_queued_units_and_preserves_retail_call_order() {
    let mut registry = ObjectRegistry::new();
    let builds = vec![active_build(0, &[60, 60])];
    registry.insert(0, Band::Build, 0);
    let mut runtime = LiveProductionRuntime::default();
    runtime.install_type(LiveProductionType::ordinary_unit(60, 100, 4));

    let mut leaders = leaders::Leaders::new();
    leaders.leaders[0].activate();
    leaders.leaders[0].ai.production_step = 2;
    leaders.leaders[0].ai.control = 6;
    let seen2 = vec![1; 64];
    let receipt = execute_strategy_all(
        &mut leaders,
        &registry,
        &builds,
        &runtime,
        inputs(7, &seen2),
    )
    .unwrap();

    assert_eq!(receipt.dispatcher_va, STRATEGY_ALL_VA);
    assert_eq!(
        receipt.queued_units[0].as_ref().unwrap().total_control_cost,
        8
    );
    assert_eq!(leaders.leaders[0].ai.effective_pop, 15);
    assert_eq!(leaders.leaders[0].ai.production_step, 3);
    assert_ne!(receipt.before_adler32, receipt.after_adler32);
    assert_eq!(
        receipt.trace.calls,
        vec![
            StrategyCall::CheckExplore {
                slot: 0,
                update: leaders::ExploreUpdate::NotDue,
            },
            StrategyCall::PlanStrategy(0),
            StrategyCall::ComputeScore { slot: 0, force: 0 },
            StrategyCall::Diplomacy(0),
        ]
    );
    let plan = receipt.trace.plan[0].as_ref().unwrap();
    let PlanArm::ProductionAi(production) = &plan.arm else {
        panic!("running cycle must enter production_ai")
    };
    assert_eq!(production.effective_pop, Some(15));
    assert_eq!(
        production.stages,
        vec![Stage::ProductionAiSetup, Stage::MakeListClear]
    );
    assert!(production.stalls.is_empty());
    assert_eq!(
        call_stage_map(&receipt.trace),
        StrategyStageMap {
            check_explore: 1,
            plan_strategy: 1,
            compute_score: 1,
            diplomacy: 1,
            check_victory: false,
        }
    );
    assert_eq!(
        leaders.leaders[0].ai.queued_units, None,
        "host answer restored"
    );
    assert_eq!(leaders.ai_off, false);
    assert_eq!(leaders.starting_resources, None);
}

#[test]
fn snapshot_resume_matches_uninterrupted_two_stage_execution() {
    let registry = ObjectRegistry::new();
    let builds = Vec::new();
    let runtime = LiveProductionRuntime::default();
    let seen2 = vec![1; 64];

    let mut uninterrupted = leaders::Leaders::new();
    uninterrupted.leaders[0].activate();
    uninterrupted.leaders[0].ai.production_step = 3;
    uninterrupted.leaders[0].ai.control = 11;
    let first = execute_strategy_all(
        &mut uninterrupted,
        &registry,
        &builds,
        &runtime,
        inputs(7, &seen2),
    )
    .unwrap();
    assert_eq!(uninterrupted.leaders[0].ai.production_step, 4);

    let wire = first.after.encode();
    let decoded = StrategySnapshot::decode(&wire).unwrap();
    assert_eq!(decoded, first.after);
    let mut resumed = leaders::Leaders::new();
    decoded.restore(&mut resumed);

    let uninterrupted_second = execute_strategy_all(
        &mut uninterrupted,
        &registry,
        &builds,
        &runtime,
        inputs(8, &seen2),
    )
    .unwrap();
    let resumed_second = execute_strategy_all(
        &mut resumed,
        &registry,
        &builds,
        &runtime,
        inputs(8, &seen2),
    )
    .unwrap();

    assert_eq!(resumed_second.before, first.after);
    assert_eq!(resumed_second.after, uninterrupted_second.after);
    assert_eq!(resumed_second.trace, uninterrupted_second.trace);
    assert_eq!(
        resumed_second.after_adler32,
        uninterrupted_second.after_adler32
    );
}

#[test]
fn canonical_ai_off_refusal_does_not_require_queue_type_facts() {
    let mut registry = ObjectRegistry::new();
    let builds = vec![active_build(0, &[77])];
    registry.insert(0, Band::Build, 0);
    let runtime = LiveProductionRuntime::default();
    let mut leaders = leaders::Leaders::new();
    leaders.leaders[0].activate();
    leaders.leaders[0].ai.production_step = 5;
    let seen2 = vec![1; 64];
    let mut input = inputs(7, &seen2);
    input.ai_off = true;

    let receipt = execute_strategy_all(&mut leaders, &registry, &builds, &runtime, input).unwrap();
    assert!(receipt.queued_units[0].is_none());
    assert_eq!(leaders.leaders[0].ai.production_step, 0);
    assert_eq!(leaders.leaders[0].ai.effective_pop, 0);
}

#[test]
fn four_seats_join_their_own_build_bands_and_keep_dispatch_order() {
    let mut registry = ObjectRegistry::new();
    let mut builds = Vec::new();
    let mut runtime = LiveProductionRuntime::default();
    let mut leaders = leaders::Leaders::new();
    for owner in 0..4usize {
        let type_index = 70 + owner as i32;
        runtime.install_type(LiveProductionType::ordinary_unit(
            type_index,
            100,
            owner as i32 + 2,
        ));
        let row = builds.len();
        builds.push(active_build(owner as u8, &[type_index as i16; 2]));
        registry.insert(owner, Band::Build, row as u32);
        leaders.leaders[owner].activate();
        leaders.leaders[owner].ai.production_step = 2;
        leaders.leaders[owner].ai.control = owner as i32;
    }
    let seen2 = vec![0b0000_1111; 64];
    let receipt = execute_strategy_all(
        &mut leaders,
        &registry,
        &builds,
        &runtime,
        inputs(7, &seen2),
    )
    .unwrap();

    for owner in 0..4usize {
        let queued = 2 * (owner as i32 + 2);
        assert_eq!(
            receipt.queued_units[owner]
                .as_ref()
                .unwrap()
                .total_control_cost,
            queued
        );
        assert_eq!(
            leaders.leaders[owner].ai.effective_pop,
            queued + owner as i32 + 1
        );
    }
    assert_eq!(
        call_stage_map(&receipt.trace),
        StrategyStageMap {
            check_explore: 4,
            plan_strategy: 4,
            compute_score: 4,
            diplomacy: 4,
            check_victory: false,
        }
    );
    for owner in 0..4usize {
        let calls = &receipt.trace.calls[owner * 4..owner * 4 + 4];
        assert!(matches!(calls[0], StrategyCall::CheckExplore { slot, .. } if slot == owner));
        assert_eq!(calls[1], StrategyCall::PlanStrategy(owner));
        assert_eq!(
            calls[2],
            StrategyCall::ComputeScore {
                slot: owner,
                force: 0
            }
        );
        assert_eq!(calls[3], StrategyCall::Diplomacy(owner));
    }
}

#[test]
fn snapshot_codec_rejects_wrong_length_magic_and_version() {
    let snapshot = StrategySnapshot::capture(&leaders::Leaders::new());
    let wire = snapshot.encode();
    assert!(matches!(
        StrategySnapshot::decode(&wire[..wire.len() - 1]),
        Err(StrategySnapshotError::Length { .. })
    ));

    let mut bad_magic = wire.clone();
    bad_magic[0] ^= 1;
    assert_eq!(
        StrategySnapshot::decode(&bad_magic),
        Err(StrategySnapshotError::Magic)
    );

    let mut bad_version = wire;
    bad_version[8..12].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        StrategySnapshot::decode(&bad_version),
        Err(StrategySnapshotError::Version(2))
    );
}
