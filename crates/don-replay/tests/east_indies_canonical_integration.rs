//! Real replay/localizer/owner-transition regression for the complete East Indies virtual.

use don_replay::harness::WorldSim;
use don_replay::replay::Replay;
use don_replay::replay_world_owner_transitions::PROOF_DOCUMENT;
use don_replay::world_owner_frontier::WorldByteSource;
use don_sim::systems::map_terrain::WorldSection;
use std::path::{Path, PathBuf};

fn replay_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .join("ron-data/replays/multi/Playback___2024.04.10_17_05_19__Wed_.rcx")
}

#[test]
fn native_return_advances_the_localizer_and_owner_chain() {
    let path = replay_path();
    if !path.is_file() {
        eprintln!("\n  SKIPPED — NOT A PASS. The checksum-bearing East Indies replay is absent.\n");
        return;
    }
    let replay = Replay::open(&path).expect("East Indies replay must decode");
    assert!(replay.checksum_packets > 0);
    assert_eq!(replay.initial.info.settings.map_style, 18);

    let sim = WorldSim::from_replay(&replay);
    let continent = sim
        .initial_continent
        .as_ref()
        .expect("East Indies must execute its complete style virtual");
    assert!(matches!(
        continent.stop,
        don_replay::continent::ContinentStop::HookComplete {
            next_va: don_replay::continent::REGIONS_CLEAR_ALL_VA
        }
    ));
    assert!(continent
        .direct_rng_sites
        .contains(&don_replay::continent::EAST_INDIES_NONPLAYER_ISLANDS_VA));
    assert_eq!(
        (
            continent.region_seeds.len(),
            continent.region_growths.len(),
            continent.grow_valid_calls.len(),
            continent.direct_rng_sites.len(),
            continent.rng_final as u32,
        ),
        (17, 23, 11, 37_018, 0xab88_731c),
        "the real recording pins all 11 island seeds/growths and their complete RNG chronology"
    );
    let tail = continent
        .east_indies_tail
        .as_ref()
        .expect("the canonical receipt must retain native tail semantics");
    assert_eq!(
        (
            tail.requested_islands,
            tail.placed_islands,
            tail.remaining_islands,
            tail.return_reason,
            tail.rng_final as u32,
        ),
        (
            11,
            11,
            0,
            don_replay::continent::EastIndiesTailReturn::AllIslandsPlaced,
            0xab88_731c,
        )
    );
    assert!(sim
        .initial_items
        .as_ref()
        .and_then(|items| items.post_continent.as_ref())
        .is_some());

    let map = sim
        .initial_world
        .as_ref()
        .expect("East Indies must retain the transitioned World");
    let ledger = map
        .ownership
        .as_ref()
        .expect("production replay World installs the byte-owner ledger");
    assert!(map.ownership_is_coherent());
    assert_eq!(ledger.snapshot().checksum, map.world.checksum_sections());
    assert!(ledger.sources().iter().any(|source| matches!(
        source,
        WorldByteSource::ExactPortTransition {
            entry_va: 0x0069_7540,
            resume_va: don_replay::continent::REGIONS_CLEAR_ALL_VA,
            proof_document,
            ..
        } if *proof_document == PROOF_DOCUMENT
    )));

    // This is the localizer's earliest lawful-candidate rule, not a claim that
    // retail differs here: the six-entry array length changed at byte zero,
    // while its equal zero neighbour remains deliberately unowned.
    assert!(ledger.owner_at(WorldSection::StartArrays, 0).is_some());
    assert!(ledger.owner_at(WorldSection::StartArrays, 1).is_none());
    assert_eq!(
        ledger.coverage().owned_bytes as u64,
        map.exact_sourced_walked_bytes()
    );
}
