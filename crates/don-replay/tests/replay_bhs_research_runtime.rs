//! Atomic replay/BHS proof for the builtin-357 single-Library bridge.

use std::path::{Path, PathBuf};

mod initial {
    pub use don_replay::initial::*;
}
mod leader_initial_prefix {
    pub use don_replay::leader_initial_prefix::*;
}
#[path = "../src/leaders_runtime_frontier.rs"]
mod leaders_runtime_frontier;
mod map_style {
    pub use don_replay::map_style::*;
}
mod script_channel {
    pub use don_replay::script_channel::*;
}
#[path = "../src/replay_bhs_live_bindings.rs"]
mod replay_bhs_live_bindings;
#[path = "../src/replay_bhs_research_runtime.rs"]
mod replay_bhs_research_runtime;
#[path = "../src/replay_bhs_runtime.rs"]
mod replay_bhs_runtime;

use don_bhs::{ScriptTimers, VmError};
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
use replay_bhs_live_bindings::{
    bind_production_map_style, bind_production_type_counts, ProductionBuiltinImage,
    ProductionCityImage, ProductionLeaderImage, ProductionRunFailure, ProductionSetupImage,
    ReplayProductionCall,
};
use replay_bhs_research_runtime::run_production_research_call;
use replay_bhs_runtime::ReplayBhsBinding;

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

fn canonical_type_owners() -> (TypeBuiltinState, BhsCreateUnitRuntime) {
    let rows = (0..NUM_TYPES)
        .map(|slot| {
            let mut row = TypeRow::empty(slot);
            row.name = match slot {
                0 => "Food".into(),
                2 => "Wealth".into(),
                50 | 51 => "Citizen".into(),
                52 => "Upgraded Citizen".into(),
                53 => "Grafted Citizen".into(),
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
                        leader_flags: if slot == OWNER { 3 } else { 0 },
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
                                Some(if leader_slot == OWNER && slot == 50 {
                                    52
                                } else {
                                    slot as i32
                                })
                            })
                            .collect(),
                        graft: (0..NUM_TYPES)
                            .map(|slot| {
                                Some(if leader_slot == OWNER && slot == 52 {
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

fn production_owners() -> (Sim, LiveProductionRuntime, usize) {
    let mut sim = Sim::new(0x357, 8);
    let mut build = BuildData {
        flags: flag::VALID | flag::ACTIVE,
        who: OWNER as u8,
        queue: BuildQueue {
            queued: 0,
            entries: vec![BuildQueueEntry::default(); 2],
        },
        ..BuildData::default()
    };
    build.other[off::OBJECT_ID..off::OBJECT_ID + 2]
        .copy_from_slice(&(BUILD_BAND_BASE as i16).to_le_bytes());
    let row = sim.spawn_build(OWNER, build);

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
    production.leaders[OWNER].resources = [100; 6];
    sim.leaders[OWNER].econ.stockpile = [100; 6];
    sim.step8.leaders[OWNER].econ.stockpile = [100; 6];
    sim.vic_leaders.slots[OWNER].leader_flags = 3;
    sim.vic_leaders.slots[OWNER].economy.bucket = [100; 6];
    sim.step8.leaders[OWNER].flags = 3;
    sim.vic_leaders.slots[OWNER].num_units[0] = 3;
    sim.vic_leaders.slots[OWNER].num_units[53 - 50] = 7;
    sim.vic_leaders.slots[OWNER].num_buildings[427 - 414] = 4;
    sim.vic_leaders.slots[OWNER].num_queued[53] = 5;
    sim.vic_leaders.slots[OWNER].num_queued[427] = 6;
    sim.vic_leaders.slots[OWNER].num_queued[0] = 9;
    sim.vic_leaders.slots[OWNER].num_queued[402] = 11;
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
    let (types, upgrades) = canonical_type_owners();
    let (mut sim, mut production, row) = production_owners();
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
    let (types, upgrades) = canonical_type_owners();
    let (mut sim, mut production, row) = production_owners();
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
        [78, 357]
    );
    assert_eq!(call, call_before);
    assert_eq!(
        don_replay::script_channel::checksum_program(script_runtime.program()).unwrap(),
        pristine_program
    );
    assert_eq!(script_runtime.script_timers(), &pristine_timers);
    assert_eq!(sim.scenario_data, scenario_before);
    assert_eq!(sim.groups.list, groups_before.list);
    assert_eq!(sim.groups.last_group, groups_before.last_group);
    assert_eq!(sim.groups.proc_group, groups_before.proc_group);
    assert_eq!(sim.builds[row].queue.queued, queue_before.queued);
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
    let inc = don_bhs_cc::load::install_include_path(&content_root);
    let loaded = don_bhs_cc::load::load_script(&inc, replay_bhs_runtime::STANDARD_AI_SCRIPT_FILE)
        .expect("compile installed economic.bhs");
    let pristine_program = don_replay::script_channel::checksum_program(&loaded.program).unwrap();
    let binding = ReplayBhsBinding {
        file: 0,
        name: loaded.entry,
    };
    let mut call = ReplayProductionCall {
        who: 1,
        step: 1,
        boom_vs_rush: 1,
        num_loops: 5,
    };
    let call_before = call;
    let (types, upgrades) = canonical_type_owners();
    let (mut sim, mut production, row) = production_owners();
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
            if who == OWNER {
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
    let mut script_runtime =
        ScriptRuntime::new_with_timers(loaded.program, None, None, timers).unwrap();
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

    let ww = error
        .trace
        .iter()
        .position(|entry| {
            entry.index == 357
                && entry.args.get(1)
                    == Some(&replay_bhs_live_bindings::ProductionBuiltinValue::Str(
                        "Written Word".into(),
                    ))
        })
        .expect("installed stock execution reaches Written Word builtin 357");
    assert_eq!(
        error.trace[ww].returned,
        replay_bhs_live_bindings::ProductionBuiltinValue::Int(BUILD_BAND_BASE as i32)
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
        Some(&replay_bhs_live_bindings::ProductionBuiltinValue::Str(
            "City State".into(),
        ))
    );
    assert_eq!(
        error.trace[ww + 3].returned,
        replay_bhs_live_bindings::ProductionBuiltinValue::Int(BUILD_BAND_BASE as i32)
    );

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
        production.leaders[OWNER].queued_counts[WRITTEN_WORD as usize],
        0
    );
    assert_eq!(
        production.leaders[OWNER].queued_counts[CITY_STATE as usize],
        0
    );
    assert_eq!(production.leaders[OWNER].ages_queued, 0);
    assert_eq!(production.leaders[OWNER].epochs_queued, 0);
}
