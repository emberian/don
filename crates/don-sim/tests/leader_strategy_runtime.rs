use don_sim::objects::{Band, ObjectRegistry};
use don_sim::systems::leader_production_ai::strategy_runtime::*;
use don_sim::systems::leader_production_ai::{PlanArm, Stage};
use don_sim::systems::leaders::{self, ExploreWorld, StrategyCall, StrategyInputs};
use don_sim::systems::player_setup::ManualPlayerSetup;
use don_sim::systems::production::runtime::{LiveProductionRuntime, LiveProductionType};
use don_sim::systems::production::{self, BuildData, BuildQueue, BuildQueueEntry};
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::tick::Sim;

fn queue_entry(type_index: i16) -> BuildQueueEntry {
    BuildQueueEntry {
        type_index,
        ..BuildQueueEntry::default()
    }
}

fn active_build(owner: u8, queued: &[i16]) -> BuildData {
    let mut build = BuildData {
        flags: production::flag::VALID | production::flag::STARTED | production::flag::ACTIVE,
        who: owner,
        queue: BuildQueue {
            queued: queued.len() as u8,
            entries: queued.iter().copied().map(queue_entry).collect(),
        },
        ..BuildData::default()
    };
    build.gather_down = -1;
    build.city = -1;
    build.city_down = -1;
    build.wonder = -1;
    build.dock = -1;
    build.attack_ox = -1;
    build.attack_whom = -1;
    build.other[0x28..0x2a].copy_from_slice(&(-1i16).to_le_bytes());
    build
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

#[test]
fn canonical_ai_projection_preserves_host_answers_and_roundtrips_v14_wire_order() {
    let canonical = CanonicalProductionAi {
        leader_flags2: 0x1020_3040,
        production_step: 3,
        prod_script_run: 1,
        script_step: 27,
        control: 88,
        effective_pop: 144,
    };
    let mut view = don_sim::systems::leader_production_ai::ProductionState {
        pers_arg: Some(-7),
        queued_units: Some(55),
        make_list_head: Some(0x220),
        script_result: Some(3),
        make_stuff_result: Some(1),
        ..Default::default()
    };
    canonical.project_into(&mut view);
    assert_eq!(CanonicalProductionAi::capture(&view), canonical);
    assert_eq!(view.pers_arg, Some(-7));
    assert_eq!(view.queued_units, Some(55));
    assert_eq!(view.make_list_head, Some(0x220));
    assert_eq!(view.script_result, Some(3));
    assert_eq!(view.make_stuff_result, Some(1));

    let expected = [
        3i32.to_le_bytes(),
        1i32.to_le_bytes(),
        27i32.to_le_bytes(),
        88i32.to_le_bytes(),
        144i32.to_le_bytes(),
    ]
    .concat();
    assert_eq!(canonical.extension_bytes().as_slice(), expected);
    assert_eq!(
        CanonicalProductionAi::from_extension_bytes(
            canonical.leader_flags2,
            &canonical.extension_bytes()
        )
        .unwrap(),
        canonical
    );
}

#[test]
fn v13_defaults_only_the_new_five_fields_and_v14_requires_them() {
    let legacy = CanonicalProductionAi::for_save_version(13, 0x55aa, None).unwrap();
    assert_eq!(legacy.leader_flags2, 0x55aa);
    assert_eq!(legacy.extension_values(), [0; 5]);
    assert_eq!(
        CanonicalProductionAi::for_save_version(13, 1, Some([0; 5])),
        Err(CanonicalAiCodecError::UnexpectedLegacyExtension { format_version: 13 })
    );
    assert_eq!(
        CanonicalProductionAi::for_save_version(14, 1, None),
        Err(CanonicalAiCodecError::MissingV14Extension { format_version: 14 })
    );

    let values = [2, 3, 4, 5, 6];
    assert_eq!(
        CanonicalProductionAi::for_save_version(14, 1, Some(values)).unwrap(),
        CanonicalProductionAi::from_extension_values(1, values)
    );
}

#[test]
fn ordinary_tick_projects_and_commits_four_canonical_ai_rows_across_seeds() {
    for seed in [1, 0x5eed, 0xdead_beef, u32::MAX as u64] {
        let mut sim = Sim::new(seed, 8);
        sim.world.frame = 7;
        sim.vic_match.frame = 7;
        for owner in 0..4usize {
            sim.activate(owner);
            let type_index = 70 + owner as i32;
            sim.production_runtime
                .install_type(LiveProductionType::ordinary_unit(
                    type_index,
                    100,
                    owner as i32 + 2,
                ));
            sim.spawn_build(owner, active_build(owner as u8, &[type_index as i16; 2]));

            let canonical = &mut sim.vic_leaders.slots[owner];
            canonical.production_step = 2;
            canonical.control = owner as i32;
        }

        sim.do_frame();

        let receipt = sim
            .last_strategy_receipt
            .as_ref()
            .expect("step 11 transaction must commit");
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
            let queued = 2 * (owner as i32 + 2);
            let canonical = &sim.vic_leaders.slots[owner];
            assert_eq!(canonical.production_step, 3);
            assert_eq!(canonical.control, owner as i32);
            assert_eq!(canonical.effective_pop, queued + owner as i32 + 1);
            assert_eq!(
                CanonicalProductionAi::capture(&sim.step8.leaders[owner].ai),
                CanonicalProductionAi {
                    leader_flags2: canonical.leader_flags2,
                    production_step: canonical.production_step,
                    prod_script_run: canonical.prod_script_run,
                    script_step: canonical.script_step,
                    control: canonical.control,
                    effective_pop: canonical.effective_pop,
                }
            );
        }
    }
}

