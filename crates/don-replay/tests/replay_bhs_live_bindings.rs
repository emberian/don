//! Strict first-call boundary for the stock production BHS program.

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
#[path = "../src/replay_bhs_runtime.rs"]
mod replay_bhs_runtime;

use don_bhs::{find_builtin, Host, HostError, Value, VmError};
use don_replay::replay::Replay;
use replay_bhs_live_bindings::{
    bind_production_call, bind_production_map_style, bind_production_population,
    bind_production_type_counts, run_production_call, ProductionBuiltinImage,
    ProductionCallBindError, ProductionCityImage, ProductionLeaderImage,
    ProductionMapStyleBindError, ProductionRetainedState, ProductionRunFailure,
    ProductionSetupImage, ReplayProductionBuiltinHost,
};
use replay_bhs_runtime::{ReplayBhsBinding, LEADER_FLAG_HUMAN};

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
use don_sim::systems::victory_score::LeaderState as VictoryLeaderState;
use don_sim::systems::{leaders as step8_leaders, victory_score};
use leaders_runtime_frontier::bind_live as bind_runtime_leaders;

const CHECKSUM_RECORDINGS: [&str; 21] = [
    "Playback___2018.11.17_13_21_42__Sat_.rcx",
    "Playback___2018.12.01_18_33_16__Sat_.rcx",
    "Playback___2019.03.24_11_56_19__Sun_.rcx",
    "Playback___2020.02.08_10_49_15__Sat_.rcx",
    "Playback___2020.02.21_09_48_48__Fri_.rcx",
    "Playback___2020.07.25_19_30_12__Sat_.rcx",
    "Playback___2020.07.25_19_32_40__Sat_.rcx",
    "Playback___2020.07.25_19_42_43__Sat_.rcx",
    "Playback___2024.02.23_20_49_35__Fri_.rcx",
    "Playback___2024.02.23_21_38_38__Fri_.rcx",
    "Playback___2024.02.24_21_25_53__Sat_.rcx",
    "Playback___2024.03.10_20_54_34__Sun_.rcx",
    "Playback___2024.03.10_20_56_31__Sun_.rcx",
    "Playback___2024.03.17_19_58_17__Sun_.rcx",
    "Playback___2024.03.18_18_18_49__Mon_.rcx",
    "Playback___2024.03.20_17_28_53__Wed_.rcx",
    "Playback___2024.03.23_21_16_13__Sat_.rcx",
    "Playback___2024.03.29_21_52_57__Fri_.rcx",
    "Playback___2024.03.29_22_00_58__Fri_.rcx",
    "Playback___2024.04.10_17_05_19__Wed_.rcx",
    "Playback___2025.02.10_21_26_50__Mon_.rcx",
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn skip(reason: &str) {
    eprintln!("\n  SKIPPED — NOT A PASS. {reason}\n  Nothing was established.\n");
}

fn digest(byte: u8) -> Sha256Digest {
    Sha256Digest([byte; 32])
}

fn type_witness(role: TypeSourceRole, component: u8) -> TypeSourceWitness {
    TypeSourceWitness {
        role,
        composition: RulesCompositionId(digest(1)),
        manifest_sha256: digest(2),
        component_sha256: digest(component),
    }
}

/// Build canonical owners with all 806 upgrade/graft cells present. The selected
/// Citizen path intentionally crosses two different targets so the adapter test
/// cannot pass by counting the source row.
fn type_count_owners() -> (
    TypeBuiltinState,
    BhsCreateUnitRuntime,
    Vec<VictoryLeaderState>,
) {
    let rows = (0..NUM_TYPES)
        .map(|slot| {
            let mut row = TypeRow::empty(slot);
            row.name = match slot {
                0 => "Food".into(),
                50 | 51 => "Citizen".into(),
                52 => "Upgraded Citizen".into(),
                53 => "Grafted Citizen".into(),
                420 => "University".into(),
                427 => "Barracks".into(),
                432 => "Dock".into(),
                436 => "Market".into(),
                439 => "Tower".into(),
                572 => "The Art of War".into(),
                _ => format!("Internal {slot}"),
            };
            row.type_name = format!("Family {slot}");
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
            witness: type_witness(TypeSourceRole::TypeRows, 3),
            value: rows,
        },
        tribes: WitnessedTypeSource {
            witness: type_witness(TypeSourceRole::TribeRoster, 4),
            value: (0..NUM_TRIBES)
                .map(|slot| Some(format!("Tribe {slot}")))
                .collect(),
        },
        leaders: WitnessedTypeSource {
            witness: type_witness(TypeSourceRole::LeaderMasks, 5),
            value: (0..NUM_LEADERS)
                .map(|slot| {
                    Some(LeaderTypeMasks {
                        leader_flags: if slot == 0 { 3 } else { 0 },
                        ..LeaderTypeMasks::default()
                    })
                })
                .collect(),
        },
    })
    .unwrap();
    let (state, provenance) = produced.into_parts();

    let projections = (0..NUM_LEADERS)
        .map(|leader_slot| {
            let mut current_upgrade = (0..NUM_TYPES)
                .map(|slot| Some(slot as i32))
                .collect::<Vec<_>>();
            let mut graft = current_upgrade.clone();
            if leader_slot == 0 {
                current_upgrade[50] = Some(52);
                graft[52] = Some(53);
            }
            Some(CreateUnitLeaderProjection {
                leader_slot,
                current_upgrade,
                graft,
            })
        })
        .collect();
    let upgrades = BhsCreateUnitRuntime::new(
        CreateUnitRuntimeInput {
            witness: CreateUnitProjectionWitness {
                composition: RulesCompositionId(digest(1)),
                manifest_sha256: digest(2),
                component_sha256: digest(6),
            },
            types: vec![None; NUM_TYPES],
            leaders: projections,
            numeric_groups: Vec::new(),
        },
        &state,
        provenance,
    )
    .unwrap();

    let mut leaders = vec![VictoryLeaderState::default(); NUM_LEADERS];
    leaders[0].leader_flags = 3;
    leaders[0].num_units[0] = 3;
    leaders[0].num_units[53 - 50] = 7;
    leaders[0].num_buildings[427 - 414] = 4;
    leaders[0].num_queued[53] = 5;
    leaders[0].num_queued[427] = 6;
    leaders[0].num_queued[0] = 9;
    leaders[0].num_queued[402] = 11;
    leaders[0].economy.bucket[0] = 321;
    (state, upgrades, leaders)
}

