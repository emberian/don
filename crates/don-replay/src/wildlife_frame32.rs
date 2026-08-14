// SPDX-License-Identifier: GPL-3.0-or-later
//! Capture admission for the supported replay's first wildlife cadence at frame 32.
//!
//! `.rcx` bytes do not contain the generated map image, post-setup RNG, owner-9 sparse Unit
//! registry, or `SubObjectData::is_animal()` answers. This module therefore admits only one
//! coherent supported-process snapshot. The exact wildlife transaction remains detached in
//! `don-sim`; binding a capture does not install it in `Sim::do_frame`.

#![forbid(unsafe_code)]

use don_sim::systems::save_load::{save_sim, SaveError};
use don_sim::systems::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256;
use don_sim::systems::wildlife_spawn_frontier::{
    prepare_wildlife_zero_spawn, WildlifeFrameAuthority, WildlifeFrameError,
    WildlifeOwner9UnitFact, WILDLIFE_FRAME,
};
use don_sim::tick::Sim;

use crate::setup_2024_frame379::REPLAY_FILE_SHA256;
use crate::world_owner_frontier::sha256;

const COMPOSITION_DOMAIN: &[u8] = b"don.wildlife.frame32.capture.v1\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame32WildlifeCaptureSource {
    /// One stopped main-thread capture at the entry of the frame-32 wildlife block in the
    /// supported retail process, after the preceding rotated object walk completed.
    SupportedRetailProcessAtWildlifeBlock,
}

/// All dynamic inputs required before the bounded wildlife prefix may run.
///
/// `canonical_sim_sha256` binds the canonical object/RNG owners. The map is bound both by its
/// section ledger and by SHA-256 of the full byte stream handed to the retail checksum visitor;
/// neither a lone recorded Adler word nor replay settings satisfy this contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame32WildlifeCapture {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: Frame32WildlifeCaptureSource,
    pub replay_file_sha256: [u8; 32],
    pub executable_sha256: [u8; 32],
    pub canonical_sim_sha256: [u8; 32],
    /// Captured bytes handed to the retail World checksum visitor, not reconstructed from the
    /// recorded checksum word.
    pub map_checksum_image: Vec<u8>,
    pub map_checksum_image_sha256: [u8; 32],
    pub frame: i32,
    pub random_state: i32,
    pub map_checksum: don_sim::systems::map_terrain::WorldChecksum,
    pub object_world_digest: u64,
    /// Exact sparse-slot order for every live owner-9 Unit below the captured Unit mark.
    pub owner9_units: Vec<WildlifeOwner9UnitFact>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame32WildlifeBindError {
    MissingRevision,
    MissingCompositionDigest,
    CompositionDigestMismatch,
    WrongReplayFile,
    UnsupportedExecutable,
    MissingCanonicalSnapshotDigest,
    MissingMapImageDigest,
    WrongFrame { expected: i32, actual: i32 },
    Snapshot(SaveError),
    CanonicalSnapshotMismatch,
    RandomStateMismatch,
    MapChecksumMismatch,
    MapChecksumImageMismatch,
    ObjectWorldDigestMismatch,
    Authority(WildlifeFrameError),
}

fn put_bytes(image: &mut Vec<u8>, bytes: &[u8]) {
    image.extend_from_slice(bytes);
}

/// Stable digest over every explicit capture claim other than the digest field itself.
pub fn frame32_wildlife_composition_digest(capture: &Frame32WildlifeCapture) -> [u8; 32] {
    let mut image = Vec::new();
    put_bytes(&mut image, COMPOSITION_DOMAIN);
    put_bytes(&mut image, &capture.revision.to_le_bytes());
    put_bytes(
        &mut image,
        &[match capture.source {
            Frame32WildlifeCaptureSource::SupportedRetailProcessAtWildlifeBlock => 1,
        }],
    );
    put_bytes(&mut image, &capture.replay_file_sha256);
    put_bytes(&mut image, &capture.executable_sha256);
    put_bytes(&mut image, &capture.canonical_sim_sha256);
    put_bytes(
        &mut image,
        &(capture.map_checksum_image.len() as u64).to_le_bytes(),
    );
    put_bytes(&mut image, &capture.map_checksum_image_sha256);
    put_bytes(&mut image, &capture.frame.to_le_bytes());
    put_bytes(&mut image, &capture.random_state.to_le_bytes());
    for section in &capture.map_checksum.per_section {
        put_bytes(&mut image, &section.adler.to_le_bytes());
        put_bytes(&mut image, &section.bytes.to_le_bytes());
    }
    put_bytes(&mut image, &capture.map_checksum.full.to_le_bytes());
    put_bytes(&mut image, &capture.map_checksum.bytes.to_le_bytes());
    put_bytes(&mut image, &capture.object_world_digest.to_le_bytes());
    put_bytes(
        &mut image,
        &(capture.owner9_units.len() as u64).to_le_bytes(),
    );
    for fact in &capture.owner9_units {
        put_bytes(&mut image, &fact.handle.id.to_le_bytes());
        put_bytes(&mut image, &fact.handle.generation.to_le_bytes());
        put_bytes(&mut image, &fact.o.to_le_bytes());
        put_bytes(&mut image, &fact.uid.to_le_bytes());
        put_bytes(&mut image, &[fact.flags]);
        put_bytes(&mut image, &fact.type_index.to_le_bytes());
        put_bytes(&mut image, &[u8::from(fact.is_animal)]);
    }
    sha256(&image)
}

