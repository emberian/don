// SPDX-License-Identifier: GPL-3.0-or-later
//! Retail replay evidence for the initial goody reconstruction boundary.

use don_replay::checksum::Channel;
use don_replay::harness::{Simulation, WorldSim};
use don_replay::initial::{
    parse_initial_state, InitialItemBoundary, InitialItemReconstructionError,
    ABSENT_REPLAY_ITEM_INPUTS,
};
use don_replay::replay::{load_payload, Replay};
use don_sim::item_runtime::ItemRuntimeError;
use std::path::{Path, PathBuf};

fn supported_replay() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("ron-data/replays/multi/Playback___2025.02.10_21_26_50__Mon_.rcx")
}

fn open_supported() -> Option<Replay> {
    let path = supported_replay();
    if !path.is_file() {
        eprintln!(
            "\n  SKIPPED — NOT A PASS. The supported retail replay is absent; \
             initial-item reconstruction was not exercised.\n"
        );
        return None;
    }
    Some(Replay::open(&path).expect("supported retail replay must decode"))
}

#[test]
fn supported_replay_executes_to_the_exact_external_map_style_boundary() {
    let Some(rep) = open_supported() else {
        return;
    };
    let plan = rep.initial.reconstruct_items();
    assert_eq!(
        plan.boundary,
        InitialItemBoundary::MapStyleContentUnavailable {
            map_style: rep.initial.info.settings.map_style
        }
    );
    assert!(plan.rules.is_some(), "static replay Rules were admitted");
    assert_eq!(plan.scalar_source_bytes(), 10);
    assert_eq!(plan.absent_replay_inputs(), &ABSENT_REPLAY_ITEM_INPUTS);
    assert!(ABSENT_REPLAY_ITEM_INPUTS
        .iter()
        .all(|missing| missing.replay_bytes == 0));

    // Exact decompressed-payload evidence for this supported specimen. The
    // 24 bytes between the parsed Game prefix and the admitted static Rules
    // section cannot be a generated 100x100 map/item/RNG snapshot.
    let sources = plan.inputs.sources;
    assert_eq!((sources.seed.offset, sources.seed.bytes), (0x3e, 4));
    assert_eq!(
        (sources.map_style.offset, sources.map_style.bytes),
        (0x53, 1)
    );
    assert_eq!((sources.map_size.offset, sources.map_size.bytes), (0x54, 1));
    assert_eq!(rep.initial.bytes_walked, 0x392);
    assert_eq!(plan.rules.unwrap().serialized_offset, 0x3aa);
    assert_eq!(
        plan.rules.unwrap().serialized_offset - rep.initial.bytes_walked,
        24
    );

    let sim = WorldSim::from_replay(&rep);
    assert_eq!(sim.initial_items.as_ref(), Some(&plan));
    assert_eq!(
        sim.initial_item_error,
        Some(InitialItemReconstructionError::Blocked(plan.boundary))
    );
    assert_eq!(
        sim.world.items_channel(),
        Err(ItemRuntimeError::Unavailable)
    );
    assert!(!sim.installed_channels()[Channel::Items as usize]);
}

#[test]
fn changing_the_source_style_byte_changes_the_plan_without_installing_items() {
    let Some(rep) = open_supported() else {
        return;
    };
    let mut payload = load_payload(&rep.path).unwrap();
    let at = rep.initial.worldgen_sources.map_style.offset;
    let original_style = payload[at];
    payload[at] = original_style ^ 1;

    let changed = parse_initial_state(&payload).expect("style selector mutation stays structural");
    let plan = changed.reconstruct_items();
    assert_eq!(plan.inputs.sources.map_style.offset, at);
    assert_eq!(plan.inputs.map_style, original_style ^ 1);
    assert_eq!(
        plan.boundary,
        InitialItemBoundary::MapStyleContentUnavailable {
            map_style: original_style ^ 1
        }
    );

    let mut map = changed.reconstruct_world().unwrap();
    let mut sim = don_sim::World::with_capacity(16, 1);
    assert_eq!(
        plan.apply(&mut sim, &mut map.world),
        Err(InitialItemReconstructionError::Blocked(plan.boundary))
    );
    assert_eq!(sim.items_channel(), Err(ItemRuntimeError::Unavailable));
}
