// SPDX-License-Identifier: GPL-3.0-or-later
//! Retail-capture binder for the supported replay's frames-26-through-32 attrition prefix.
//!
//! This is an optional post-frame-2 oracle, not part of the minimal setup-to-frame-2 bundle.
//! The replay file does not serialize Unit positions, the generated WData owner plane, or the
//! live neutral-attrition mirrors.  A capture must therefore bind one whole canonical `Sim`, the
//! complete terrain checksum image, and the reached generational Unit identity before the
//! detached `don-sim` transaction can be prepared.

#![forbid(unsafe_code)]

use don_sim::systems::golden_phase32_attrition::{
    prepare_golden_phase32_attrition, GoldenPhase32AttritionAuthority, GoldenPhase32AttritionError,
    GoldenPhase32AttritionSource, GoldenPhase32UnitIdentity, PreparedGoldenPhase32Attrition,
};
use don_sim::systems::map_terrain::WorldChecksum;
use don_sim::systems::save_load::{save_sim, SaveError};
use don_sim::systems::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256;
use don_sim::tick::Sim;

use crate::setup_2024_frame379::REPLAY_FILE_SHA256;
use crate::world_owner_frontier::sha256;

const COMPOSITION_DOMAIN: &[u8] = b"don.replay.golden.phase32.attrition.capture.v1\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenPhase32AttritionCaptureSource {
    SupportedRetailProcessAtUnitAttritionGate,
}

/// Standalone second-stage capture for one first-cadence Unit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenPhase32AttritionCapture {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: GoldenPhase32AttritionCaptureSource,
    pub replay_file_sha256: [u8; 32],
    pub executable_sha256: [u8; 32],
    /// Provenance pointer to the separately attested frame-2 post-tick authority.  It is not
    /// authorization for frame 26: unowned frames 2..25 intervene, so the immediate
    /// `canonical_sim_sha256` below remains mandatory and independently source-backed.
    pub frame2_post_tick_authority_digest: [u8; 32],
    pub canonical_sim_sha256: [u8; 32],
    pub frame: i32,
    pub unit: GoldenPhase32UnitIdentity,
    pub stored_x: i32,
    pub stored_y: i32,
    pub x: i32,
    pub y: i32,
    pub wx: i32,
    pub wy: i32,
    pub cell_index: usize,
    pub territory_owner: i8,
    pub terrain_checksum: WorldChecksum,
    pub terrain_checksum_image: Vec<u8>,
    pub terrain_checksum_image_sha256: [u8; 32],
    pub object_world_digest: u64,
    pub victory_neutral_attrition: i32,
    pub step8_neutral_attrition: i32,
    pub scenario_attrition_free_points_empty: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoldenPhase32AttritionBindError {
    MissingRevision,
    MissingCompositionDigest,
    CompositionDigestMismatch,
    WrongReplayFile,
    UnsupportedExecutable,
    MissingFrame2Chronology,
    MissingCanonicalSnapshotDigest,
    MissingTerrainImageDigest,
    TerrainImageDigestMismatch,
    Snapshot(SaveError),
    CanonicalSnapshotMismatch,
    Authority(GoldenPhase32AttritionError),
}

fn put(image: &mut Vec<u8>, bytes: &[u8]) {
    image.extend_from_slice(bytes);
}

/// SHA-256 composition identity over every explicit capture claim except the digest itself.
pub fn golden_phase32_attrition_composition_digest(
    capture: &GoldenPhase32AttritionCapture,
) -> [u8; 32] {
    let mut image = Vec::new();
    put(&mut image, COMPOSITION_DOMAIN);
    put(&mut image, &capture.revision.to_le_bytes());
    put(
        &mut image,
        &[match capture.source {
            GoldenPhase32AttritionCaptureSource::SupportedRetailProcessAtUnitAttritionGate => 1,
        }],
    );
    put(&mut image, &capture.replay_file_sha256);
    put(&mut image, &capture.executable_sha256);
    put(&mut image, &capture.frame2_post_tick_authority_digest);
    put(&mut image, &capture.canonical_sim_sha256);
    put(&mut image, &capture.frame.to_le_bytes());
    put(&mut image, &capture.unit.handle.id.to_le_bytes());
    put(&mut image, &capture.unit.handle.generation.to_le_bytes());
    put(&mut image, &[capture.unit.who]);
    put(&mut image, &capture.unit.o.to_le_bytes());
    put(&mut image, &capture.unit.uid.to_le_bytes());
    put(&mut image, &capture.unit.type_index.to_le_bytes());
    for value in [
        capture.stored_x,
        capture.stored_y,
        capture.x,
        capture.y,
        capture.wx,
        capture.wy,
    ] {
        put(&mut image, &value.to_le_bytes());
    }
    put(&mut image, &(capture.cell_index as u64).to_le_bytes());
    put(&mut image, &[capture.territory_owner as u8]);
    for section in &capture.terrain_checksum.per_section {
        put(&mut image, &section.adler.to_le_bytes());
        put(&mut image, &section.bytes.to_le_bytes());
    }
    put(&mut image, &capture.terrain_checksum.full.to_le_bytes());
    put(&mut image, &capture.terrain_checksum.bytes.to_le_bytes());
    put(
        &mut image,
        &(capture.terrain_checksum_image.len() as u64).to_le_bytes(),
    );
    put(&mut image, &capture.terrain_checksum_image_sha256);
    put(&mut image, &capture.object_world_digest.to_le_bytes());
    put(&mut image, &capture.victory_neutral_attrition.to_le_bytes());
    put(&mut image, &capture.step8_neutral_attrition.to_le_bytes());
    put(
        &mut image,
        &[u8::from(capture.scenario_attrition_free_points_empty)],
    );
    sha256(&image)
}

