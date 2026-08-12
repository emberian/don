//! Atomic replay/BHS proof for the builtin-357 single-Library bridge.

use std::path::{Path, PathBuf};

use don_bhs::{ScriptTimers, VmError};
use don_replay::replay::Replay;
use don_replay::replay_bhs_live_bindings::{
    bind_production_map_style, bind_production_type_counts, ProductionBuiltinImage,
    ProductionBuiltinValue, ProductionCityImage, ProductionLeaderImage, ProductionRunFailure,
    ProductionSetupImage, ReplayProductionCall,
};
use don_replay::replay_bhs_research_runtime::run_production_research_call;
use don_replay::replay_bhs_runtime::{
    load_replay_bhs_program, ReplayBhsBinding, LEADER_FLAG_HUMAN,
};
use don_sim::objects::BUILD_BAND_BASE;
use don_sim::script_runtime::{ExternalGameSeconds, ScriptRuntime};
use don_sim::systems::bhs_create_unit_runtime::{
    BhsCreateUnitRuntime, CreateUnitLeaderProjection, CreateUnitProjectionWitness,
    CreateUnitRuntimeInput,
};
use don_sim::systems::bhs_type_factory::{
    produce_type_builtin_state, ComposedTypeRow, RulesCompositionId, Sha256Digest,
    TypeBuiltinFactoryInput, TypeSourceRole, TypeSourceWitness, WitnessedTypeSource,
};
use don_sim::systems::bhs_type_table::{
    LeaderTypeMasks, TypeBuiltinState, TypeRow, NUM_LEADERS, NUM_TRIBES, NUM_TYPES,
};
use don_sim::systems::production::runtime::{
    LiveProductionRuntime, LiveProductionType, SingleLibraryResearchStatus,
};
use don_sim::systems::production::{flag, off, BuildData, BuildQueue, BuildQueueEntry};
use don_sim::tick::Sim;

const OWNER: usize = 0;
const LIBRARY: i32 = 435;
const WRITTEN_WORD: i32 = 551;
const CITY_STATE: i32 = 565;
const COST: [i32; 6] = [0, 12, 5, 0, 0, 0];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn digest(byte: u8) -> Sha256Digest {
    Sha256Digest([byte; 32])
}

fn witness(role: TypeSourceRole, component: u8) -> TypeSourceWitness {
    TypeSourceWitness {
        role,
        composition: RulesCompositionId(digest(1)),
        manifest_sha256: digest(2),
        component_sha256: digest(component),
    }
}

fn canonical_type_owners(owner: usize) -> (TypeBuiltinState, BhsCreateUnitRuntime) {
    let rows = (0..NUM_TYPES)
        .map(|slot| {
            let mut row = TypeRow::empty(slot);
            row.name = match slot {
                0 => "Food".into(),
                2 => "Wealth".into(),
                50 => "Citizen".into(),
                51 => "Citizens".into(),
                52 => "Upgraded Citizen".into(),
                53 => "Grafted Citizen".into(),
                417 => "Farm".into(),
                418 => "Woodcutter's Camp".into(),
                419 => "Mine".into(),
                420 => "University".into(),
                427 => "Barracks".into(),
                432 => "Dock".into(),
                435 => "Library".into(),
                436 => "Market".into(),
                439 => "Tower".into(),
                551 => "Written Word".into(),
                565 => "City State".into(),
                572 => "The Art of War".into(),
                _ => format!("Internal {slot}"),
            };
            row.type_name = format!("Family {slot}");
            if slot == WRITTEN_WORD as usize {
                row.common.costs = COST;
                row.common.job_time = 200;
                row.common.preq = [-1; 3];
                row.where_type = LIBRARY;
            }
            if slot == CITY_STATE as usize {
                row.common.costs = [12, 0, 0, 0, 0, 0];
                row.common.job_time = 200;
                row.common.preq = [-1; 3];
                row.where_type = LIBRARY;
            }
            Some(ComposedTypeRow {
                index: row.index,
                name: row.name,
                type_name: row.type_name,
                common: row.common,
                from: row.from,
                where_type: row.where_type,
                modified: row.modified,
                grid_x: row.grid_x,
                grid_y: row.grid_y,
                is_non_strict: Some(row.is_list),
                body: row.body,
            })
        })
        .collect();
    let produced = produce_type_builtin_state(TypeBuiltinFactoryInput {
        types: WitnessedTypeSource {
            witness: witness(TypeSourceRole::TypeRows, 3),
            value: rows,
        },
        tribes: WitnessedTypeSource {
            witness: witness(TypeSourceRole::TribeRoster, 4),
            value: (0..NUM_TRIBES)
                .map(|slot| Some(format!("Tribe {slot}")))
                .collect(),
        },
        leaders: WitnessedTypeSource {
            witness: witness(TypeSourceRole::LeaderMasks, 5),
            value: (0..NUM_LEADERS)
                .map(|slot| {
                    Some(LeaderTypeMasks {
                        leader_flags: if slot == owner { 3 } else { 0 },
                        ..LeaderTypeMasks::default()
                    })
                })
                .collect(),
        },
    })
    .unwrap();
    let (state, provenance) = produced.into_parts();
    let upgrades = BhsCreateUnitRuntime::new(
        CreateUnitRuntimeInput {
            witness: CreateUnitProjectionWitness {
                composition: RulesCompositionId(digest(1)),
                manifest_sha256: digest(2),
                component_sha256: digest(6),
            },
            types: vec![None; NUM_TYPES],
            leaders: (0..NUM_LEADERS)
                .map(|leader_slot| {
                    Some(CreateUnitLeaderProjection {
                        leader_slot,
                        current_upgrade: (0..NUM_TYPES)
                            .map(|slot| {
                                Some(if leader_slot == owner && slot == 50 {
                                    52
                                } else {
                                    slot as i32
                                })
                            })
                            .collect(),
                        graft: (0..NUM_TYPES)
                            .map(|slot| {
                                Some(if leader_slot == owner && slot == 52 {
                                    53
                                } else {
                                    slot as i32
                                })
                            })
                            .collect(),
                    })
                })
                .collect(),
            numeric_groups: Vec::new(),
        },
        &state,
        provenance,
    )
    .unwrap();
    (state, upgrades)
}