fn population_owner_with_probe(
    control: i32,
    population_cap: i32,
    probe_flags: i32,
    probe_slot: Option<usize>,
) -> Option<(replay_bhs_live_bindings::ProductionPopulationImage, i32)> {
    let replay_root = repo_root().join("ron-data/replays/multi");
    for name in CHECKSUM_RECORDINGS {
        let Ok(replay) = Replay::open(&replay_root.join(name)) else {
            continue;
        };
        let Ok(prefix) = don_replay::leader_initial_prefix::derive(&replay.initial) else {
            continue;
        };
        let wants_active = probe_flags & victory_score::leader_flag::VALID != 0;
        let probe = probe_slot
            .filter(|slot| prefix.rows[*slot].active == wants_active)
            .or_else(|| {
                probe_slot
                    .is_none()
                    .then(|| {
                        prefix
                            .rows
                            .iter()
                            .position(|row| row.active == wants_active)
                    })
                    .flatten()
            });
        let Some(probe) = probe else {
            continue;
        };
        let types = victory_score::TypeTable::with_default_kinds(Default::default());
        let mut victory = victory_score::Leaders::new(types);
        let mut step8 = step8_leaders::Leaders::new();
        for slot in 0..NUM_LEADERS {
            let mut flags = 0i32;
            if prefix.rows[slot].active {
                flags |= victory_score::leader_flag::VALID | victory_score::leader_flag::ACTIVE;
            }
            if prefix.rows[slot].human {
                flags |= victory_score::leader_flag::HUMAN;
            }
            if slot == probe {
                flags = probe_flags;
            }
            victory.slots[slot].leader_flags = flags;
            victory.slots[slot].population_cap = population_cap;
            step8.leaders[slot].flags = flags as u32;
            step8.leaders[slot].pop_cap = population_cap;
            step8.leaders[slot].ai.control = control;
        }
        let frontier = bind_runtime_leaders(&prefix, &victory, &step8).ok()?;
        return bind_production_population(&frontier)
            .ok()
            .map(|image| (image, probe as i32 + 1));
    }
    None
}

