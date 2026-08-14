// SPDX-License-Identifier: GPL-3.0-or-later

use don_sim::rng::Random;
use don_sim::systems::map_terrain::World as TerrainWorld;
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::systems::wildlife_spawn_frontier::{
    commit_wildlife_zero_spawn, prepare_wildlife_zero_spawn, WildlifeAxis, WildlifeFrameAuthority,
    WildlifeFrameError, WildlifeOwner9UnitFact, OBJECTS_INIT_UNIT_VA, RANDOM_HIGH, RANDOM_LOW,
    WILDBIRD_TYPE, WILDLIFE_CELL_FLAG, WILDLIFE_FRAME, WILDLIFE_OWNER,
};
use don_sim::tick::Sim;
use don_sim::world::Handle;

const AUTHORITY_DIGEST: [u8; 32] = [0xa5; 32];

fn authority(sim: &Sim, owner9_units: Vec<WildlifeOwner9UnitFact>) -> WildlifeFrameAuthority {
    WildlifeFrameAuthority {
        revision: 1,
        composition_digest: AUTHORITY_DIGEST,
        frame: WILDLIFE_FRAME,
        random_state: sim.world.random.state(),
        map_checksum: sim.map.world.checksum_sections(),
        map_checksum_image: sim.map.world.checksum_image().0,
        object_world_digest: sim.world.digest(),
        owner9_units,
    }
}

fn owner9_fact(sim: &Sim, handle: Handle, is_animal: bool) -> WildlifeOwner9UnitFact {
    let row = sim.world.row_of(handle).unwrap();
    WildlifeOwner9UnitFact {
        handle,
        o: sim.world.units.o()[row],
        uid: sim.world.units.get_uid(row),
        flags: sim.world.units.get_flags(row),
        type_index: sim.unit_type[row],
        is_animal,
    }
}

#[test]
fn zero_spawn_commits_only_the_eight_conditional_axis_draws() {
    let mut sim = Sim::new(0x1234, 20);
    sim.world.frame = WILDLIFE_FRAME;
    let authority = authority(&sim, vec![]);
    let random_before = sim.world.random.state();
    let object_digest_before = sim.world.digest();
    let map_before = sim.map.world.checksum_sections();

    let mut expected = Random::new(random_before);
    for _ in 0..8 {
        expected.get(RANDOM_LOW, RANDOM_HIGH);
    }
    let prepared =
        prepare_wildlife_zero_spawn(&sim.world, &sim.map.world, &sim.unit_type, &authority)
            .unwrap();
    let receipt = commit_wildlife_zero_spawn(
        &mut sim.world,
        &sim.map.world,
        &sim.unit_type,
        &authority,
        prepared,
    )
    .unwrap();

    assert!(receipt.validates());
    assert_eq!(receipt.quota, 4);
    assert_eq!(receipt.existing_wildbirds, 0);
    assert_eq!(receipt.attempts, 4);
    assert_eq!(receipt.candidates.len(), 4);
    assert_eq!(sim.world.random.state(), expected.state());
    assert_eq!(sim.world.digest(), object_digest_before);
    assert_eq!(sim.map.world.checksum_sections(), map_before);
}

#[test]
fn viable_first_cell_returns_the_exact_unowned_child_without_publishing_rng() {
    let mut sim = Sim::new(0x2345, 20);
    sim.world.frame = WILDLIFE_FRAME;
    let random_before = sim.world.random.state();
    let mut expected = sim.world.random;
    let x = expected.get(RANDOM_LOW, RANDOM_HIGH) % sim.map.world.xs;
    let y = expected.get(RANDOM_LOW, RANDOM_HIGH) % sim.map.world.ys;
    sim.map.world.wdata[(y * sim.map.world.xs + x) as usize].flags |= WILDLIFE_CELL_FLAG;
    let authority = authority(&sim, vec![]);

    let error = prepare_wildlife_zero_spawn(&sim.world, &sim.map.world, &sim.unit_type, &authority)
        .unwrap_err();
    let WildlifeFrameError::SpawnRequired(required) = error else {
        panic!("expected the typed spawn boundary");
    };

    assert_eq!(required.rejected, vec![]);
    assert_eq!((required.candidate.x, required.candidate.y), (x, y));
    assert_eq!(required.random_state_before, random_before);
    assert_eq!(required.random_state_at_child, expected.state());
    assert_eq!(required.first_unowned_child_va, OBJECTS_INIT_UNIT_VA);
    assert_eq!(required.request.owner, i32::from(WILDLIFE_OWNER));
    assert_eq!(required.request.type_index, WILDBIRD_TYPE);
    assert_eq!(required.request.x, x * 0x300 + 0x180);
    assert_eq!(required.request.y, y * 0x300 + 0x180);
    assert_eq!(sim.world.random.state(), random_before);
}

#[test]
fn stale_map_rejects_commit_without_publishing_the_prepared_rng() {
    let mut sim = Sim::new(0x3456, 20);
    sim.world.frame = WILDLIFE_FRAME;
    let authority = authority(&sim, vec![]);
    let random_before = sim.world.random.state();
    let prepared =
        prepare_wildlife_zero_spawn(&sim.world, &sim.map.world, &sim.unit_type, &authority)
            .unwrap();
    sim.map.world.wdata[0].flags ^= 1;

    let error = commit_wildlife_zero_spawn(
        &mut sim.world,
        &sim.map.world,
        &sim.unit_type,
        &authority,
        prepared,
    )
    .unwrap_err();
    assert_eq!(error, WildlifeFrameError::StaleCanonicalState);
    assert_eq!(sim.world.random.state(), random_before);
}