fn production_owners(owner: usize) -> (Sim, LiveProductionRuntime, usize) {
    let mut sim = Sim::new(0x357, 8);
    let mut build = BuildData {
        flags: flag::VALID | flag::ACTIVE,
        who: owner as u8,
        city: 0,
        city_down: -1,
        queue: BuildQueue {
            queued: 0,
            entries: vec![BuildQueueEntry::default(); 2],
        },
        ..BuildData::default()
    };
    build.other[off::OBJECT_ID..off::OBJECT_ID + 2]
        .copy_from_slice(&(BUILD_BAND_BASE as i16).to_le_bytes());
    let row = sim.spawn_build(owner, build);
    sim.cities.city_mark[owner] = 1;
    let city = &mut sim.cities.slots[owner][0];
    city.city_flags = 1;
    city.city = 0;
    city.o = BUILD_BAND_BASE as i16;
    city.who = owner as i8;
    city.name = "Athens".into();
    city.id = "capital_0".into();

    let mut production = std::mem::take(&mut sim.production_runtime);
    production.register_build(row, LIBRARY);
    let mut library = LiveProductionType::in_place_building(LIBRARY, 1);
    library.is_library = true;
    production.install_type(library);
    let mut written_word = LiveProductionType::research(WRITTEN_WORD, 200);
    written_word.repeat_cost = Some(COST);
    production.install_type(written_word);
    let mut city_state = LiveProductionType::research(CITY_STATE, 200);
    city_state.repeat_cost = Some([12, 0, 0, 0, 0, 0]);
    production.install_type(city_state);
    production.leaders[owner].resources = [100; 6];
    sim.leaders[owner].econ.stockpile = [100; 6];
    sim.step8.leaders[owner].econ.stockpile = [100; 6];
    sim.vic_leaders.slots[owner].leader_flags = 3;
    sim.vic_leaders.slots[owner].economy.bucket = [100; 6];
    sim.step8.leaders[owner].flags = 3;
    sim.vic_leaders.slots[owner].num_units[0] = 3;
    sim.vic_leaders.slots[owner].num_units[53 - 50] = 7;
    sim.vic_leaders.slots[owner].num_buildings[427 - 414] = 4;
    sim.vic_leaders.slots[owner].num_queued[53] = 5;
    sim.vic_leaders.slots[owner].num_queued[427] = 6;
    sim.vic_leaders.slots[owner].num_queued[0] = 9;
    sim.vic_leaders.slots[owner].num_queued[402] = 11;
    (sim, production, row)
}

fn game_seconds(seconds: i32) -> ExternalGameSeconds {
    ExternalGameSeconds::admit(Some(seconds), Some(seconds)).unwrap()
}