fn population_owner(
    control: i32,
    population_cap: i32,
) -> Option<replay_bhs_live_bindings::ProductionPopulationImage> {
    population_owner_with_probe(control, population_cap, 3, Some(0)).map(|(image, _)| image)
}

#[test]
fn caller_arguments_are_setup_identity_plus_explicit_retained_state() {
    let mut player = initial::InitialPlayer {
        slot: 3,
        present: true,
        flags: 3,
        counters_and_frames: [0; 12],
        tribe: 7,
        who: 3,
        team: 0,
        handicap: 0,
        play: 0,
        pauses: 0,
        difficulty: 3,
        name: "AI".into(),
    };
    let call = bind_production_call(
        &player,
        ProductionRetainedState {
            script_step: 35,
            personality_rush: -1,
        },
    )
    .unwrap();
    assert_eq!(
        (call.who, call.step, call.boom_vs_rush, call.num_loops),
        (4, 35, 1, 5)
    );

    player.flags |= LEADER_FLAG_HUMAN;
    assert_eq!(
        bind_production_call(
            &player,
            ProductionRetainedState {
                script_step: 1,
                personality_rush: 0,
            }
        ),
        Err(ProductionCallBindError::HumanPlayer { slot: 3 })
    );
}

#[test]
fn canonical_prefix_guards_and_city_lookup_do_not_use_a_search_cursor() {
    let image = ProductionBuiltinImage {
        leaders: std::array::from_fn(|who0| {
            if who0 == 0 {
                ProductionLeaderImage {
                    flags: 3,
                    flags2: 0,
                    is_major_power: Some(true),
                    city_num: 1,
                    nation: "Greeks".into(),
                    age: 0,
                    cities: vec![ProductionCityImage {
                        active: true,
                        name: "Athens".into(),
                        ..Default::default()
                    }],
                }
            } else {
                ProductionLeaderImage::default()
            }
        }),
        techs_per_age: [Some(4), None, None, None, None, None, None],
        ..Default::default()
    };
    let mut host = ReplayProductionBuiltinHost::new(&image);
    let city = find_builtin("find_city_with_num").unwrap();
    assert_eq!(
        host.call(city, &[Value::Int(1), Value::Int(1)]),
        Ok(Value::str("Athens"))
    );
    assert_eq!(
        host.call(city, &[Value::Int(1), Value::Int(1)]),
        Ok(Value::str("Athens")),
        "builtin 383 is a direct indexed read, not a cursor advance"
    );
    assert_eq!(
        host.call(city, &[Value::Int(1), Value::Int(2)]),
        Ok(Value::str(""))
    );
    let attacked = find_builtin("was_city_attacked").unwrap();
    assert_eq!(
        host.call(attacked, &[Value::Int(1), Value::str(""), Value::Int(-1)]),
        Ok(Value::Int(0))
    );
    assert_eq!(
        host.call(
            attacked,
            &[Value::Int(1), Value::str("Athens"), Value::Int(-1)]
        ),
        Err(HostError::Unimplemented),
        "named-city attack history is outside the admitted shipped call shape"
    );
    assert_eq!(
        replay_bhs_live_bindings::PRODUCTION_PREFIX_BUILTINS,
        [
            81, 147, 245, 246, 248, 254, 255, 258, 259, 260, 261, 323, 358, 362, 377, 383, 712,
            713,
        ]
    );
}

