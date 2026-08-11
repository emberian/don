// SPDX-License-Identifier: GPL-3.0-or-later
//! `GameDaemon::calc_danger` `0x00732D10` against the authoritative `map_terrain::World`.
//!
//! # What this file does and does not prove
//!
//! The step-12 child hook lives in `crates/don-sim/src/tick.rs`, which a sibling lane owns
//! this wave, so **the body is not yet reachable from `Sim::do_frame`**. The first test here
//! pins that fact from the real tick — step 12 runs, the scheduler reaches `calc_danger` on
//! `frame % 200 == 0`, and the child is still charged to
//! [`Gap::GameDaemonCalcDanger`] with every danger plane untouched. It is a *red* proof, and
//! it will start failing the moment the hook lands, which is exactly when it should.
//!
//! The remaining tests drive [`game_daemon_calc_danger::calc_danger`] directly against
//! `sim.map.world` — the same `map_terrain::World` the `world` checksum channel walks, not a
//! test double — so the plane mutations are on the authoritative state even though the tick
//! does not call them yet. Do not read them as real-tick coverage.

use don_sim::systems::game_daemon_calc_danger::{
    calc_danger, region_of, BuildDangerFacts, CalcDangerError, CalcDangerHost, DangerPlanes,
    LeaderDangerFacts, UnitDangerFacts, BUILD_BAND_BASE, BUILD_FLAGS_HIGH_VALUE, COORD_XOR,
    LEADER_SLOTS, STRENGTH_HIGH_VALUE, UNIT_ROLE_DANGEROUS,
};
use don_sim::systems::leaders::flag;
use don_sim::systems::map_terrain::COORD_PER_RCELL;
use don_sim::tick::{Gap, Sim, StepRun};

/// A host with no live object band: `preflight` refuses, which is what the `Sim` integration
/// must do until the building band carries authoritative `Build`/`Wall` facts.
struct EmptyBandHost;

impl CalcDangerHost for EmptyBandHost {
    type Fault = &'static str;
    fn preflight(&self) -> Result<(), Self::Fault> {
        Err("no authoritative object band")
    }
    fn leader(&self, _slot: usize) -> LeaderDangerFacts {
        LeaderDangerFacts::default()
    }
    fn unit_band_end(&self, _who: usize) -> i32 {
        0
    }
    fn build_band_end(&self, _who: usize) -> i32 {
        0
    }
    fn unit(&self, _who: usize, _o: i32) -> UnitDangerFacts {
        UnitDangerFacts::default()
    }
    fn build(&self, _who: usize, _o: i32) -> BuildDangerFacts {
        BuildDangerFacts::default()
    }
    fn is_seen(&self, _from: usize, _o: i32, _to: usize) -> bool {
        true
    }
}

/// Two live leaders and an explicit unit/building population.
#[derive(Default)]
struct ScriptedHost {
    leaders: [LeaderDangerFacts; LEADER_SLOTS],
    unit_end: [i32; LEADER_SLOTS],
    build_end: [i32; LEADER_SLOTS],
    units: Vec<((usize, i32), UnitDangerFacts)>,
    builds: Vec<((usize, i32), BuildDangerFacts)>,
}

impl ScriptedHost {
    fn two_players() -> Self {
        let mut host = ScriptedHost::default();
        for (slot, leader) in host.leaders.iter_mut().enumerate() {
            leader.slot = slot as i32;
        }
        host.leaders[0].flags = flag::IN_GAME | flag::PROCESS;
        host.leaders[1].flags = flag::IN_GAME | flag::PROCESS;
        host
    }
}

impl CalcDangerHost for ScriptedHost {
    type Fault = &'static str;
    fn preflight(&self) -> Result<(), Self::Fault> {
        Ok(())
    }
    fn leader(&self, slot: usize) -> LeaderDangerFacts {
        self.leaders[slot]
    }
    fn unit_band_end(&self, who: usize) -> i32 {
        self.unit_end[who]
    }
    fn build_band_end(&self, who: usize) -> i32 {
        self.build_end[who]
    }
    fn unit(&self, who: usize, o: i32) -> UnitDangerFacts {
        self.units
            .iter()
            .find(|(key, _)| *key == (who, o))
            .map(|(_, facts)| *facts)
            .unwrap_or_default()
    }
    fn build(&self, who: usize, o: i32) -> BuildDangerFacts {
        self.builds
            .iter()
            .find(|(key, _)| *key == (who, o))
            .map(|(_, facts)| *facts)
            .unwrap_or_default()
    }
    fn is_seen(&self, _from: usize, _o: i32, _to: usize) -> bool {
        true
    }
}

fn internal(region: i32) -> i32 {
    (region * COORD_PER_RCELL) ^ COORD_XOR
}