#[test]
fn exact_owner9_animal_facts_can_satisfy_the_quota_without_draws() {
    let mut sim = Sim::new(0x4567, 20);
    let mut facts = Vec::new();
    for i in 0..4 {
        let handle = sim
            .spawn_unit(usize::from(WILDLIFE_OWNER), WILDBIRD_TYPE, i, i, 0)
            .unwrap();
        facts.push(owner9_fact(&sim, handle, true));
    }
    sim.world.frame = WILDLIFE_FRAME;
    let authority = authority(&sim, facts);
    let random_before = sim.world.random.state();

    let prepared =
        prepare_wildlife_zero_spawn(&sim.world, &sim.map.world, &sim.unit_type, &authority)
            .unwrap();
    let receipt = commit_wildlife_zero_spawn(
        &mut sim.world,
        &sim.map.world,
        &sim.unit_type,
        &authority,
        prepared,
    )
    .unwrap();
    assert_eq!(receipt.existing_wildbirds, 4);
    assert_eq!(receipt.attempts, 0);
    assert_eq!(receipt.random_state_after, random_before);
    assert_eq!(sim.world.random.state(), random_before);
}

#[test]
fn captured_is_animal_answer_is_not_inferred_from_type_402() {
    let mut sim = Sim::new(0x5678, 10);
    let handle = sim
        .spawn_unit(usize::from(WILDLIFE_OWNER), WILDBIRD_TYPE, 0, 0, 0)
        .unwrap();
    sim.world.frame = WILDLIFE_FRAME;
    let authority = authority(&sim, vec![owner9_fact(&sim, handle, false)]);

    let prepared =
        prepare_wildlife_zero_spawn(&sim.world, &sim.map.world, &sim.unit_type, &authority)
            .unwrap();
    let receipt = commit_wildlife_zero_spawn(
        &mut sim.world,
        &sim.map.world,
        &sim.unit_type,
        &authority,
        prepared,
    )
    .unwrap();
    assert_eq!(receipt.quota, 1);
    assert_eq!(receipt.existing_wildbirds, 0);
    assert_eq!(receipt.attempts, 1);
}

#[test]
fn singleton_axis_consumes_no_random_draw() {
    let mut sim = Sim::new(0x6789, 1);
    sim.map.world = TerrainWorld::init_default_rules(1, 100);
    sim.world.frame = WILDLIFE_FRAME;
    let authority = authority(&sim, vec![]);
    let mut expected = sim.world.random;
    expected.get(RANDOM_LOW, RANDOM_HIGH);

    let prepared =
        prepare_wildlife_zero_spawn(&sim.world, &sim.map.world, &sim.unit_type, &authority)
            .unwrap();
    let receipt = commit_wildlife_zero_spawn(
        &mut sim.world,
        &sim.map.world,
        &sim.unit_type,
        &authority,
        prepared,
    )
    .unwrap();
    assert_eq!(receipt.attempts, 1);
    assert_eq!(receipt.candidates[0].x, 0);
    assert_eq!(receipt.candidates[0].x_draw, None);
    assert_eq!(receipt.candidates[0].y_draw.unwrap().axis, WildlifeAxis::Y);
    assert_eq!(sim.world.random.state(), expected.state());
}

#[test]
fn authority_is_exactly_frame_32() {
    let mut sim = Sim::new(0x789a, 20);
    sim.world.frame = 64;
    let mut authority = authority(&sim, vec![]);
    authority.frame = 64;

    assert_eq!(
        prepare_wildlife_zero_spawn(&sim.world, &sim.map.world, &sim.unit_type, &authority)
            .unwrap_err(),
        WildlifeFrameError::WrongFrame(64)
    );
}

#[test]
fn save_resume_rebinds_the_same_zero_spawn_transaction() {
    let mut direct = Sim::new(0x89ab, 20);
    direct.world.frame = WILDLIFE_FRAME;
    direct.vic_match.frame = WILDLIFE_FRAME;
    let authority = authority(&direct, vec![]);
    let saved = save_sim(&direct).unwrap();
    let mut resumed = load_sim(&saved).unwrap();

    let direct_prepared = prepare_wildlife_zero_spawn(
        &direct.world,
        &direct.map.world,
        &direct.unit_type,
        &authority,
    )
    .unwrap();
    let direct_receipt = commit_wildlife_zero_spawn(
        &mut direct.world,
        &direct.map.world,
        &direct.unit_type,
        &authority,
        direct_prepared,
    )
    .unwrap();
    let resumed_prepared = prepare_wildlife_zero_spawn(
        &resumed.world,
        &resumed.map.world,
        &resumed.unit_type,
        &authority,
    )
    .unwrap();
    let resumed_receipt = commit_wildlife_zero_spawn(
        &mut resumed.world,
        &resumed.map.world,
        &resumed.unit_type,
        &authority,
        resumed_prepared,
    )
    .unwrap();

    assert_eq!(resumed_receipt, direct_receipt);
    assert_eq!(resumed.world.random, direct.world.random);
    assert_eq!(resumed.world.digest(), direct.world.digest());
    assert_eq!(
        resumed.map.world.checksum_image().0,
        direct.map.world.checksum_image().0
    );
}
