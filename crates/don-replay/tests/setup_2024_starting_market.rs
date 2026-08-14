use std::path::{Path, PathBuf};

use don_replay::replay::Replay;
use don_replay::setup_2024_starting_market::{
    derive_golden_starting_market_plan, DUTCH_STARTING_MARKET_O, DUTCH_STARTING_MARKET_TYPE,
    MARKET_DOMAIN, MARKET_FOOTPRINT, STARTING_VILLAGE_O,
};
use don_sim::systems::leader_produce_building_blocked_site_prefix::MARKET_BUILD_FLAGS;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn installed_golden_replay_requires_market_before_build_units() {
    let path = repo_root().join("ron-data/replays/multi/Playback___2024.02.23_20_49_35__Fri_.rcx");
    if !path.exists() {
        eprintln!("SKIPPED -- NOT A PASS: missing {}", path.display());
        return;
    }
    let replay = Replay::open(&path).expect("installed 2024 witness must decode");
    let plan = derive_golden_starting_market_plan(&replay).unwrap();

    assert_eq!(plan.owner, 0);
    assert_eq!(plan.tribe, 22);
    assert_eq!(
        plan.probes
            .iter()
            .map(|probe| (probe.bonus, probe.granted))
            .collect::<Vec<_>>(),
        [
            (4, false),
            (22, true),
            (5, false),
            (16, false),
            (7, false),
            (10, false),
            (18, false),
        ]
    );
    assert_eq!(plan.calls.len(), 1);
    assert_eq!(plan.calls[0].type_index, DUTCH_STARTING_MARKET_TYPE);
    assert_eq!(plan.calls[0].origin_build_object, STARTING_VILLAGE_O as i16);
    assert_eq!(plan.calls[0].mode, 0);
    assert_eq!(DUTCH_STARTING_MARKET_O, 2001);
    assert_eq!(plan.market_type.build_flags, MARKET_BUILD_FLAGS);
    assert_eq!(plan.market_type.domain, MARKET_DOMAIN);
    assert_eq!(
        [plan.market_type.x_size, plan.market_type.y_size],
        [MARKET_FOOTPRINT.x_size, MARKET_FOOTPRINT.y_size]
    );
}
