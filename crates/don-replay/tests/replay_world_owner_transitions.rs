//! Real replay integration for the continent/post-continent/fertility owner chain.

use don_replay::harness::WorldSim;
use don_replay::initial::InitialItemReconstructionError;
use don_replay::map_style::{ron_data_root_for_replay, MapStyleStaticData};
use don_replay::replay::{corpus, Replay};
use don_replay::replay_world_owner_transitions::PROOF_DOCUMENT;
use don_replay::world_owner_frontier::{
    ExactPortTransitionProof, WorldByteSource, WorldOwnerError, WorldSectionMask,
};
use don_sim::systems::map_terrain::{WCoord, WorldSection};
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}

fn skip(reason: &str) {
    eprintln!("\n  SKIPPED — NOT A PASS. {reason}\n");
}

#[test]
fn checksum_bearing_corpus_commits_a_coherent_transitioned_ledger() {
    let files = corpus(&repo_root());
    if files.is_empty() {
        skip("No .rcx under ron-data/replays/.");
        return;
    }

    let mut checksummed = 0usize;
    for path in files {
        let Ok(replay) = Replay::open(&path) else {
            continue;
        };
        if replay.checksum_packets == 0 {
            continue;
        }
        checksummed += 1;
        let sim = WorldSim::from_replay(&replay);
        let map = sim
            .initial_world
            .as_ref()
            .expect("checksum-bearing procedural replay must reconstruct a World");
        let ledger = map
            .ownership
            .as_ref()
            .expect("production replay World must install ownership");
        assert!(map.ownership_is_coherent(), "{}", path.display());
        assert_eq!(ledger.snapshot().checksum, map.world.checksum_sections());
        assert!(
            ledger.sources().iter().any(|source| matches!(
                source,
                WorldByteSource::ExactPortTransition { proof_document, .. }
                    if *proof_document == PROOF_DOCUMENT
            )),
            "{}: continent execution must produce a transition",
            path.display()
        );
        assert!(
            map.exact_sourced_walked_bytes() > 76,
            "{}: changed generator bytes must advance exact coverage",
            path.display()
        );

        for section in WorldSection::all() {
            let bytes = ledger.snapshot().section(section).len();
            for offset in 0..bytes {
                if matches!(
                    ledger.owner_at(section, offset),
                    Some(WorldByteSource::ExactPortTransition { proof_document, .. })
                        if *proof_document == PROOF_DOCUMENT
                ) {
                    assert!(
                        matches!(section, WorldSection::StartArrays | WorldSection::WData),
                        "{}: transition owner escaped the admitted sections into {section:?}+{offset}",
                        path.display()
                    );
                }
            }
        }
    }
    assert_eq!(checksummed, 21, "frozen local checksum-bearing corpus");
}

#[test]
fn late_owner_shape_failure_rolls_back_the_real_replay_transaction() {
    let path = repo_root().join("ron-data/replays/multi/Playback___2025.02.10_21_26_50__Mon_.rcx");
    if !path.is_file() {
        skip("The supported Mediterranean replay is absent.");
        return;
    }
    let replay = Replay::open(&path).expect("supported replay must decode");
    let root = ron_data_root_for_replay(&replay.path).expect("replay must have a ron-data owner");
    let style =
        MapStyleStaticData::load_from_ron_data(&root, replay.initial.info.settings.map_style)
            .expect("installed map style must parse");
    let mut plan = replay
        .initial
        .reconstruct_items_with_style(style)
        .expect("style must match replay");
    let mut map = replay
        .initial
        .reconstruct_world()
        .expect("procedural replay must reconstruct a World");

    // Give the otherwise-unowned StartArrays section an exact prior owner.
    // The real continent transaction will wipe and regrow it to a different
    // length, exercising the fail-closed shape guard after continent execution.
    let input = map.checksum.full;
    map.world.add_starting_location(WCoord(8), WCoord(9));
    let output = map.world.checksum_sections().full;
    let ledger = map.ownership.as_mut().unwrap();
    ledger
        .advance_exact_port(
            &map.world,
            ExactPortTransitionProof {
                entry_va: 0x006b_2de0,
                resume_va: 0x006b_3019,
                implementation_sha256: [0x31; 32],
                receipt_sha256: [0x41; 32],
                proof_document: "tests/replay_world_owner_transitions.rs",
                input_checksum: input,
                output_checksum: output,
                allowed_sections: WorldSectionMask::only(WorldSection::StartArrays),
            },
        )
        .unwrap();
    map.checksum = ledger.snapshot().checksum.clone();
    map.sourced_walked_bytes = ledger.coverage().owned_bytes as u64;
    assert!(map.ownership_is_coherent());

    let map_before = map.clone();
    let plan_before = plan.clone();
    let error = plan
        .advance_continent_prefix_with_tilesets(&mut map, &root.join("tilesets.xml"))
        .unwrap_err();
    assert!(matches!(
        error,
        InitialItemReconstructionError::WorldOwnership(
            don_replay::replay_world_owner_transitions::ReplayWorldOwnerTransitionError::Ownership(
                WorldOwnerError::OwnedSectionShapeMutation {
                    section: WorldSection::StartArrays,
                }
            )
        )
    ));
    assert_eq!(plan, plan_before);
    assert_eq!(map.checksum, map_before.checksum);
    assert_eq!(map.sourced_walked_bytes, map_before.sourced_walked_bytes);
    assert_eq!(map.ownership, map_before.ownership);
    assert_eq!(map.generation_regions, map_before.generation_regions);
    assert_eq!(
        format!("{:?}", map.world),
        format!("{:?}", map_before.world)
    );
}