#[test]
fn population_reads_the_reconciled_runtime_leader_control_and_cap_bytes() {
    let Some(population) = population_owner(123, 7) else {
        skip("no replay-derived Leader prefix could bind the runtime owner.");
        return;
    };
    let image = ProductionBuiltinImage {
        population: Some(population),
        ..Default::default()
    };
    let mut host = ReplayProductionBuiltinHost::new(&image);
    assert_eq!(
        host.call(find_builtin("population").unwrap(), &[Value::Int(1)]),
        Ok(Value::Int(123)),
        "retail returns control directly without clamping it to pop_cap"
    );
    assert_eq!(
        host.call(find_builtin("population_cap").unwrap(), &[Value::Int(1)]),
        Ok(Value::Int(7))
    );
    assert_eq!(
        host.call(find_builtin("population").unwrap(), &[Value::Int(9)]),
        Ok(Value::Int(-1))
    );
    for who in [0, i32::MIN, i32::MAX] {
        assert_eq!(
            host.call(find_builtin("population").unwrap(), &[Value::Int(who)]),
            Ok(Value::Int(-1))
        );
    }

    for flags in [1, 2] {
        let Some((population, who)) = population_owner_with_probe(123, 7, flags, None) else {
            skip("no replay-derived Leader row matched the requested validity shape.");
            return;
        };
        let image = ProductionBuiltinImage {
            population: Some(population),
            ..Default::default()
        };
        assert_eq!(
            ReplayProductionBuiltinHost::new(&image)
                .call(find_builtin("population").unwrap(), &[Value::Int(who)]),
            Ok(Value::Int(-1)),
            "each low Leader flag is independently required"
        );
    }
}

#[test]
fn canonical_type_count_join_covers_direct_upgrade_graft_queue_resource_and_alias_paths() {
    let (state, upgrades, leaders) = type_count_owners();
    let counts = bind_production_type_counts(&state, &upgrades, &leaders).unwrap();
    let image = ProductionBuiltinImage {
        type_counts: Some(counts),
        ..Default::default()
    };
    let mut host = ReplayProductionBuiltinHost::new(&image);
    let num_type = find_builtin("num_type").unwrap();
    let num_upgrade = find_builtin("num_type_upgrade").unwrap();
    let with_queued = find_builtin("num_type_with_queued").unwrap();

    assert_eq!(
        host.call(num_type, &[Value::Int(1), Value::str("cItIzEn")]),
        Ok(Value::Int(3)),
        "ordered duplicate lookup selects TypeIndex 50 without upgrade/graft"
    );
    assert_eq!(
        host.call(num_upgrade, &[Value::Int(1), Value::str("Citizen")]),
        Ok(Value::Int(7)),
        "50 -> current_upgrade 52 -> graft 53 reads the final active counter"
    );
    assert_eq!(
        host.call(with_queued, &[Value::Int(1), Value::str("Citizen")]),
        Ok(Value::Int(12)),
        "builtin 261 adds num_queued[53] to the final active counter"
    );
    assert_eq!(
        host.call(with_queued, &[Value::Int(1), Value::str("Barracks")]),
        Ok(Value::Int(10)),
        "the same join covers the full Build counter band"
    );
    assert_eq!(
        host.call(with_queued, &[Value::Int(1), Value::str("Food")]),
        Ok(Value::Int(321)),
        "Good types return the decoded resource stockpile without adding a queue"
    );
    assert_eq!(
        host.call(with_queued, &[Value::Int(1), Value::str("Internal 6")]),
        Err(HostError::Unimplemented),
        "unowned encrypted Good slots stay red instead of becoming zero"
    );
    assert_eq!(
        host.call(with_queued, &[Value::Int(1), Value::str("Internal 402")]),
        Ok(Value::Int(20)),
        "retail Unit types 402..413 alias their active read onto queue slots 0..11"
    );
    assert_eq!(
        host.call(with_queued, &[Value::Int(1), Value::str("Internal 700")]),
        Ok(Value::Int(-1)),
        "a resolved non-Unit/Build/Good row is not a counter target"
    );
    assert_eq!(
        host.call(
            find_builtin("have_tech").unwrap(),
            &[Value::Int(1), Value::str("The Art of War")]
        ),
        Ok(Value::Int(0)),
        "have_tech consumes the canonical TypeBuiltinState Leader bitmask"
    );
    assert_eq!(
        host.call(
            find_builtin("have_tech").unwrap(),
            &[Value::Int(1), Value::str("Food")]
        ),
        Ok(Value::Int(1)),
        "retail's Good domain returns true without reading the bitmask"
    );
    assert_eq!(
        host.call(
            find_builtin("have_tech").unwrap(),
            &[Value::Int(1), Value::str("Citizen")]
        ),
        Err(HostError::Unimplemented),
        "Unit and Build domains remain red until tribe_can_type is joined"
    );
}

