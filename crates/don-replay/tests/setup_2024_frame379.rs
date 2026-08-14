use std::path::{Path, PathBuf};

use don_replay::replay::Replay;
use don_replay::setup_2024_frame379::{
    discover_frame379_setup, produce_frame379_setup, Frame379LeaderSetupAuthority,
    Frame379LeaderSetupSource, Frame379SetupError, Frame379WorldgenAuthority,
    Frame379WorldgenSource, REPLAY_FILE_SHA256,
};
use don_replay::setup_units_producer::{
    StartingUnitBonuses, StartingUnitPhase, StartingUnitTypeFacts, TypeResolutionFacts,
    BASE_PEASANT_TYPE, BASE_SCOUT_TYPE, DUTCH_MERCHANT_TYPE,
};
use don_sim::systems::map_terrain::{SectionDigest, WorldChecksum, WorldSection};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn installed_witness() -> Option<Replay> {
    let path = repo_root().join("ron-data/replays/multi/Playback___2024.02.23_20_49_35__Fri_.rcx");
    if !path.exists() {
        eprintln!("SKIPPED -- NOT A PASS: missing {}", path.display());
        return None;
    }
    Some(Replay::open(&path).expect("installed 2024 witness must decode"))
}

fn type_facts(base: i32, squad_size: i32, crew_size: i32) -> TypeResolutionFacts {
    TypeResolutionFacts {
        base,
        tribe_can_base: true,
        nation_variant: base,
        build_units_upgrade: base,
        place_unit_upgrade: base,
        uber_size: 1,
        squad_size,
        crew_size,
    }
}

fn leader() -> Frame379LeaderSetupAuthority {
    Frame379LeaderSetupAuthority {
        revision: 1,
        composition_digest: [0xa5; 32],
        source: Frame379LeaderSetupSource::CanonicalNewGameLeaderTypeState,
        replay_file_sha256: REPLAY_FILE_SHA256,
        owner: 0,
        tribe: 22,
        bonuses: StartingUnitBonuses {
            dutch_merchants: true,
            ..StartingUnitBonuses::default()
        },
        types: StartingUnitTypeFacts {
            scout: type_facts(BASE_SCOUT_TYPE, 1, 1),
            dutch_merchant: type_facts(DUTCH_MERCHANT_TYPE, 1, 2),
            citizen: type_facts(BASE_PEASANT_TYPE, 1, 0),
        },
    }
}

#[test]
fn source_backed_witness_has_exact_seven_call_schedule_and_guy_shape() {
    let Some(replay) = installed_witness() else {
        return;
    };
    let facts = discover_frame379_setup(&replay, &leader()).unwrap();
    assert_eq!(facts.center_position, (74_592, 10_080));
    assert_eq!(facts.start_tile, (97, 13));
    assert_eq!(facts.center_build_o, 2_000);
    assert_eq!(
        facts
            .plan
            .calls
            .iter()
            .map(|call| call.place_unit_upgrade)
            .collect::<Vec<_>>(),
        [69, 62, 62, 50, 50, 50, 50]
    );
    assert_eq!(
        facts
            .plan
            .calls
            .iter()
            .map(|call| call.squad_size + call.crew_size)
            .collect::<Vec<_>>(),
        [2, 3, 3, 1, 1, 1, 1]
    );
    assert!(matches!(
        facts.plan.calls[0].phase,
        StartingUnitPhase::BaseScout
    ));
    assert!(matches!(
        facts.plan.calls[1].phase,
        StartingUnitPhase::DutchMerchant { index: 0 }
    ));
    assert!(matches!(
        facts.plan.calls[2].phase,
        StartingUnitPhase::DutchMerchant { index: 1 }
    ));
    assert_eq!(
        facts.plan.calls[3..]
            .iter()
            .map(|call| call.phase)
            .collect::<Vec<_>>(),
        (0..4)
            .map(|index| StartingUnitPhase::Citizen { index })
            .collect::<Vec<_>>()
    );
}

#[test]
fn dynamic_leader_selectors_fail_closed() {
    let Some(replay) = installed_witness() else {
        return;
    };
    let mut dynamic = leader();
    dynamic.types.dutch_merchant.place_unit_upgrade = 63;
    assert!(matches!(
        discover_frame379_setup(&replay, &dynamic),
        Err(Frame379SetupError::LeaderAuthorityMismatch)
            | Err(Frame379SetupError::WrongResolvedType)
            | Err(Frame379SetupError::TypeFacts(_))
    ));

    let mut dynamic = leader();
    dynamic.bonuses.extra_scouts = true;
    assert_eq!(
        discover_frame379_setup(&replay, &dynamic),
        Err(Frame379SetupError::LeaderAuthorityMismatch)
    );

    let mut dynamic = leader();
    dynamic.composition_digest = [0; 32];
    assert_eq!(
        discover_frame379_setup(&replay, &dynamic),
        Err(Frame379SetupError::MissingLeaderCompositionDigest)
    );
}

#[test]
fn chronology_refuses_to_substitute_an_empty_receipt_set_for_real_units() {
    let Some(replay) = installed_witness() else {
        return;
    };
    let worldgen = Frame379WorldgenAuthority {
        revision: 1,
        composition_digest: [0x5a; 32],
        source: Frame379WorldgenSource::CompletedGreatLakesWorldgenAndStartingVillage,
        replay_file_sha256: REPLAY_FILE_SHA256,
        world_checksum: WorldChecksum {
            per_section: [SectionDigest::default(); WorldSection::COUNT],
            full: 0,
            bytes: 0,
        },
        random_state: 0,
    };
    assert_eq!(
        produce_frame379_setup(&replay, &[], &[], &worldgen, &leader()),
        Err(Frame379SetupError::WrongReceiptCount)
    );

    let mut missing = worldgen;
    missing.composition_digest = [0; 32];
    assert_eq!(
        produce_frame379_setup(&replay, &[], &[], &missing, &leader()),
        Err(Frame379SetupError::MissingCompositionDigest)
    );
}
