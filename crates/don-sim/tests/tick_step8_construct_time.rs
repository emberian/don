// SPDX-License-Identifier: GPL-3.0-or-later
//! `Wall::update_construct_time` `0x0063D560`, driven through the real tick.
//!
//! `Leader::calc_wall_stats` `0x006CF7C0` direct-calls this body from **both** of its
//! loops (`0x006CF838` for the 2000/building band, `0x006CF908` for the 3000/wall band)
//! for every active object whose `WallData::is_active` answers zero. Nothing here calls
//! the ported function; every assertion is made after `Sim::do_frame` has run the whole
//! `Game::do_frame` schedule, so a regression anywhere between step 8's outer `flags & 2`
//! gate and the object write-back fails these tests.

use don_sim::systems::leaders::{self, WallConstructTimeInputs};
use don_sim::systems::{production, walls};
use don_sim::tick::{Gap, Sim, StepRun};

/// A building type whose `TypeData::time(who)` answer is big enough that the recomputed
/// `constr_time` never lands under the spawned `job_counter` and completes the site
/// mid-test.
const TYPE_TIME: u32 = 100_000;

/// Spawn one under-construction building and one under-construction wall for slot 0, arm
/// the edge-triggered stat passes through `Leader::gather`'s `BitMask<44>` union, and hand
/// both bands their `Wall::update_construct_time` package.
fn sim_with_two_construction_sites() -> Sim {
    let mut sim = Sim::new(0x5170, 16);
    sim.activate(0);

    // `Leader::gather` compares `mask_A | mask_B` against the effective mask and sets
    // `0x0C000000` on a difference (`0x006CE35F`); that is the only thing in the engine
    // that arms either stat pass.
    sim.leaders[0].gather_inputs.rares[3] = true;

    // VALID | STARTED and *not* ACTIVE: `WallData::is_active` (`flags & 4`) answers zero,
    // which is the predicate `calc_wall_stats` tests before the direct call.
    sim.spawn_build(
        0,
        production::BuildData {
            flags: production::flag::VALID | production::flag::STARTED,
            constr_time: 7,
            ..Default::default()
        },
    );
    sim.spawn_wall(
        0,
        walls::WallState {
            flags: walls::FLAG_ALIVE | walls::FLAG_STARTED,
            constr_time: 9,
            ..Default::default()
        },
    );

    let package = Some(WallConstructTimeInputs {
        type_time: TYPE_TIME,
        ..Default::default()
    });
    sim.step8_env.leaders[0].objects.band_2000 = vec![leaders::StatObject {
        wall_construct_time_inputs: package,
        ..Default::default()
    }];
    sim.step8_env.leaders[0].objects.band_3000 = vec![leaders::StatObject {
        wall_construct_time_inputs: package,
        ..Default::default()
    }];
    sim
}

#[test]
fn real_tick_recomputes_construct_time_for_both_bands() {
    let mut sim = sim_with_two_construction_sites();
    // A leader with at least one city skips `CAPITAL_BUILD_TIME`, and with no tribe, tech
    // or rare the whole chain reduces to the final `(10 - 0) * v / 10` identity.
    sim.step8.leaders[0].city_num = 1;

    let tick = sim.do_frame();
    assert_eq!(tick.steps[8], StepRun::Executed);
    assert_eq!(sim.cover.leader_wall_stat_passes, 1);
    assert_eq!(sim.cover.leader_construct_time_updates, 2);
    assert_eq!(sim.builds[0].constr_time, TYPE_TIME);
    assert_eq!(sim.walls[0].constr_time, TYPE_TIME);

    // The two `constr_time` writes are the only thing this pass could resolve, so what is
    // left charged is exactly the four still-explicit base/override query packages:
    // `Object::update_hits` and `Object::update_los` on the wall band, and
    // `Wall::update_hits`/`Wall::update_los` on the building band.
    assert_eq!(sim.cover.gaps[Gap::LeaderCalcWallStats.index()], 4);
}

/// Every reduction is edge-triggered off the *current* leader state, so a leader who loses
/// its last city between two armed passes gets the nomad multiplier on the second one.
#[test]
fn real_tick_applies_capital_build_time_only_while_city_num_is_zero() {
    let mut sim = sim_with_two_construction_sites();
    sim.step8.leaders[0].city_num = 1;
    sim.do_frame();
    assert_eq!(sim.builds[0].constr_time, TYPE_TIME);

    // Re-arm the union with a different rare bit and drop to zero cities.
    sim.leaders[0].gather_inputs.rares[7] = true;
    sim.step8.leaders[0].city_num = 0;
    sim.do_frame();
    assert_eq!(sim.cover.leader_construct_time_updates, 4);
    // CAPITAL_BUILD_TIME ships 300 and is a multiplier over a literal 100, not a
    // `RULE + 100` divisor like the six tribe/wonder/rare reductions above it.
    assert_eq!(sim.builds[0].constr_time, TYPE_TIME * 3);
    assert_eq!(sim.walls[0].constr_time, TYPE_TIME * 3);
}

