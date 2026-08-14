// SPDX-License-Identifier: GPL-3.0-or-later

use don_sim::systems::golden_phase32_attrition::{
    commit_golden_phase32_attrition, expected_golden_actor_o, prepare_golden_phase32_attrition,
    store_object_coord, GoldenPhase32AttritionAuthority, GoldenPhase32AttritionCone,
    GoldenPhase32AttritionError, GoldenPhase32AttritionSource, GoldenPhase32UnitIdentity,
    ATTRITION_GATE_CALL_VA, ATTRITION_GATE_FIRST_VA, ATTRITION_GATE_LAST_VA,
    FRIENDLY_TERRITORY_RETURN_VA, PROCESS_ATTRITION_CLEAR_MASK, UNIT_PROCESS_ATTRITION_VA,
    UNIT_PROCESS_VA, UNOWNED_ZERO_NEUTRAL_RETURN_VA,
};
use don_sim::systems::map_terrain::{Coord, WCoord};
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::tick::Sim;
use don_sim::world::Handle;

fn actor_sim(frame: i32, territory_owner: i8) -> (Sim, Handle) {
    let mut sim = Sim::new(0x1234, 20);
    let mut handles = Vec::new();
    for type_index in 100..107 {
        handles.push(sim.spawn_unit(0, type_index, 0, 0, 4).unwrap());
    }
    sim.world.frame = frame;
    sim.vic_match.frame = frame;
    let o = expected_golden_actor_o(frame).unwrap();
    let row = sim.world.unit_row_at(0, i32::from(o)).unwrap();
    let handle = sim.world.handle_at_row(row).unwrap();
    assert_eq!(handle, handles[o as usize]);

    let x = 4 * 768 + 123;
    let y = 5 * 768 + 456;
    sim.world
        .set_pos(row, store_object_coord(x), store_object_coord(y));
    sim.world.units.set_unit_masks(row, 0xdead_beef);
    sim.world
        .units
        .set_unit_masks2(row, 0x0004_0003 | 0x5500_0000);
    sim.world.units.attrition_mut()[row] = 73;
    let wx = WCoord::from_coord(Coord(x)).0;
    let wy = WCoord::from_coord(Coord(y)).0;
    let cell_index = sim.map.world.w_index(wx, wy);
    sim.map.world.wdata[cell_index].who = territory_owner;
    (sim, handle)
}

fn authority(sim: &Sim, handle: Handle) -> GoldenPhase32AttritionAuthority {
    let row = sim.world.row_of(handle).unwrap();
    let stored_x = sim.world.units.x_internal()[row];
    let stored_y = sim.world.units.y_internal()[row];
    let x = don_sim::systems::borders_fog::deobf(stored_x as u32);
    let y = don_sim::systems::borders_fog::deobf(stored_y as u32);
    let wx = WCoord::from_coord(Coord(x)).0;
    let wy = WCoord::from_coord(Coord(y)).0;
    let cell_index = sim.map.world.w_index(wx, wy);
    GoldenPhase32AttritionAuthority {
        revision: 7,
        composition_digest: [0x32; 32],
        source: GoldenPhase32AttritionSource::SupportedRetailProcessAtUnitAttritionGate,
        canonical_sim_image: save_sim(sim).unwrap(),
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
        terrain_checksum_image: sim.map.world.checksum_image().0,
        object_world_digest: sim.world.digest(),
        victory_neutral_attrition: sim.vic_leaders.slots[0].neutral_attrition,
        step8_neutral_attrition: sim.step8.leaders[0].neutral_attrition,
        scenario_attrition_free_points_empty: true,
    }
}

#[test]
fn addresses_masks_and_golden_schedule_are_frozen() {
    assert_eq!(UNIT_PROCESS_VA, 0x0061_0bc0);
    assert_eq!(ATTRITION_GATE_FIRST_VA, 0x0061_15ea);
    assert_eq!(ATTRITION_GATE_CALL_VA, 0x0061_1612);
    assert_eq!(ATTRITION_GATE_LAST_VA, 0x0061_1626);
    assert_eq!(UNIT_PROCESS_ATTRITION_VA, 0x005e_11a0);
    assert_eq!(FRIENDLY_TERRITORY_RETURN_VA, 0x005e_12c5);
    assert_eq!(UNOWNED_ZERO_NEUTRAL_RETURN_VA, 0x005e_12a5);
    assert_eq!(PROCESS_ATTRITION_CLEAR_MASK, 0x0040_0080);
    assert_eq!(
        (26..=32).map(expected_golden_actor_o).collect::<Vec<_>>(),
        [
            Some(6),
            Some(5),
            Some(4),
            Some(3),
            Some(2),
            Some(1),
            Some(0)
        ]
    );
    assert_eq!(expected_golden_actor_o(25), None);
    assert_eq!(expected_golden_actor_o(33), None);
}

