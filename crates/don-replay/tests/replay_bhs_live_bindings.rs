//! Strict first-call boundary for the stock production BHS program.

use std::path::{Path, PathBuf};

mod initial {
    pub use don_replay::initial::*;
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
    bind_production_call, run_production_call, ProductionBuiltinImage, ProductionCallBindError,
    ProductionCityImage, ProductionLeaderImage, ProductionRetainedState, ProductionRunFailure,
    ReplayProductionBuiltinHost,
};
use replay_bhs_runtime::{ReplayBhsBinding, LEADER_FLAG_HUMAN};

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
        [248, 258, 323, 358, 383, 712, 713]
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
fn strict_economic_prefix_reaches_seven_handlers_then_get_mapstyle_and_rolls_back() {
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
    let image = ProductionBuiltinImage {
        leaders: std::array::from_fn(|who0| {
            if who0 == 0 {
                ProductionLeaderImage {
                    flags: 3,
                    city_num: 1,
                    nation: "Greeks".into(),
                    age: 0,
                    cities: vec![ProductionCityImage {
                        active: true,
                        name: "Athens".into(),
                        last_attacked: 0,
                        last_raided: 0,
                    }],
                }
            } else {
                ProductionLeaderImage::default()
            }
        }),
        techs_per_age: [Some(4), None, None, None, None, None, None],
        ..Default::default()
    };

    let error = run_production_call(&mut program, &binding, &mut call, &image).unwrap_err();
    assert!(matches!(
        error.failure,
        ProductionRunFailure::Vm(VmError::UnimplementedBuiltin {
            index: 81,
            name: "get_mapstyle"
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