/// The full chain, through the tick: Maya, `BUILDINGS_CREATED_FASTER`, Tobacco, the
/// British airdefense reduction, Roman forts, the nomad multiplier, and the building-speed
/// upgrade tail — each applied to the previous stage's result, in emitted order.
#[test]
fn real_tick_composes_the_whole_reduction_chain_in_emitted_order() {
    let mut sim = sim_with_two_construction_sites();
    let leader = &mut sim.step8.leaders[0];
    leader.city_num = 0;
    leader.unit_stats.set_tribe_bonus(1, true); // Maya, MAYA_BUILDING_SPEED = 20
    leader.unit_stats.set_tribe_bonus(6, true); // Roman, ROMAN_FORT_SPEED = 50
    leader.unit_stats.set_tribe_bonus(0x0B, true); // British, BRITISH_AA_SPEED = 33
    leader.build_stats.buildings_created_faster = true;
    leader.build_stats.buildings_faster = [true, false, true];
    // The rare has to arrive the way retail delivers it: `GatherInputs::rares` mirrors
    // `Leader + 0x6DCC`, and `Leader::gather`'s union at `0x006CE35F` is what puts it in
    // the effective mask. Writing `rare_effective` directly would be overwritten by the
    // union in the same frame.
    sim.leaders[0].gather_inputs.rares[leaders::RareMask::TOBACCO] = true;

    // A FORTX that is also an AIRDEFENSE type reaches both the British and Roman arms.
    let package = Some(WallConstructTimeInputs {
        type_time: TYPE_TIME,
        is_fort: true,
        is_airdefense: true,
        ..Default::default()
    });
    sim.step8_env.leaders[0].objects.band_2000[0].wall_construct_time_inputs = package;
    sim.step8_env.leaders[0].objects.band_3000[0].wall_construct_time_inputs = package;

    sim.do_frame();

    // Recomputed here from the shipped rules, stage by stage, rather than transcribed.
    let expected = {
        let v = TYPE_TIME * 100 / (20 + 100); // Maya
        let v = v * 3 / 4; // BUILDINGS_CREATED_FASTER
        let v = v * 100 / (0 + 100); // Versailles: absent
        let v = v * 100 / (10 + 100); // Tobacco
        let v = v * 100 / (33 + 100); // British airdefense
        let v = v * 100 / (50 + 100); // Roman fort
        let v = 300 * v / 100; // no cities
        (10 - 2) * v / 10 // two BUILDINGS_FASTER upgrades
    };
    assert_eq!(sim.builds[0].constr_time, expected);
    assert_eq!(sim.walls[0].constr_time, expected);
    assert_ne!(expected, TYPE_TIME);
}

/// The Maya, Dutch and Roman arms each re-issue object vtable `+0x2C`; Versailles,
/// Tobacco and the British do not consult it at all. A hoisted "is a Wonder, skip
/// everything" flag would pass the tests above and fail this one.
#[test]
fn real_tick_suppresses_only_the_three_wonder_gated_reductions() {
    let mut sim = sim_with_two_construction_sites();
    sim.step8.leaders[0].city_num = 1;
    sim.step8.leaders[0].unit_stats.set_tribe_bonus(1, true); // Maya: gated on !is_wonder
    sim.leaders[0].gather_inputs.rares[leaders::RareMask::TOBACCO] = true; // not gated

    // Band 2000 dispatches `BuildData::is_wonder`; band 3000's `WallData` vtable holds the
    // constant-zero body, so a plain wall can never answer yes.
    sim.step8_env.leaders[0].objects.band_2000[0].wall_construct_time_inputs =
        Some(WallConstructTimeInputs {
            type_time: TYPE_TIME,
            is_wonder: true,
            ..Default::default()
        });

    sim.do_frame();

    // The Wonder skipped Maya and still took Tobacco.
    assert_eq!(sim.builds[0].constr_time, TYPE_TIME * 100 / 110);
    // The wall took both.
    assert_eq!(sim.walls[0].constr_time, TYPE_TIME * 100 / 120 * 100 / 110);
}

/// Without its type package the call is refused, not guessed: `constr_time` keeps the
/// value the object already carried and the refusal is charged to the named gap.
#[test]
fn real_tick_charges_a_missing_construct_time_package_and_leaves_state_alone() {
    let mut sim = sim_with_two_construction_sites();
    sim.step8.leaders[0].city_num = 1;
    sim.step8_env.leaders[0].objects.band_2000[0].wall_construct_time_inputs = None;
    sim.step8_env.leaders[0].objects.band_3000[0].wall_construct_time_inputs = None;

    sim.do_frame();

    assert_eq!(sim.cover.leader_construct_time_updates, 0);
    assert_eq!(sim.builds[0].constr_time, 7);
    assert_eq!(sim.walls[0].constr_time, 9);
    // Four base/override query packages plus the two refused construct-time calls.
    assert_eq!(sim.cover.gaps[Gap::LeaderCalcWallStats.index()], 6);
}

/// A finished building never reaches the call. `WallData::is_active` is the gate, and it
/// is tested per object, not per pass.
#[test]
fn real_tick_skips_construct_time_for_a_completed_building() {
    let mut sim = sim_with_two_construction_sites();
    sim.step8.leaders[0].city_num = 1;
    sim.builds[0].flags |= production::flag::ACTIVE;
    sim.walls[0].flags |= walls::FLAG_ACTIVE;

    sim.do_frame();

    assert_eq!(sim.cover.leader_wall_stat_passes, 1);
    assert_eq!(sim.cover.leader_construct_time_updates, 0);
    assert_eq!(sim.builds[0].constr_time, 7);
    assert_eq!(sim.walls[0].constr_time, 9);
}
