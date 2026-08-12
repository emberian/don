use std::path::{Path, PathBuf};

use don_replay::groups_first_farm_authority::{
    discover_first_2018_farm, FirstFarmAuthorityBlocker, FirstFarmAuthorityError, FIRST_FRAME,
    FIRST_OWNER, FIRST_PLAY, FIRST_SELECTED_O, FIRST_SERIAL, STRICT_REPLAY_SHA256,
};
use don_replay::groups_pre_pair_unit_authority::{
    replay_build_type_facts, PrePairUnitAuthorityError,
};
use don_replay::replay::{load_payload, Replay};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}

fn replay_path() -> PathBuf {
    repo_root().join("ron-data/replays/multi/Playback___2018.11.17_13_21_42__Sat_.rcx")
}

#[test]
fn first_real_farm_advances_to_the_exact_setup_and_runtime_boundary() {
    let path = replay_path();
    let replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let discovered = discover_first_2018_farm(&replay).unwrap();

    assert_eq!(discovered.replay_file_sha256, STRICT_REPLAY_SHA256);
    assert_eq!(
        (
            discovered.package.lockstep_serial,
            discovered.package.package_frame,
            discovered.package.play,
            discovered.package.who,
            discovered.package.objects.as_slice(),
        ),
        (
            FIRST_SERIAL,
            FIRST_FRAME,
            FIRST_PLAY,
            FIRST_OWNER,
            &[FIRST_SELECTED_O][..],
        )
    );
    assert_eq!(discovered.package.command_index, 0);
    assert_eq!(discovered.package.shell_prefix, []);
    assert_eq!(discovered.package.shell_suffix, [0x4a, 0x48]);
    assert_eq!(
        (
            discovered.player_slot,
            discovered.tribe_index,
            discovered.tribe.tribe_id,
            discovered.scout.type_index,
            discovered.citizen.type_index,
        ),
        (1, 14, 14, 69, 50)
    );

    assert_eq!(
        (
            discovered.center_build_o,
            discovered.center_position,
            discovered.center_world_cell,
            discovered.reconstructed_center_region,
            discovered.expected_center_region,
            discovered.city_slot
        ),
        (2_000, (21_600, 64_608), (28, 84), 64, 1, 0)
    );
    assert_eq!(
        (
            discovered.builder_schedule.base_scout_calls,
            discovered.builder_schedule.citizen_calls,
            discovered.builder_schedule.selected_o,
            discovered.builder_schedule.selected_call_ordinal,
            discovered.builder_schedule.selected_citizen_index,
        ),
        (1, 4, 4, 4, 3)
    );
    assert!(!discovered.builder_schedule.allocation_receipts_bound);
    assert!(!discovered.builder_schedule.current_upgrade_bound);

    assert_eq!((discovered.farm.x_size, discovered.farm.y_size), (4, 4));
    assert_eq!(discovered.geometry.requested, (22_286, 66_184));
    assert_eq!(discovered.geometry.requested_second, (22_286, 66_184));
    assert_eq!(discovered.geometry.corner_tcoord, (114, 342));
    assert_eq!(discovered.geometry.snapped, (22_272, 66_048));
    assert_eq!(discovered.geometry.world_cell, (29, 86));

    assert!(discovered.recorded_groups_matches_pre_issue_state());
    assert_eq!(discovered.recorded_groups_checksum, 0x1c78_f3f5);
    assert_eq!(discovered.recorded_units_checksum, 0x2bc4_5014);
    assert!(!discovered.runtime_authority_ready());
    assert_eq!(
        discovered.blockers,
        [
            FirstFarmAuthorityBlocker::PostWorldgenRandomState,
            FirstFarmAuthorityBlocker::SetupPlacementWorldSnapshot,
            FirstFarmAuthorityBlocker::ObjectsInitUnitReceipts,
            FirstFarmAuthorityBlocker::Frame79UnitHandleAndState,
            FirstFarmAuthorityBlocker::InterveningFrameChronology,
            FirstFarmAuthorityBlocker::Frame79CityAfterImage,
            FirstFarmAuthorityBlocker::Frame79WorldObjectHead,
            FirstFarmAuthorityBlocker::ValidateBuildProbeChronology,
            FirstFarmAuthorityBlocker::ObjectsInitBuildAfterImage,
            FirstFarmAuthorityBlocker::BuilderSwarmSearchAfterImage,
        ]
    );

    eprintln!(
        "first Farm facts: Farm={:#?} Units={:08x} blockers={:?}",
        discovered.farm, discovered.recorded_units_checksum, discovered.blockers
    );
}

#[test]
fn in_memory_wire_or_rules_mutation_cannot_be_promoted_to_first_packet_authority() {
    let path = replay_path();
    let mut replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let action = replay
        .turns
        .iter_mut()
        .find(|turn| turn.turn == FIRST_SERIAL)
        .unwrap()
        .players
        .iter_mut()
        .find(|player| player.play == FIRST_PLAY as i32)
        .unwrap()
        .commands
        .iter_mut()
        .find(|command| command.opcode == 25)
        .unwrap();
    action.bytes[1] ^= 1;
    assert_eq!(
        discover_first_2018_farm(&replay).unwrap_err(),
        FirstFarmAuthorityError::WrongFirstPackage
    );

    let replay = Replay::open(&path).unwrap();
    let rules = replay.initial.rules.unwrap();
    let mut payload = load_payload(&path).unwrap();
    let farm = replay_build_type_facts(&payload, &rules, 0x1a1).unwrap();
    payload[farm.spans.object.offset + (0x234 - 0x1e4)] ^= 1;
    assert_eq!(
        replay_build_type_facts(&payload, &rules, 0x1a1),
        Err(PrePairUnitAuthorityError::RulesSha256Mismatch)
    );
}
