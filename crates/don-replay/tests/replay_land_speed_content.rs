use std::path::{Path, PathBuf};

use don_replay::replay::{load_payload, Replay};
use don_replay::replay_land_speed_content::{
    produce_replay_land_speed_content, produce_replay_land_speed_content_from_payload,
    ReplayLandSpeedContentError,
};
use don_sim::systems::land_speed_authority::{LandSpeedConstants, LandSpeedContent};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn witness() -> Option<Replay> {
    let path = repo_root().join("ron-data/replays/multi/Playback___2024.02.23_20_49_35__Fri_.rcx");
    match Replay::open(&path) {
        Ok(replay) => Some(replay),
        Err(error) => {
            eprintln!("SKIPPED -- NOT A PASS: {}: {error}", path.display());
            None
        }
    }
}

#[test]
fn admitted_2024_rules_supply_every_exact_land_speed_content_input() {
    let Some(replay) = witness() else {
        return;
    };
    let content = produce_replay_land_speed_content(&replay).unwrap();
    assert_eq!(content.type_count(), 364);
    assert_ne!(content.land_speed_revision(), 0);
    assert_eq!(
        content.land_speed_composition_digest(),
        replay.initial.rules.unwrap().serialized_sha256
    );
    assert_eq!(
        content.constants(),
        LandSpeedConstants {
            coord_scale: 1,
            irq_spear_bonus: 1,
            irq_mo_spear_bonus: 1,
            irq_hmo_spear_bonus: 1,
            irq_emo_spear_bonus: 1,
            alexander_napoleon_aura_256: 384,
            spitamenes_stable_256: 307,
            porus_elephant_256: 307,
            napoleon_siege_percent: 150,
            charles_percent: 120,
            blucher_stable_percent: 120,
            hero_aura_speed: 42,
        }
    );

    let citizen = content.type_fact(50).unwrap();
    assert_eq!(
        citizen,
        don_sim::systems::land_speed_authority::LandSpeedTypeFacts {
            type_id: 50,
            from: -1,
            where_type: 414,
            graft: -1,
            domain: 0,
            unit_flags: 6_273,
            unit_flags2: 2,
        }
    );
    assert_eq!(content.type_fact(49), None);
    assert_eq!(content.type_fact(414), None);
}

#[test]
fn payload_or_rules_mutation_cannot_be_rebound_as_speed_content() {
    let Some(replay) = witness() else {
        return;
    };
    let mut payload = load_payload(&replay.path).unwrap();
    payload[replay.initial.rules.unwrap().serialized_offset + 9] ^= 1;
    assert_eq!(
        produce_replay_land_speed_content_from_payload(&replay, &payload),
        Err(ReplayLandSpeedContentError::PayloadSha256Mismatch)
    );
}