#[test]
fn insufficient_resources_return_zero_before_cursor_group_or_queue_commit() {
    let fixture = repo_root().join("crates/don-replay/tests/fixtures/bhs_research_rollback.bhs");
    let inc = don_bhs_cc::load::install_include_path(repo_root());
    let loaded = don_bhs_cc::load::load_script_file(&inc, &fixture).unwrap();
    let mut script_runtime = ScriptRuntime::new(loaded.program, None, None).unwrap();
    let binding = ReplayBhsBinding {
        file: 0,
        name: "research_only".into(),
    };
    let mut call = ReplayProductionCall {
        who: 1,
        step: 99,
        boom_vs_rush: 1,
        num_loops: 5,
    };
    let image = ProductionBuiltinImage {
        leaders: std::array::from_fn(|who| {
            if who == OWNER {
                ProductionLeaderImage {
                    flags: 3,
                    ..ProductionLeaderImage::default()
                }
            } else {
                ProductionLeaderImage::default()
            }
        }),
        ..ProductionBuiltinImage::default()
    };
    let (types, upgrades) = canonical_type_owners(OWNER);
    let (mut sim, mut production, row) = production_owners(OWNER);
    production.leaders[OWNER].resources = [0; 6];
    sim.leaders[OWNER].econ.stockpile = [0; 6];
    sim.step8.leaders[OWNER].econ.stockpile = [0; 6];
    sim.vic_leaders.slots[OWNER].economy.bucket = [0; 6];
    let groups_before = sim.groups.clone();
    let queue_before = sim.builds[row].queue.clone();

    let receipt = run_production_research_call(
        &mut script_runtime,
        &binding,
        &mut call,
        &image,
        &types,
        &upgrades,
        &mut sim,
        &mut production,
        game_seconds(0),
    )
    .unwrap();

    assert_eq!(call.step, 0);
    assert_eq!(receipt.production.returned, 0);
    assert_eq!(receipt.research.len(), 1);
    assert_eq!(
        receipt.research[0].status,
        SingleLibraryResearchStatus::Unavailable
    );
    assert_eq!(sim.scenario_data.find_counters[30], BUILD_BAND_BASE as i32);
    assert_eq!(sim.groups.list, groups_before.list);
    assert_eq!(sim.groups.last_group, groups_before.last_group);
    assert_eq!(sim.builds[row].queue.queued, queue_before.queued);
    assert_eq!(
        sim.builds[row].queue.entries[0].image(),
        queue_before.entries[0].image()
    );
    assert_eq!(production.leaders[OWNER].resources, [0; 6]);
    assert_eq!(production.leaders[OWNER].epochs_queued, 0);
}

