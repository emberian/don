//! Retail step 15, `Objects::inc_time` `0x0065DB70`, driven through the real tick.
//!
//! Every assertion here rides `Sim::do_frame`, not a direct call into the recovered module,
//! and every one of them fails if the step-15 shell regresses to the ammo-only body it used
//! to be. What each test kills is stated on the test.
//!
//! The instruction evidence for the shape being asserted is in
//! `crates/don-sim/src/systems/unit_inctime.rs` and `docs/assembly/objects-inc-time-step15.md`.

use don_sim::systems::production::BuildData;
use don_sim::systems::unit_inctime::{TYPE_SCHOLARS, TYPE_SCHOLARS_KOREAN};
use don_sim::tick::{Gap, Sim, StepRun};

/// A world with objects but no projectile still runs step 15, because the object bands are
/// part of the step.
///
/// Kills: the old `if self.ammo.live() == 0 { return Vacuous }` early return, which made an
/// entire tick of unit and building clocks disappear whenever nothing was in flight.
#[test]
fn objects_alone_execute_step_fifteen_without_any_projectile() {
    let mut sim = Sim::new(0x1500, 32);
    sim.activate(0);
    sim.spawn_unit(0, 11, 3_000, 3_000, 4).expect("unit spawns");
    sim.spawn_unit(0, 11, 3_400, 3_000, 4).expect("unit spawns");
    sim.spawn_build(0, BuildData::default());

    let tick = sim.do_frame();

    assert_eq!(tick.steps[15], StepRun::Executed);
    assert_eq!(tick.work[15], 3, "two unit-band and one build-band visits");
    assert_eq!(sim.cover.ammo_steps, 0, "nothing was in flight");
    assert_eq!(sim.cover.inc_time_units, 2);
    assert_eq!(sim.cover.inc_time_builds, 1);
}

/// `Nuke::do_damage` `0x0092BC80` is called once per step 15, before every loop, and it is
/// charged whether or not the world has anything else in it.
///
/// Kills: dropping the head call, or moving it inside a population gate.
#[test]
fn the_head_nuke_call_is_charged_once_per_tick_even_in_an_empty_world() {
    let mut sim = Sim::new(0x1501, 16);
    for _ in 0..5 {
        sim.do_frame();
    }
    assert_eq!(sim.cover.gaps[Gap::NukeDoDamage.index()], 5);
    assert_eq!(sim.cover.gaps[Gap::FarmsIncTime.index()], 5);
    // An empty world reaches no object, so nothing below the head call is charged.
    assert_eq!(sim.cover.gaps[Gap::UnitIncTime.index()], 0);
    assert_eq!(sim.cover.gaps[Gap::WallIncTime.index()], 0);
    assert_eq!(sim.cover.gaps[Gap::DeathObjIncTime.index()], 0);
}

/// `Unit::inc_time`'s gate at `0x00610B43` is `inside_up < 0 || type == 52 || type == 53`.
/// A garrisoned ordinary unit is skipped entirely — that is reproduced retail work, not a
/// gap — and a garrisoned Scholar is not.
///
/// Kills: running the guy clocks for every unit unconditionally, dropping either Scholar id,
/// and reading the wrong sign of `inside_up`.
#[test]
fn the_unit_gate_skips_garrisoned_units_but_never_scholars() {
    let mut sim = Sim::new(0x1502, 32);
    sim.activate(0);
    let outside = sim.spawn_unit(0, 11, 3_000, 3_000, 4).expect("spawns");
    let garrisoned = sim.spawn_unit(0, 11, 3_400, 3_000, 4).expect("spawns");
    let scholar = sim
        .spawn_unit(0, TYPE_SCHOLARS, 3_800, 3_000, 4)
        .expect("spawns");
    let korean = sim
        .spawn_unit(0, TYPE_SCHOLARS_KOREAN, 4_200, 3_000, 4)
        .expect("spawns");

    for h in [garrisoned, scholar, korean] {
        let row = sim.world.row_of(h).expect("live row");
        sim.world.units.inside_up_mut()[row] = 3;
    }
    let outside_row = sim.world.row_of(outside).expect("live row");
    assert!(sim.world.units.inside_up()[outside_row] < 0);

    let tick = sim.do_frame();

    assert_eq!(tick.steps[15], StepRun::Executed);
    assert_eq!(tick.work[15], 4, "all four are still visited by the band loop");
    assert_eq!(
        sim.cover.inc_time_units_gated, 1,
        "only the garrisoned non-Scholar is skipped"
    );
    assert_eq!(sim.cover.inc_time_units, 3);
    assert_eq!(
        sim.cover.gaps[Gap::UnitIncTime.index()],
        3,
        "guy clocks are owed by exactly the units the gate accepted"
    );
}

