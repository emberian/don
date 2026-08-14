// SPDX-License-Identifier: GPL-3.0-or-later

use don_replay::setup_2024_frame379::REPLAY_FILE_SHA256;
use don_replay::setup_2024_phase32_attrition::{
    bind_and_prepare_golden_phase32_attrition, golden_phase32_attrition_composition_digest,
    GoldenPhase32AttritionBindError, GoldenPhase32AttritionCapture,
    GoldenPhase32AttritionCaptureSource,
};
use don_replay::world_owner_frontier::sha256;
use don_sim::systems::golden_phase32_attrition::{
    commit_golden_phase32_attrition, expected_golden_actor_o, store_object_coord,
    GoldenPhase32AttritionCone, GoldenPhase32UnitIdentity,
};
use don_sim::systems::map_terrain::{Coord, WCoord};
use don_sim::systems::save_load::{save_sim, SaveError};
use don_sim::systems::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256;
use don_sim::tick::Sim;
use don_sim::world::Handle;

fn canonical_sim(frame: i32, territory_owner: i8) -> (Sim, Handle) {
    let mut sim = Sim::new(0x2024, 20);
    for type_index in 200..207 {
        sim.spawn_unit(0, type_index, 0, 0, 4).unwrap();
    }
    sim.world.frame = frame;
    sim.vic_match.frame = frame;
    let o = expected_golden_actor_o(frame).unwrap();
    let row = sim.world.unit_row_at(0, i32::from(o)).unwrap();
    let handle = sim.world.handle_at_row(row).unwrap();
    let x = 6 * 768 + 100;
    let y = 7 * 768 + 200;
    sim.world
        .set_pos(row, store_object_coord(x), store_object_coord(y));
    sim.world.units.set_unit_masks(row, 0x0040_0081);
    sim.world.units.set_unit_masks2(row, 0x0004_0003);
    sim.world.units.attrition_mut()[row] = 48;
    let wi = sim.map.world.w_index(
        WCoord::from_coord(Coord(x)).0,
        WCoord::from_coord(Coord(y)).0,
    );
    sim.map.world.wdata[wi].who = territory_owner;
    (sim, handle)
}

fn capture(sim: &Sim, handle: Handle) -> GoldenPhase32AttritionCapture {
    let row = sim.world.row_of(handle).unwrap();
    let stored_x = sim.world.units.x_internal()[row];
    let stored_y = sim.world.units.y_internal()[row];
    let x = don_sim::systems::borders_fog::deobf(stored_x as u32);
    let y = don_sim::systems::borders_fog::deobf(stored_y as u32);
    let wx = WCoord::from_coord(Coord(x)).0;
    let wy = WCoord::from_coord(Coord(y)).0;
    let cell_index = sim.map.world.w_index(wx, wy);
    let terrain_checksum_image = sim.map.world.checksum_image().0;
    let mut capture = GoldenPhase32AttritionCapture {
        revision: 9,
        composition_digest: [0; 32],
        source: GoldenPhase32AttritionCaptureSource::SupportedRetailProcessAtUnitAttritionGate,
        replay_file_sha256: REPLAY_FILE_SHA256,
        executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
        frame2_post_tick_authority_digest: [0x02; 32],
        canonical_sim_sha256: sha256(&save_sim(sim).unwrap()),
        frame: sim.world.frame,
        unit: GoldenPhase32UnitIdentity {
            handle,
            who: sim.world.units.get_who(row),
            o: sim.world.units.o()[row],
            uid: sim.world.units.get_uid(row),
            type_index: sim.world.unit_type_id(row).unwrap(),
        },
        stored_x,
        stored_y,
        x,
        y,
        wx,
        wy,
        cell_index,
        territory_owner: sim.map.world.wdata[cell_index].who,
        terrain_checksum: sim.map.world.checksum_sections(),
        terrain_checksum_image_sha256: sha256(&terrain_checksum_image),
        terrain_checksum_image,
        object_world_digest: sim.world.digest(),
        victory_neutral_attrition: sim.vic_leaders.slots[0].neutral_attrition,
        step8_neutral_attrition: sim.step8.leaders[0].neutral_attrition,
        scenario_attrition_free_points_empty: true,
    };
    capture.composition_digest = golden_phase32_attrition_composition_digest(&capture);
    capture
}