/// Bind and immediately prepare one detached exact transaction.
///
/// Returning a prepared transaction is intentional: a capture that binds but reaches foreign
/// territory is still red at the core's typed selection/supply boundary and therefore does not
/// masquerade as installed progress.
pub fn bind_and_prepare_golden_phase32_attrition(
    sim: &Sim,
    capture: &GoldenPhase32AttritionCapture,
) -> Result<PreparedGoldenPhase32Attrition, GoldenPhase32AttritionBindError> {
    if capture.revision == 0 {
        return Err(GoldenPhase32AttritionBindError::MissingRevision);
    }
    if capture.composition_digest == [0; 32] {
        return Err(GoldenPhase32AttritionBindError::MissingCompositionDigest);
    }
    if capture.composition_digest != golden_phase32_attrition_composition_digest(capture) {
        return Err(GoldenPhase32AttritionBindError::CompositionDigestMismatch);
    }
    if capture.replay_file_sha256 != REPLAY_FILE_SHA256 {
        return Err(GoldenPhase32AttritionBindError::WrongReplayFile);
    }
    if capture.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256 {
        return Err(GoldenPhase32AttritionBindError::UnsupportedExecutable);
    }
    if capture.frame2_post_tick_authority_digest == [0; 32] {
        return Err(GoldenPhase32AttritionBindError::MissingFrame2Chronology);
    }
    if capture.canonical_sim_sha256 == [0; 32] {
        return Err(GoldenPhase32AttritionBindError::MissingCanonicalSnapshotDigest);
    }
    if capture.terrain_checksum_image_sha256 == [0; 32] {
        return Err(GoldenPhase32AttritionBindError::MissingTerrainImageDigest);
    }
    if sha256(&capture.terrain_checksum_image) != capture.terrain_checksum_image_sha256 {
        return Err(GoldenPhase32AttritionBindError::TerrainImageDigestMismatch);
    }
    let snapshot = save_sim(sim).map_err(GoldenPhase32AttritionBindError::Snapshot)?;
    if sha256(&snapshot) != capture.canonical_sim_sha256 {
        return Err(GoldenPhase32AttritionBindError::CanonicalSnapshotMismatch);
    }
    let authority = GoldenPhase32AttritionAuthority {
        revision: capture.revision,
        composition_digest: capture.composition_digest,
        source: GoldenPhase32AttritionSource::SupportedRetailProcessAtUnitAttritionGate,
        canonical_sim_image: snapshot,
        frame: capture.frame,
        unit: capture.unit,
        stored_x: capture.stored_x,
        stored_y: capture.stored_y,
        x: capture.x,
        y: capture.y,
        wx: capture.wx,
        wy: capture.wy,
        cell_index: capture.cell_index,
        territory_owner: capture.territory_owner,
        terrain_checksum: capture.terrain_checksum.clone(),
        terrain_checksum_image: capture.terrain_checksum_image.clone(),
        object_world_digest: capture.object_world_digest,
        victory_neutral_attrition: capture.victory_neutral_attrition,
        step8_neutral_attrition: capture.step8_neutral_attrition,
        scenario_attrition_free_points_empty: capture.scenario_attrition_free_points_empty,
    };
    prepare_golden_phase32_attrition(sim, &authority)
        .map_err(GoldenPhase32AttritionBindError::Authority)
}