/// Step 15's owner walk is the single ten-iteration loop at `0x0065DBA0..0x0065DC17`, so the
/// building band is visited for owners 8 and 9 as well. Step 14 walks Build/Wall for the
/// eight banded owners only, and the two therefore disagree on the same world.
///
/// Kills: reusing `SparseObjectBands::traversal_into` (step 14's traversal) for step 15,
/// which would silently drop the nature owners' buildings from the step-15 pass.
#[test]
fn the_building_band_reaches_the_nature_owners_that_step_fourteen_skips() {
    let mut sim = Sim::new(0x1503, 32);
    sim.activate(0);
    assert!(sim.world.set_object_owner_active(9, true));
    sim.spawn_build(0, BuildData::default());
    sim.spawn_build(9, BuildData::default());

    let tick = sim.do_frame();

    assert_eq!(tick.steps[15], StepRun::Executed);
    assert_eq!(
        sim.cover.inc_time_builds, 2,
        "step 15 walks the building band for all ten owners"
    );
    assert_eq!(sim.cover.gaps[Gap::WallIncTime.index()], 2);
    assert_eq!(
        sim.cover.build_process, 1,
        "step 14 walks the building band for owners 0..7 only"
    );
    assert!(tick.steps[14].ran());
}

/// The building-band call site does not exist in the wall band: `Objects::inc_time` has no
/// loop at base 3000, so a wall is processed at step 14 and never reaches step 15.
///
/// Kills: adding a third inner loop, or reusing a traversal that emits `RetailBand::Wall`.
#[test]
fn walls_are_processed_but_never_inc_timed() {
    let mut sim = Sim::new(0x1504, 32);
    sim.activate(0);
    sim.spawn_wall(0, Default::default());

    let tick = sim.do_frame();

    assert!(sim.cover.wall_process > 0, "step 14 does visit the wall");
    assert_eq!(sim.cover.inc_time_builds, 0);
    assert_eq!(sim.cover.gaps[Gap::WallIncTime.index()], 0);
    assert_eq!(
        tick.steps[15],
        StepRun::Vacuous,
        "a wall-only world gives step 15 nothing to visit"
    );
}

/// The death loop at `0x0065DC7D` walks `Objects+0x14C` in slot order and calls
/// `DeathObj::inc_time` for every nonzero `valid`. The body is unadmitted, so each reached
/// corpse is charged.
///
/// Kills: dropping the death loop, or gating it on the ammo population the way the old body
/// gated everything.
#[test]
fn every_valid_corpse_is_reached_by_the_death_loop() {
    let mut sim = Sim::new(0x1505, 16);
    assert!(sim.deaths.slots.len() >= 3, "the ring has slots to fill");
    sim.deaths.slots[0].valid = 1;
    sim.deaths.slots[2].valid = 1;

    let tick = sim.do_frame();

    assert_eq!(tick.steps[15], StepRun::Executed, "corpses are population");
    assert_eq!(tick.work[15], 2);
    assert_eq!(sim.cover.inc_time_deaths_visited, 2);
    assert_eq!(sim.cover.gaps[Gap::DeathObjIncTime.index()], 2);
}

/// One tick over a world holding every family the shell reaches charges each call site
/// exactly once.
///
/// Kills: dropping any one of the five charged call sites while the others keep the step
/// green. It does **not** pin the order of the loops — the counters are order-blind, and the
/// order is asserted only by the instruction transcription in `tick.rs` and the module doc.
#[test]
fn one_tick_charges_the_whole_shell_when_every_family_is_populated() {
    let mut sim = Sim::new(0x1506, 32);
    sim.activate(0);
    sim.spawn_unit(0, 11, 3_000, 3_000, 4).expect("spawns");
    sim.spawn_build(0, BuildData::default());
    sim.deaths.slots[0].valid = 1;

    let tick = sim.do_frame();

    assert_eq!(tick.steps[15], StepRun::Executed);
    assert_eq!(tick.work[15], 3, "one unit, one building, one corpse");
    assert_eq!(sim.cover.gaps[Gap::NukeDoDamage.index()], 1);
    assert_eq!(sim.cover.gaps[Gap::UnitIncTime.index()], 1);
    assert_eq!(sim.cover.gaps[Gap::WallIncTime.index()], 1);
    assert_eq!(sim.cover.gaps[Gap::DeathObjIncTime.index()], 1);
    assert_eq!(sim.cover.gaps[Gap::FarmsIncTime.index()], 1);
    // `Unit::execute_events` is a second virtual on the unit band only; the building band
    // makes no such call, so the charge cannot exceed the unit count.
    assert!(
        sim.cover.gaps[Gap::UnitExecuteEvents.index()] + sim.cover.inc_time_verify_paths == 1,
        "exactly one `vt+0x154` call site, on one arm or the other"
    );
}

/// Step 15's traversal does not read `Game::frame`, so the work it reports is identical on
/// every tick of a static world — unlike step 14, whose owner rotation is frame-derived.
///
/// Kills: reintroducing a `(frame + i) % 10` rotation into the step-15 walk. With one
/// building owned by owner 9 and one by owner 0, a rotated walk that also kept step 14's
/// eight-owner Build restriction would drop a visit on some frames.
#[test]
fn the_step_fifteen_walk_is_frame_independent() {
    let mut sim = Sim::new(0x1507, 32);
    sim.activate(0);
    assert!(sim.world.set_object_owner_active(9, true));
    sim.spawn_unit(0, 11, 3_000, 3_000, 4).expect("spawns");
    sim.spawn_build(0, BuildData::default());
    sim.spawn_build(9, BuildData::default());

    for frame in 0..12 {
        let tick = sim.do_frame();
        assert_eq!(tick.frame, frame);
        assert_eq!(tick.work[15], 3, "frame {frame} changed the step-15 walk");
    }
    assert_eq!(sim.cover.inc_time_units, 12);
    assert_eq!(sim.cover.inc_time_builds, 24);
}