#[test]
fn the_real_tick_still_charges_the_danger_child_and_leaves_every_plane_zero() {
    let mut sim = Sim::new(0x12_732d10, 16);
    // Salt one plane so a hook that ran the wipe would be visible even with no population.
    sim.map.world.danger[0][0] = 4242;

    let frame0 = sim.do_frame();

    assert_eq!(frame0.steps[12], StepRun::Executed, "the shell itself runs");
    assert_eq!(
        sim.cover.gaps[Gap::GameDaemonCalcDanger.index()],
        1,
        "frame 0 is the frame % 200 == 0 phase, so the child is reached and charged"
    );
    assert_eq!(
        sim.map.world.danger[0][0], 4242,
        "the hook is NOT wired: nothing wipes the plane from the real tick yet"
    );

    let frame1 = sim.do_frame();
    assert_eq!(frame1.steps[12], StepRun::Executed);
    assert_eq!(
        sim.cover.gaps[Gap::GameDaemonCalcDanger.index()],
        1,
        "frame 1 is off-phase"
    );
}

#[test]
fn the_authoritative_world_satisfies_the_region_lattice_the_child_requires() {
    let sim = Sim::new(0x12_732d10, 16);
    let world = &sim.map.world;
    assert_eq!(world.reg_size, world.reg_xs * world.reg_ys);
    assert_eq!(world.reg_size, DangerPlanes::reg_size(world));
    for who in 0..LEADER_SLOTS {
        assert_eq!(world.danger[who].len(), world.reg_size as usize);
        assert!(DangerPlanes::plane_present(world, who));
    }
}

#[test]
fn a_host_that_cannot_supply_the_band_refuses_before_touching_the_world() {
    let mut sim = Sim::new(0x12_732d10, 16);
    sim.map.world.danger[3][2] = -17;

    let outcome = calc_danger(&mut sim.map.world, &EmptyBandHost);

    assert_eq!(
        outcome,
        Err(CalcDangerError::Host("no authoritative object band"))
    );
    assert_eq!(
        sim.map.world.danger[3][2], -17,
        "a refused child must not have wiped a single plane"
    );
}

#[test]
fn the_child_rewrites_the_authoritative_danger_planes_end_to_end() {
    let mut sim = Sim::new(0x12_732d10, 16);
    let reg_xs = sim.map.world.reg_xs;
    assert!(reg_xs >= 4, "the fixture needs room for a 3x3 stamp");

    // Stale values in both live planes; pass 1 must remove them.
    for cell in sim.map.world.danger[0].iter_mut() {
        *cell = 1234;
    }
    for cell in sim.map.world.danger[1].iter_mut() {
        *cell = -99;
    }

    let mut host = ScriptedHost::two_players();
    host.unit_end[0] = 1;
    host.units.push((
        (0, 0),
        UnitDangerFacts {
            is_valid_unit: true,
            is_on_map: true,
            role: UNIT_ROLE_DANGEROUS,
            x_internal: internal(2),
            y_internal: internal(2),
            attack: 40,
        },
    ));
    host.build_end[1] = BUILD_BAND_BASE + 1;
    host.builds.push((
        (1, BUILD_BAND_BASE),
        BuildDangerFacts {
            is_valid_wall: true,
            is_active: true,
            city: 0,
            x_internal: internal(2),
            y_internal: internal(2),
            basic_type_build_flags: BUILD_FLAGS_HIGH_VALUE,
            ..Default::default()
        },
    ));

    let trace = calc_danger(&mut sim.map.world, &host).unwrap();

    assert_eq!(trace.planes_cleared, 2);
    assert_eq!(trace.units_scanned, 1);
    assert_eq!(trace.builds_scanned, 1);
    assert_eq!(trace.unmapped_centre_cells, 0);

    let centre = (2 * reg_xs + 2) as usize;
    let nw = (1 * reg_xs + 1) as usize;
    // Player 1's plane: +20 from player 0's unit, -50 from player 1's own building.
    assert_eq!(sim.map.world.danger[1][centre], 20 - STRENGTH_HIGH_VALUE);
    assert_eq!(sim.map.world.danger[1][nw], -(STRENGTH_HIGH_VALUE / 2));
    // Player 0's plane: the enemy building deposits, the friendly unit does not.
    assert_eq!(sim.map.world.danger[0][centre], STRENGTH_HIGH_VALUE);
    assert_eq!(sim.map.world.danger[0][nw], STRENGTH_HIGH_VALUE / 2);
    // Everything else was wiped by pass 1 and never rewritten.
    assert_eq!(sim.map.world.danger[2].iter().copied().max(), Some(0));
    assert_eq!(sim.map.world.danger[2].iter().copied().min(), Some(0));
}

#[test]
fn region_of_agrees_with_the_worlds_own_rcoord_arithmetic() {
    for region in [-3i32, -1, 0, 1, 7, 63] {
        assert_eq!(region_of(internal(region)), region);
    }
    // The XOR is load-bearing: reading the raw field without it lands somewhere else.
    assert_ne!(region_of(3 * COORD_PER_RCELL), 3);
}