#[test]
fn later_vm_failure_rolls_back_program_ref_timer_cursor_group_queue_and_all_leader_mirrors() {
    let fixture = repo_root().join("crates/don-replay/tests/fixtures/bhs_research_rollback.bhs");
    let inc = don_bhs_cc::load::install_include_path(repo_root());
    let loaded = don_bhs_cc::load::load_script_file(&inc, &fixture).unwrap();
    let pristine_program = don_replay::script_channel::checksum_program(&loaded.program).unwrap();
    let mut timers = ScriptTimers::default();
    timers.add_timer("1", 300).unwrap();
    let pristine_timers = timers.clone();
    let mut script_runtime =
        ScriptRuntime::new_with_timers(loaded.program, None, None, timers).unwrap();
    let binding = ReplayBhsBinding {
        file: 0,
        name: "research_then_fail".into(),
    };
    let mut call = ReplayProductionCall {
        who: 1,
        step: 99,
        boom_vs_rush: 1,
        num_loops: 5,
    };
    let call_before = call;
    let image = ProductionBuiltinImage {
        leaders: std::array::from_fn(|who| {
            if who == OWNER {
                ProductionLeaderImage {
                    flags: 3,
                    ..ProductionLeaderImage::default()
                }
            } else {
                ProductionLeaderImage::default()
            }
        }),
        ..ProductionBuiltinImage::default()
    };
    let (types, upgrades) = canonical_type_owners(OWNER);
    let (mut sim, mut production, row) = production_owners(OWNER);
    let idle_unit = sim
        .spawn_unit(OWNER, 50, 100, 200, 4)
        .expect("install one live idle Citizen captain");
    let idle_row = sim.world.row_of(idle_unit).unwrap();
    let idle_flags_before = sim.world.units.get_flags(idle_row);
    let idle_inside_before = sim.world.units.inside_up()[idle_row];
    let idle_captain_before = sim.world.units.o_up()[idle_row];
    let idle_orders_before = sim.world.orders(idle_row).clone();
    let cities_before = sim.cities.clone();
    let scenario_before = sim.scenario_data;
    let groups_before = sim.groups.clone();
    let queue_before = sim.builds[row].queue.clone();
    let resources_before = production.leaders[OWNER].resources;

    let error = run_production_research_call(
        &mut script_runtime,
        &binding,
        &mut call,
        &image,
        &types,
        &upgrades,
        &mut sim,
        &mut production,
        game_seconds(0),
    )
    .unwrap_err();

    assert!(
        matches!(
            &error.failure,
            ProductionRunFailure::Vm(VmError::UnimplementedBuiltin {
                index: 94,
                name: "get_is_no_nation_powers"
            })
        ),
        "unexpected failure: {:?}",
        error.failure
    );
    assert_eq!(
        error
            .trace
            .iter()
            .map(|call| call.index)
            .collect::<Vec<_>>(),
        [78, 357, 455, 386]
    );
    assert_eq!(call, call_before);
    assert_eq!(
        don_replay::script_channel::checksum_program(script_runtime.program()).unwrap(),
        pristine_program
    );
    assert_eq!(script_runtime.script_timers(), &pristine_timers);
    assert_eq!(
        error.trace.last().map(|call| (&call.index, &call.returned)),
        Some((&386, &ProductionBuiltinValue::Int(0)))
    );
    assert_eq!(sim.scenario_data, scenario_before);
    assert_eq!(sim.cities.slots, cities_before.slots);
    assert_eq!(sim.cities.city_mark, cities_before.city_mark);
    assert_eq!(sim.groups.list, groups_before.list);
    assert_eq!(sim.groups.last_group, groups_before.last_group);
    assert_eq!(sim.groups.proc_group, groups_before.proc_group);
    assert_eq!(sim.builds[row].queue.queued, queue_before.queued);
    assert_eq!(sim.world.units.get_flags(idle_row), idle_flags_before);
    assert_eq!(sim.world.units.inside_up()[idle_row], idle_inside_before);
    assert_eq!(sim.world.units.o_up()[idle_row], idle_captain_before);
    assert_eq!(sim.world.orders(idle_row), &idle_orders_before);
    assert_eq!(sim.unit_type[idle_row], 50);
    assert_eq!(
        sim.builds[row].queue.entries[0].image(),
        queue_before.entries[0].image()
    );
    assert_eq!(production.leaders[OWNER].resources, resources_before);
    assert_eq!(
        production.leaders[OWNER].queued_counts[WRITTEN_WORD as usize],
        0
    );
    assert_eq!(production.leaders[OWNER].ages_queued, 0);
    assert_eq!(production.leaders[OWNER].epochs_queued, 0);
    assert_eq!(sim.leaders[OWNER].econ.stockpile, resources_before);
    assert_eq!(sim.step8.leaders[OWNER].econ.stockpile, resources_before);
    assert_eq!(
        sim.vic_leaders.slots[OWNER].economy.bucket,
        resources_before
    );
    assert_eq!(
        sim.vic_leaders.slots[OWNER].num_queued[WRITTEN_WORD as usize],
        0
    );
}