#[test]
fn exact_second_stage_capture_binds_and_commits_detached() {
    let (mut sim, handle) = canonical_sim(32, -1);
    let capture = capture(&sim, handle);
    let prepared = bind_and_prepare_golden_phase32_attrition(&sim, &capture).unwrap();
    assert_eq!(
        prepared.cone(),
        GoldenPhase32AttritionCone::UnownedZeroNeutralAttrition
    );
    let receipt = commit_golden_phase32_attrition(&mut sim, prepared).unwrap();
    assert_eq!(receipt.authority_revision, 9);
}

#[test]
fn composition_digest_covers_position_cell_and_leader_facts() {
    let (sim, handle) = canonical_sim(31, 0);
    let mut capture = capture(&sim, handle);
    capture.victory_neutral_attrition = 1;
    assert_eq!(
        bind_and_prepare_golden_phase32_attrition(&sim, &capture).unwrap_err(),
        GoldenPhase32AttritionBindError::CompositionDigestMismatch
    );
}

#[test]
fn canonical_sim_and_full_terrain_images_are_mandatory() {
    let (sim, handle) = canonical_sim(30, 0);
    let mut missing_snapshot = capture(&sim, handle);
    missing_snapshot.canonical_sim_sha256 = [0; 32];
    missing_snapshot.composition_digest =
        golden_phase32_attrition_composition_digest(&missing_snapshot);
    assert_eq!(
        bind_and_prepare_golden_phase32_attrition(&sim, &missing_snapshot).unwrap_err(),
        GoldenPhase32AttritionBindError::MissingCanonicalSnapshotDigest
    );

    let mut changed_terrain = capture(&sim, handle);
    changed_terrain.terrain_checksum_image.push(1);
    changed_terrain.composition_digest =
        golden_phase32_attrition_composition_digest(&changed_terrain);
    assert_eq!(
        bind_and_prepare_golden_phase32_attrition(&sim, &changed_terrain).unwrap_err(),
        GoldenPhase32AttritionBindError::TerrainImageDigestMismatch
    );
}

#[test]
fn later_capture_requires_a_separate_frame2_chronology_attestation() {
    let (sim, handle) = canonical_sim(29, 0);
    let mut capture = capture(&sim, handle);
    capture.frame2_post_tick_authority_digest = [0; 32];
    capture.composition_digest = golden_phase32_attrition_composition_digest(&capture);
    assert_eq!(
        bind_and_prepare_golden_phase32_attrition(&sim, &capture).unwrap_err(),
        GoldenPhase32AttritionBindError::MissingFrame2Chronology
    );
}

#[test]
fn a_changed_sim_is_stale_before_any_core_write() {
    let (mut sim, handle) = canonical_sim(28, 0);
    let capture = capture(&sim, handle);
    let row = sim.world.row_of(handle).unwrap();
    sim.world.units.attrition_mut()[row] = 99;
    assert_eq!(
        bind_and_prepare_golden_phase32_attrition(&sim, &capture).unwrap_err(),
        GoldenPhase32AttritionBindError::CanonicalSnapshotMismatch
    );
}

#[test]
fn save_failure_is_not_flattened_into_a_snapshot_mismatch() {
    let (mut sim, handle) = canonical_sim(27, 0);
    let capture = capture(&sim, handle);
    sim.unit_type.pop();
    assert_eq!(
        bind_and_prepare_golden_phase32_attrition(&sim, &capture).unwrap_err(),
        GoldenPhase32AttritionBindError::Snapshot(SaveError::Invalid("unit side-store lengths"))
    );
}