fn install_four_seat_unit_types(sim: &mut Sim) {
    for owner in 0..4usize {
        sim.production_runtime
            .install_type(LiveProductionType::ordinary_unit(
                70 + owner as i32,
                100,
                owner as i32 + 2,
            ));
    }
}

fn canonical_ai_rows(sim: &Sim) -> [CanonicalProductionAi; 4] {
    std::array::from_fn(|owner| {
        let row = &sim.vic_leaders.slots[owner];
        CanonicalProductionAi {
            leader_flags2: row.leader_flags2,
            production_step: row.production_step,
            prod_script_run: row.prod_script_run,
            script_step: row.script_step,
            control: row.control,
            effective_pop: row.effective_pop,
        }
    })
}

#[test]
fn v14_save_resume_matches_the_next_natural_four_seat_tick_across_seeds() {
    for seed in [2, 0x5eed, 0x1234_5678] {
        let mut original = Sim::new(seed, 8);
        let mut setup = ManualPlayerSetup {
            active_mask: 0x0f,
            team_style: 1,
            local_player_setup_slot: 0,
            ..ManualPlayerSetup::default()
        };
        setup.teams[..4].copy_from_slice(&[0, 1, 0, 1]);
        original.start_manual_player_setup(setup).unwrap();
        original.world.frame = 7;
        original.vic_match.frame = 7;
        install_four_seat_unit_types(&mut original);
        for owner in 0..4usize {
            let type_index = 70 + owner as i32;
            let row =
                original.spawn_build(owner, active_build(owner as u8, &[type_index as i16; 2]));
            let object_id = (BUILD_BAND_BASE
                + original.world.objects.slot(owner).band(Band::Build).len()
                - 1) as i16;
            original.builds[row].other[production::off::OBJECT_ID..production::off::OBJECT_ID + 2]
                .copy_from_slice(&object_id.to_le_bytes());
            original.vic_leaders.slots[owner].production_step = 2;
            original.vic_leaders.slots[owner].control = owner as i32;
        }

        original.do_frame();
        let first_rows = canonical_ai_rows(&original);
        let bytes = save_sim(&original).expect("current format must own production-AI progress");
        assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 20);
        let mut resumed = load_sim(&bytes).expect("current production-AI rows must load");
        assert_eq!(canonical_ai_rows(&resumed), first_rows);
        assert_eq!(resumed.channel_digest(), original.channel_digest());

        // Live type facts are an installed content authority, not a second save owner.
        install_four_seat_unit_types(&mut resumed);
        original.do_frame();
        resumed.do_frame();

        assert_eq!(canonical_ai_rows(&resumed), canonical_ai_rows(&original));
        assert_eq!(resumed.channel_digest(), original.channel_digest());
        assert_eq!(
            resumed.last_strategy_receipt.as_ref().unwrap().after,
            original.last_strategy_receipt.as_ref().unwrap().after
        );
        assert_eq!(
            resumed.last_strategy_receipt.as_ref().unwrap().trace,
            original.last_strategy_receipt.as_ref().unwrap().trace
        );
    }
}