#[test]
fn friendly_territory_commits_exact_caller_child_caller_writes() {
    let (mut sim, handle) = actor_sim(26, 0);
    let random_before = sim.world.random.state();
    let authority = authority(&sim, handle);
    let prepared = prepare_golden_phase32_attrition(&sim, &authority).unwrap();
    let before = prepared.before();
    let writes = prepared.writes();

    assert_eq!(
        prepared.cone(),
        GoldenPhase32AttritionCone::FriendlyTerritory
    );
    assert_eq!(writes.unit_masks2_after_call_gate, 0x5500_0003);
    assert_eq!(writes.unit_masks_after_child_entry, 0xdead_be6f);
    assert_eq!(writes.attrition_after_child_entry, 0);
    assert_eq!(writes.unit_masks2_after_child_return, 0x5500_0000);

    let receipt = commit_golden_phase32_attrition(&mut sim, prepared).unwrap();
    let row = sim.world.row_of(handle).unwrap();
    assert_eq!(receipt.before, before);
    assert_eq!(receipt.return_va, FRIENDLY_TERRITORY_RETURN_VA);
    assert_eq!(sim.world.units.get_unit_masks(row), 0xdead_be6f);
    assert_eq!(sim.world.units.get_unit_masks2(row), 0x5500_0000);
    assert_eq!(sim.world.units.attrition()[row], 0);
    assert_eq!(sim.world.random.state(), random_before);
}

#[test]
fn unowned_zero_neutral_uses_the_earlier_exact_return() {
    let (mut sim, handle) = actor_sim(32, -1);
    let authority = authority(&sim, handle);
    let prepared = prepare_golden_phase32_attrition(&sim, &authority).unwrap();
    let receipt = commit_golden_phase32_attrition(&mut sim, prepared).unwrap();

    assert_eq!(
        receipt.cone,
        GoldenPhase32AttritionCone::UnownedZeroNeutralAttrition
    );
    assert_eq!(receipt.return_va, UNOWNED_ZERO_NEUTRAL_RETURN_VA);
}

#[test]
fn foreign_territory_is_typed_red_and_performs_no_prefix_write() {
    let (sim, handle) = actor_sim(31, 1);
    let authority = authority(&sim, handle);
    let before = save_sim(&sim).unwrap();

    assert_eq!(
        prepare_golden_phase32_attrition(&sim, &authority).unwrap_err(),
        GoldenPhase32AttritionError::ForeignTerritoryRequiresSelectionAndSupplyAuthority(1)
    );
    assert_eq!(save_sim(&sim).unwrap(), before);
}

#[test]
fn neutral_mirrors_must_agree_and_be_zero() {
    let (mut disagree, handle) = actor_sim(30, -1);
    disagree.vic_leaders.slots[0].neutral_attrition = 3;
    let disagree_authority = authority(&disagree, handle);
    assert_eq!(
        prepare_golden_phase32_attrition(&disagree, &disagree_authority).unwrap_err(),
        GoldenPhase32AttritionError::NeutralAttritionMirrorMismatch {
            victory: 3,
            step8: 0,
        }
    );

    let (mut nonzero, handle) = actor_sim(30, -1);
    nonzero.vic_leaders.slots[0].neutral_attrition = 3;
    nonzero.step8.leaders[0].neutral_attrition = 3;
    let nonzero_authority = authority(&nonzero, handle);
    assert_eq!(
        prepare_golden_phase32_attrition(&nonzero, &nonzero_authority).unwrap_err(),
        GoldenPhase32AttritionError::NeutralAttritionNonZero(3)
    );
}

#[test]
fn populated_scenario_points_remain_a_typed_boundary() {
    let (sim, handle) = actor_sim(29, 0);
    let mut authority = authority(&sim, handle);
    authority.scenario_attrition_free_points_empty = false;
    assert_eq!(
        prepare_golden_phase32_attrition(&sim, &authority).unwrap_err(),
        GoldenPhase32AttritionError::ScenarioAttritionFreePoints
    );
}

#[test]
fn stale_unit_or_full_world_refuses_atomically() {
    let (mut sim, handle) = actor_sim(28, 0);
    let capture = authority(&sim, handle);
    let prepared = prepare_golden_phase32_attrition(&sim, &capture).unwrap();

    // This unrelated WData mutation is outside the actor's cell.  A cell-local authority
    // would miss it; the canonical Sim/full terrain before-image must reject it.
    sim.map.world.wdata[0].who = 7;
    let before_commit = save_sim(&sim).unwrap();
    assert_eq!(
        commit_golden_phase32_attrition(&mut sim, prepared).unwrap_err(),
        GoldenPhase32AttritionError::CanonicalSimImageMismatch
    );
    assert_eq!(save_sim(&sim).unwrap(), before_commit);

    let (sim, handle) = actor_sim(27, 0);
    let mut wrong_position = authority(&sim, handle);
    wrong_position.x += 1;
    assert_eq!(
        prepare_golden_phase32_attrition(&sim, &wrong_position).unwrap_err(),
        GoldenPhase32AttritionError::PositionMismatch
    );
}

#[test]
fn committed_masks_and_period_survive_save_load() {
    let (mut sim, handle) = actor_sim(29, 0);
    let authority = authority(&sim, handle);
    let prepared = prepare_golden_phase32_attrition(&sim, &authority).unwrap();
    let receipt = commit_golden_phase32_attrition(&mut sim, prepared).unwrap();
    let bytes = save_sim(&sim).unwrap();
    let loaded = load_sim(&bytes).unwrap();
    let row = loaded.world.row_of(handle).unwrap();

    assert_eq!(
        loaded.world.units.get_unit_masks(row),
        receipt.writes.unit_masks_after_child_entry
    );
    assert_eq!(
        loaded.world.units.get_unit_masks2(row),
        receipt.writes.unit_masks2_after_child_return
    );
    assert_eq!(loaded.world.units.attrition()[row], 0);
    assert_eq!(save_sim(&loaded).unwrap(), bytes);
}
