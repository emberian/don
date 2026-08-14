// SPDX-License-Identifier: GPL-3.0-or-later

use don_replay::setup_2024_frame379::REPLAY_FILE_SHA256;
use don_replay::wildlife_frame32::{
    bind_frame32_wildlife_capture, frame32_wildlife_composition_digest, Frame32WildlifeBindError,
    Frame32WildlifeCapture, Frame32WildlifeCaptureSource,
};
use don_replay::world_owner_frontier::sha256;
use don_sim::systems::save_load::save_sim;
use don_sim::systems::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256;
use don_sim::systems::wildlife_spawn_frontier::{
    prepare_wildlife_zero_spawn, WildlifeFrameError, WILDLIFE_CELL_FLAG, WILDLIFE_FRAME,
};
use don_sim::tick::Sim;

fn frame32_sim(seed: u64) -> Sim {
    let mut sim = Sim::new(seed, 20);
    sim.world.frame = WILDLIFE_FRAME;
    sim.vic_match.frame = WILDLIFE_FRAME;
    sim
}

fn capture(sim: &Sim) -> Frame32WildlifeCapture {
    let map_checksum_image = sim.map.world.checksum_image().0;
    let mut capture = Frame32WildlifeCapture {
        revision: 1,
        composition_digest: [0; 32],
        source: Frame32WildlifeCaptureSource::SupportedRetailProcessAtWildlifeBlock,
        replay_file_sha256: REPLAY_FILE_SHA256,
        executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
        canonical_sim_sha256: sha256(&save_sim(sim).unwrap()),
        map_checksum_image_sha256: sha256(&map_checksum_image),
        map_checksum_image,
        frame: WILDLIFE_FRAME,
        random_state: sim.world.random.state(),
        map_checksum: sim.map.world.checksum_sections(),
        object_world_digest: sim.world.digest(),
        owner9_units: vec![],
    };
    capture.composition_digest = frame32_wildlife_composition_digest(&capture);
    capture
}

#[test]
fn exact_supported_snapshot_binds_detached_authority() {
    let sim = frame32_sim(0x1234);
    let capture = capture(&sim);

    let authority = bind_frame32_wildlife_capture(&sim, &capture).unwrap();
    assert_eq!(authority.frame, WILDLIFE_FRAME);
    assert_eq!(authority.map_checksum, sim.map.world.checksum_sections());
    assert!(
        prepare_wildlife_zero_spawn(&sim.world, &sim.map.world, &sim.unit_type, &authority).is_ok()
    );
}

#[test]
fn viable_cell_is_admitted_but_remains_spawn_required() {
    let mut sim = frame32_sim(0x2345);
    let mut random = sim.world.random;
    let x = random.get(0, 0xffff) % sim.map.world.xs;
    let y = random.get(0, 0xffff) % sim.map.world.ys;
    sim.map.world.wdata[(y * sim.map.world.xs + x) as usize].flags |= WILDLIFE_CELL_FLAG;
    let capture = capture(&sim);

    let authority = bind_frame32_wildlife_capture(&sim, &capture).unwrap();
    assert!(matches!(
        prepare_wildlife_zero_spawn(&sim.world, &sim.map.world, &sim.unit_type, &authority),
        Err(WildlifeFrameError::SpawnRequired(_))
    ));
}

#[test]
fn composition_digest_covers_every_capture_claim() {
    let sim = frame32_sim(0x3456);
    let mut capture = capture(&sim);
    capture.random_state ^= 1;

    assert_eq!(
        bind_frame32_wildlife_capture(&sim, &capture).unwrap_err(),
        Frame32WildlifeBindError::CompositionDigestMismatch
    );
}

#[test]
fn exact_rng_is_required_even_with_a_recomputed_contract_digest() {
    let sim = frame32_sim(0x4567);
    let mut capture = capture(&sim);
    capture.random_state ^= 1;
    capture.composition_digest = frame32_wildlife_composition_digest(&capture);

    assert_eq!(
        bind_frame32_wildlife_capture(&sim, &capture).unwrap_err(),
        Frame32WildlifeBindError::RandomStateMismatch
    );
}

#[test]
fn full_map_image_digest_is_mandatory() {
    let sim = frame32_sim(0x5678);
    let mut capture = capture(&sim);
    capture.map_checksum_image_sha256 = [0; 32];
    capture.composition_digest = frame32_wildlife_composition_digest(&capture);

    assert_eq!(
        bind_frame32_wildlife_capture(&sim, &capture).unwrap_err(),
        Frame32WildlifeBindError::MissingMapImageDigest
    );
}