/// Admit one exact captured frame-32 state as detached wildlife authority.
///
/// A viable WData cell is a valid capture: the core planner returns `SpawnRequired` there and
/// binding succeeds so the typed boundary remains inspectable. Any other core validation error
/// rejects the capture.
pub fn bind_frame32_wildlife_capture(
    sim: &Sim,
    capture: &Frame32WildlifeCapture,
) -> Result<WildlifeFrameAuthority, Frame32WildlifeBindError> {
    if capture.revision == 0 {
        return Err(Frame32WildlifeBindError::MissingRevision);
    }
    if capture.composition_digest == [0; 32] {
        return Err(Frame32WildlifeBindError::MissingCompositionDigest);
    }
    if capture.composition_digest != frame32_wildlife_composition_digest(capture) {
        return Err(Frame32WildlifeBindError::CompositionDigestMismatch);
    }
    if capture.replay_file_sha256 != REPLAY_FILE_SHA256 {
        return Err(Frame32WildlifeBindError::WrongReplayFile);
    }
    if capture.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256 {
        return Err(Frame32WildlifeBindError::UnsupportedExecutable);
    }
    if capture.canonical_sim_sha256 == [0; 32] {
        return Err(Frame32WildlifeBindError::MissingCanonicalSnapshotDigest);
    }
    if capture.map_checksum_image_sha256 == [0; 32] {
        return Err(Frame32WildlifeBindError::MissingMapImageDigest);
    }
    if sha256(&capture.map_checksum_image) != capture.map_checksum_image_sha256 {
        return Err(Frame32WildlifeBindError::MapChecksumImageMismatch);
    }
    if capture.frame != WILDLIFE_FRAME {
        return Err(Frame32WildlifeBindError::WrongFrame {
            expected: WILDLIFE_FRAME,
            actual: capture.frame,
        });
    }
    if sim.world.frame != capture.frame {
        return Err(Frame32WildlifeBindError::WrongFrame {
            expected: capture.frame,
            actual: sim.world.frame,
        });
    }
    if sim.world.random.state() != capture.random_state {
        return Err(Frame32WildlifeBindError::RandomStateMismatch);
    }
    if sim.map.world.checksum_sections() != capture.map_checksum {
        return Err(Frame32WildlifeBindError::MapChecksumMismatch);
    }
    if sim.map.world.checksum_image().0 != capture.map_checksum_image {
        return Err(Frame32WildlifeBindError::MapChecksumImageMismatch);
    }
    if sim.world.digest() != capture.object_world_digest {
        return Err(Frame32WildlifeBindError::ObjectWorldDigestMismatch);
    }
    let snapshot = save_sim(sim).map_err(Frame32WildlifeBindError::Snapshot)?;
    if sha256(&snapshot) != capture.canonical_sim_sha256 {
        return Err(Frame32WildlifeBindError::CanonicalSnapshotMismatch);
    }

    let authority = WildlifeFrameAuthority {
        revision: capture.revision,
        composition_digest: capture.composition_digest,
        frame: capture.frame,
        random_state: capture.random_state,
        map_checksum: capture.map_checksum.clone(),
        map_checksum_image: capture.map_checksum_image.clone(),
        object_world_digest: capture.object_world_digest,
        owner9_units: capture.owner9_units.clone(),
    };
    match prepare_wildlife_zero_spawn(&sim.world, &sim.map.world, &sim.unit_type, &authority) {
        Ok(_) | Err(WildlifeFrameError::SpawnRequired(_)) => Ok(authority),
        Err(error) => Err(Frame32WildlifeBindError::Authority(error)),
    }
}