#[test]
fn shipped_economic_program_reaches_written_word_then_the_measured_city_state_cohort() {
    let content_root = repo_root().join("ron-data/bhs-corpus");
    if !content_root.is_dir() {
        eprintln!(
            "\n  SKIPPED — NOT A PASS. Installed ron-data/bhs-corpus is absent; the shipped trace was not established.\n"
        );
        return;
    }
    let replay_path =
        repo_root().join("ron-data/replays/multi/Playback___2020.07.25_19_30_12__Sat_.rcx");
    if !replay_path.is_file() {
        eprintln!(
            "\n  SKIPPED — NOT A PASS. The installed AI replay witness is absent; the canonical replay-selected BHS path was not established.\n"
        );
        return;
    }
    let replay = Replay::open(&replay_path).expect("decode installed AI replay witness");
    let content_owner = 2usize;
    assert!(replay.initial.active_players().any(|player| {
        player.slot as usize == content_owner && player.flags & LEADER_FLAG_HUMAN == 0
    }));
    let loaded = load_replay_bhs_program(&replay.initial, &content_root)
        .expect("select installed economic.bhs from the replay's AI roster");
    let pristine_program = loaded.checksum;
    let mut call = ReplayProductionCall {
        who: content_owner as i32 + 1,
        step: 1,
        boom_vs_rush: 1,
        num_loops: 5,
    };
    let call_before = call;
    let (types, upgrades) = canonical_type_owners(content_owner);
    let (mut sim, mut production, row) = production_owners(content_owner);
    let installed_style = don_replay::map_style::MapStyleStaticData::load_from_ron_data(
        &repo_root().join("ron-data"),
        12,
    )
    .expect("load installed Mediterranean map-style owner");
    let image = ProductionBuiltinImage {
        map_style: Some(bind_production_map_style(12, &installed_style).unwrap()),
        setup: Some(ProductionSetupImage {
            game_rules: 0,
            starting_town: 2,
            starting_resources: 1,
            starting_resources2: 1,
            semaphore: [0; 32],
        }),
        leaders: std::array::from_fn(|who| {
            if who == content_owner {
                ProductionLeaderImage {
                    flags: 3,
                    flags2: 0,
                    is_major_power: Some(true),
                    city_num: 1,
                    nation: "Romans".into(),
                    age: 0,
                    cities: vec![ProductionCityImage {
                        active: true,
                        object_id: 100,
                        name: "Athens".into(),
                        identity: "capital_0".into(),
                        last_attacked: 0,
                        last_raided: 0,
                    }],
                }
            } else {
                ProductionLeaderImage::default()
            }
        }),
        type_counts: Some(
            bind_production_type_counts(&types, &upgrades, &sim.vic_leaders.slots).unwrap(),
        ),
        population: None,
        techs_per_age: [Some(4), None, None, None, None, None, None],
    };
    let mut timers = ScriptTimers::default();
    timers.add_timer("1", 300).unwrap();
    let pristine_timers = timers.clone();

    let (mut failure_sim, mut failure_production, failure_row) = production_owners(content_owner);
    failure_production.leaders[content_owner].resources = [0; 6];
    failure_sim.leaders[content_owner].econ.stockpile = [0; 6];
    failure_sim.step8.leaders[content_owner].econ.stockpile = [0; 6];
    failure_sim.vic_leaders.slots[content_owner].economy.bucket = [0; 6];
    let failure_scenario_before = failure_sim.scenario_data;
    let failure_groups_before = failure_sim.groups.clone();
    let failure_queue_before = failure_sim.builds[failure_row].queue.clone();
    let mut failure_call = call;
    let failure_loaded = load_replay_bhs_program(&replay.initial, &content_root)
        .expect("reload the installed replay-selected program for the refusal arm");
    let (mut failure_runtime, failure_binding) = failure_loaded
        .into_script_runtime_with_timers(timers.clone())
        .expect("bind the installed refusal runtime");
    let failure_binding = failure_binding.expect("AI replay carries an economic binding");
    let refusal = run_production_research_call(
        &mut failure_runtime,
        &failure_binding,
        &mut failure_call,
        &image,
        &types,
        &upgrades,
        &mut failure_sim,
        &mut failure_production,
        game_seconds(0),
    )
    .expect("the installed insufficient-resource path returns from economic.bhs");
    let refused = refusal
        .production
        .trace
        .iter()
        .position(|entry| {
            entry.index == 357
                && entry.args.get(1) == Some(&ProductionBuiltinValue::Str("Written Word".into()))
        })
        .expect("installed stock execution reaches the refused Written Word builtin 357");
    assert_eq!(
        refusal.production.trace[refused].returned,
        ProductionBuiltinValue::Int(0)
    );
    assert_eq!(
        refusal.production.trace[refused + 1].index,
        332,
        "the installed insufficient-resource continuation is at_least_type"
    );
    assert_eq!(
        refusal.production.trace[refused + 1].args,
        [
            ProductionBuiltinValue::Int(content_owner as i32 + 1),
            ProductionBuiltinValue::Int(75),
            ProductionBuiltinValue::Str("Wealth".into()),
        ]
    );
    assert_eq!(
        refusal.production.trace[refused + 1].returned,
        ProductionBuiltinValue::Int(0)
    );
    assert_eq!(refusal.production.returned, 1);
    assert_eq!(failure_call.step, 7);
    assert_eq!(failure_call.who, call_before.who);
    assert_eq!(failure_call.boom_vs_rush, call_before.boom_vs_rush);
    assert_eq!(failure_call.num_loops, call_before.num_loops);
    let mut expected_failure_scenario = failure_scenario_before;
    expected_failure_scenario.find_counters[30] = BUILD_BAND_BASE as i32;
    assert_eq!(failure_sim.scenario_data, expected_failure_scenario);
    assert_eq!(failure_sim.groups.list, failure_groups_before.list);
    assert_eq!(
        failure_sim.builds[failure_row].queue.queued,
        failure_queue_before.queued
    );
    assert_eq!(
        failure_sim.builds[failure_row].queue.entries,
        failure_queue_before.entries
    );
    assert_eq!(failure_production.leaders[content_owner].resources, [0; 6]);
    assert!(refusal
        .research
        .iter()
        .all(|receipt| receipt.status == SingleLibraryResearchStatus::Unavailable));

    let (mut script_runtime, binding) = loaded
        .into_script_runtime_with_timers(timers)
        .expect("bind the installed success runtime");
    let binding = binding.expect("AI replay carries an economic binding");
    let groups_before = sim.groups.clone();
    let queue_before = sim.builds[row].queue.clone();

    let error = run_production_research_call(
        &mut script_runtime,
        &binding,
        &mut call,
        &image,
        &types,
        &upgrades,
        &mut sim,
        &mut production,
        game_seconds(0),
    )
    .expect_err("the next unowned production builtin must keep the whole call red");
    assert!(
        matches!(
            &error.failure,
            ProductionRunFailure::Vm(VmError::UnimplementedBuiltin {
                index: 436,
                name: "num_type_queued"
            })
        ),
        "unexpected installed continuation: {:?}",
        error.failure
    );

    let ww = error
        .trace
        .iter()
        .position(|entry| {
            entry.index == 357
                && entry.args.get(1) == Some(&ProductionBuiltinValue::Str("Written Word".into()))
        })
        .expect("installed stock execution reaches Written Word builtin 357");
    assert_eq!(
        error.trace[ww].returned,
        ProductionBuiltinValue::Int(BUILD_BAND_BASE as i32)
    );
    assert_eq!(
        error.trace[ww + 1..ww + 4]
            .iter()
            .map(|entry| entry.index)
            .collect::<Vec<_>>(),
        [258, 362, 357],
        "the immediate measured success continuation is num_cities/have_tech/City State research"
    );
    assert_eq!(
        error.trace[ww + 3].args.get(1),
        Some(&ProductionBuiltinValue::Str("City State".into()))
    );
    assert_eq!(
        error.trace[ww + 3].returned,
        ProductionBuiltinValue::Int(BUILD_BAND_BASE as i32)
    );
    let idle = error
        .trace
        .iter()
        .position(|entry| entry.index == 455)
        .expect("installed success continuation reaches the live idle-Unit census");
    assert_eq!(
        error.trace[idle].args,
        [
            ProductionBuiltinValue::Int(content_owner as i32 + 1),
            ProductionBuiltinValue::Str("Citizens".into()),
        ]
    );
    assert_eq!(
        error.trace[idle].returned,
        ProductionBuiltinValue::Int(0),
        "the installed Sim has an empty canonical Unit band"
    );
    let city_counts: Vec<_> = error
        .trace
        .iter()
        .skip(idle + 1)
        .take_while(|entry| entry.index == 386)
        .collect();
    assert_eq!(city_counts.len(), 3);
    assert_eq!(
        city_counts
            .iter()
            .map(|entry| entry.args.get(2))
            .collect::<Vec<_>>(),
        [
            Some(&ProductionBuiltinValue::Str("Farm".into())),
            Some(&ProductionBuiltinValue::Str("Woodcutter's Camp".into())),
            Some(&ProductionBuiltinValue::Str("Mine".into())),
        ]
    );
    assert!(city_counts
        .iter()
        .all(|entry| entry.returned == ProductionBuiltinValue::Int(0)));

    assert_eq!(call, call_before);
    assert_eq!(
        don_replay::script_channel::checksum_program(script_runtime.program()).unwrap(),
        pristine_program
    );
    assert_eq!(script_runtime.script_timers(), &pristine_timers);
    assert_eq!(sim.scenario_data.find_counters[30], -1);
    assert_eq!(sim.groups.list, groups_before.list);
    assert_eq!(sim.groups.last_group, groups_before.last_group);
    assert_eq!(sim.builds[row].queue.queued, queue_before.queued);
    assert_eq!(
        production.leaders[content_owner].queued_counts[WRITTEN_WORD as usize],
        0
    );
    assert_eq!(
        production.leaders[content_owner].queued_counts[CITY_STATE as usize],
        0
    );
    assert_eq!(production.leaders[content_owner].ages_queued, 0);
    assert_eq!(production.leaders[content_owner].epochs_queued, 0);
}