#[test]
fn find_city_id_scans_city_identity_then_name_and_returns_signed_object_id() {
    let image = ProductionBuiltinImage {
        leaders: std::array::from_fn(|who0| match who0 {
            0 => ProductionLeaderImage {
                flags: 1,
                cities: vec![ProductionCityImage {
                    active: true,
                    object_id: -123,
                    name: "Athens".into(),
                    identity: "capital_0".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            1 => ProductionLeaderImage {
                flags: 1,
                cities: vec![ProductionCityImage {
                    active: true,
                    object_id: 32_767,
                    name: "Sparta".into(),
                    identity: String::new(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            _ => ProductionLeaderImage::default(),
        }),
        ..Default::default()
    };
    let builtin = find_builtin("find_city_id").unwrap();
    let mut host = ReplayProductionBuiltinHost::new(&image);
    assert_eq!(
        host.call(builtin, &[Value::str("CAPITAL_0")]),
        Ok(Value::Int(-123)),
        "City::id is compared first and City::o is sign-extended"
    );
    assert_eq!(
        host.call(builtin, &[Value::str("aThEnS")]),
        Ok(Value::Int(-123)),
        "City::name uses the same case-insensitive comparison"
    );
    assert_eq!(
        host.call(builtin, &[Value::str("")]),
        Ok(Value::Int(32_767)),
        "retail String::ignore treats two empty strings as equal"
    );
}

#[test]
fn mapstyle_binding_requires_the_installed_ordered_catalog_and_exact_replay_selector() {
    let ron_data = repo_root().join("ron-data");
    if !ron_data.join("rules.xml").is_file() {
        skip("ron-data/rules.xml is absent.");
        return;
    }
    let style = don_replay::map_style::MapStyleStaticData::load_from_ron_data(&ron_data, 12)
        .expect("load the installed Mediterranean owner");
    let binding = bind_production_map_style(12, &style).unwrap();
    assert_eq!(
        (binding.ordinal, binding.name.as_str()),
        (12, "Mediterranean")
    );
    assert!(binding.catalog_source.path.ends_with("rules.xml"));
    assert_eq!(
        bind_production_map_style(14, &style),
        Err(ProductionMapStyleBindError::SelectorMismatch {
            replay_ordinal: 14,
            installed_ordinal: 12,
        })
    );

    let image = ProductionBuiltinImage {
        map_style: Some(binding),
        ..Default::default()
    };
    let mut host = ReplayProductionBuiltinHost::new(&image);
    assert_eq!(
        host.call(find_builtin("get_mapstyle").unwrap(), &[]),
        Ok(Value::str("Mediterranean"))
    );
}

#[test]
fn setup_gates_preserve_replay_options_and_live_leader_branches() {
    let mut semaphore = [0; 32];
    semaphore[1] = 0x10; // Game semaphore bit 12.
    let mut image = ProductionBuiltinImage {
        setup: Some(ProductionSetupImage {
            game_rules: 8,
            starting_town: 2,
            starting_resources: 1,
            starting_resources2: 7,
            semaphore,
        }),
        leaders: std::array::from_fn(|who0| {
            if who0 == 0 {
                ProductionLeaderImage {
                    flags: 3,
                    flags2: 0,
                    is_major_power: Some(false),
                    city_num: 1,
                    ..Default::default()
                }
            } else {
                ProductionLeaderImage::default()
            }
        }),
        ..Default::default()
    };
    {
        let mut host = ReplayProductionBuiltinHost::new(&image);
        assert_eq!(
            host.call(find_builtin("is_conquest_scenario").unwrap(), &[]),
            Ok(Value::Int(0))
        );
        assert_eq!(
            host.call(
                find_builtin("get_starting_town_size").unwrap(),
                &[Value::Int(1)]
            ),
            Ok(Value::Int(1)),
            "scenario bit 12 maps starting town to live city presence"
        );
        assert_eq!(
            host.call(
                find_builtin("get_starting_resources").unwrap(),
                &[Value::Int(1)]
            ),
            Ok(Value::Int(7)),
            "game-rules mode 8 selects resources2 for a minor power"
        );
    }

    image.leaders[0].is_major_power = Some(true);
    image.leaders[0].flags2 = 0x80;
    let mut host = ReplayProductionBuiltinHost::new(&image);
    assert_eq!(
        host.call(
            find_builtin("get_starting_resources").unwrap(),
            &[Value::Int(1)]
        ),
        Ok(Value::Int(1))
    );
    assert_eq!(
        host.call(
            find_builtin("get_starting_town_size").unwrap(),
            &[Value::Int(1)]
        ),
        Ok(Value::Int(0)),
        "LeaderData flags2 bit 0x80 forces nomad size"
    );
}

#[test]
fn successful_four_argument_call_commits_only_the_ref_parameter() {
    let root = repo_root();
    let fixture = root.join("crates/don-bhs/tests/fixtures/mixed_params.bhs");
    let inc = don_bhs_cc::load::install_include_path(root);
    let loaded = don_bhs_cc::load::load_script_file(&inc, &fixture).unwrap();
    let mut program = loaded.program;
    let binding = ReplayBhsBinding {
        file: 0,
        name: "accumulate".into(),
    };
    let mut call = replay_bhs_live_bindings::ReplayProductionCall {
        who: 1,
        step: 10,
        boom_vs_rush: 100,
        num_loops: 1_000,
    };
    let receipt = run_production_call(
        &mut program,
        &binding,
        &mut call,
        &ProductionBuiltinImage::default(),
    )
    .unwrap();
    assert_eq!(receipt.returned, 1_111);
    assert_eq!(receipt.after_step, 1_111);
    assert_eq!(call.step, 1_111);
    assert_eq!(
        (call.who, call.boom_vs_rush, call.num_loops),
        (1, 100, 1_000)
    );
    assert!(receipt.trace.is_empty());
}

#[test]
fn strict_economic_prefix_reaches_stop_timer_and_rolls_back() {
    let content_root = repo_root().join("ron-data/bhs-corpus");
    if !content_root.is_dir() {
        skip("ron-data/bhs-corpus is absent.");
        return;
    }
    let inc = don_bhs_cc::load::install_include_path(content_root);
    let loaded = don_bhs_cc::load::load_script(&inc, replay_bhs_runtime::STANDARD_AI_SCRIPT_FILE)
        .expect("compile shipped economic program");
    let binding = ReplayBhsBinding {
        file: 0,
        name: loaded.entry,
    };
    let mut program = loaded.program;
    let pristine = don_replay::script_channel::checksum_program(&program).unwrap();
    let mut call = replay_bhs_live_bindings::ReplayProductionCall {
        who: 1,
        step: 1,
        boom_vs_rush: 1,
        num_loops: 5,
    };
    let before_call = call;
    let installed_style = don_replay::map_style::MapStyleStaticData::load_from_ron_data(
        &repo_root().join("ron-data"),
        12,
    )
    .expect("load installed Mediterranean owner");
    let (type_state, upgrades, counter_leaders) = type_count_owners();
    let population = population_owner(0, 77).expect("bind the reconciled Leader control owner");
    let image = ProductionBuiltinImage {
        map_style: Some(bind_production_map_style(12, &installed_style).unwrap()),
        setup: Some(ProductionSetupImage {
            game_rules: 0,
            starting_town: 2,
            starting_resources: 1,
            starting_resources2: 1,
            semaphore: [0; 32],
        }),
        leaders: std::array::from_fn(|who0| {
            if who0 == 0 {
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
            bind_production_type_counts(&type_state, &upgrades, &counter_leaders).unwrap(),
        ),
        population: Some(population),
        techs_per_age: [Some(4), None, None, None, None, None, None],
        ..Default::default()
    };

    let error = run_production_call(&mut program, &binding, &mut call, &image).unwrap_err();
    assert!(matches!(
        error.failure,
        ProductionRunFailure::Vm(VmError::UnimplementedBuiltin {
            index: 78,
            name: "stop_timer"
        })
    ));
    assert!(error.bytecodes_executed > 0);
    assert_eq!(
        error.trace.iter().map(|call| call.name).collect::<Vec<_>>(),
        vec![
            "num_cities",
            "find_city_with_num",
            "was_city_attacked",
            "was_city_raided",
            "find_nation",
            "get_techs_per_age",
            "age",
            "num_cities",
            "num_cities",
            "get_mapstyle",
            "get_mapstyle",
            "get_mapstyle",
            "get_mapstyle",
            "get_mapstyle",
            "get_mapstyle",
            "get_mapstyle",
            "get_mapstyle",
            "find_city_with_num",
            "find_city_with_num",
            "is_conquest_scenario",
            "get_starting_resources",
            "num_cities",
            "get_starting_town_size",
            "find_city_with_num",
            "find_city_with_num",
            "find_city_with_num",
            "find_city_id",
            "find_city_id",
            "find_city_id",
            "num_type_with_queued",
            "num_type_with_queued",
            "num_type",
            "population",
            "num_type",
            "num_type",
            "have_tech",
            "num_type",
            "find_nation",
        ]
    );
    assert_eq!(
        call, before_call,
        "failed prefix must not commit ref-step writes"
    );
    assert_eq!(
        don_replay::script_channel::checksum_program(&program).unwrap(),
        pristine,
        "failed prefix must not commit initialized BHS statics"
    );
}

#[test]
fn checksum_corpus_supplies_only_who_and_never_fabricates_retained_arguments() {
    let replay_root = repo_root().join("ron-data/replays/multi");
    if !replay_root.is_dir() {
        skip("the replay corpus is absent.");
        return;
    }
    let mut ai_recordings = 0usize;
    let mut ai_players = 0usize;
    let mut who = std::collections::BTreeMap::<u8, usize>::new();
    for name in CHECKSUM_RECORDINGS {
        let replay = Replay::open(&replay_root.join(name)).unwrap();
        let setup = ProductionSetupImage::from_initial(&replay.initial)
            .expect("checksum replay carries the complete Game semaphore");
        assert_eq!(setup.game_rules, replay.initial.info.settings.game_rules);
        assert_eq!(
            setup.starting_resources,
            replay.initial.info.settings.starting_resources
        );
        let mut in_recording = 0usize;
        for player in replay
            .initial
            .active_players()
            .filter(|player| player.flags & LEADER_FLAG_HUMAN == 0)
        {
            // Only the identity expression is replay-backed. Deliberately do not call
            // bind_production_call here: doing so would require invented step/rush state.
            assert!(player.who <= 7);
            *who.entry(player.who + 1).or_default() += 1;
            in_recording += 1;
            ai_players += 1;
        }
        if in_recording > 0 {
            ai_recordings += 1;
        }
    }
    eprintln!(
        "  checksum recordings=21; AI recordings={ai_recordings}; AI player calls={ai_players}; who={who:?}; retained step/rush claims=0"
    );
    assert_eq!(ai_recordings, 14);
    assert!(ai_players >= ai_recordings);
}
