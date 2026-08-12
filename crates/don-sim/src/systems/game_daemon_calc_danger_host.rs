//! Canonical `Sim` host for the exact `GameDaemon::calc_danger` body.
//!
//! The prepared image is an execution adapter, not another state owner. It freezes every reached
//! const virtual before step 12 begins, so `GameDaemon::process_all` can preflight all seven
//! children as one transaction and `calc_danger` cannot fail after victory work has committed.

use crate::objects::Band;
use crate::systems::{game_daemon_calc_danger, leaders, production};
use crate::tick::{Sim, NUM_LEADERS};
use crate::world::OBJ_FLAG_ACTIVE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PreparedDangerObject<T: Copy + Default> {
    facts: T,
    seen: [bool; NUM_LEADERS],
}

impl<T: Copy + Default> Default for PreparedDangerObject<T> {
    fn default() -> Self {
        Self {
            facts: T::default(),
            seen: [false; NUM_LEADERS],
        }
    }
}

/// Immutable, infallible image of every fact the scheduled danger child can reach.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PreparedSimCalcDangerHost {
    leaders: [game_daemon_calc_danger::LeaderDangerFacts; NUM_LEADERS],
    units: [Vec<PreparedDangerObject<game_daemon_calc_danger::UnitDangerFacts>>; NUM_LEADERS],
    builds: [Vec<PreparedDangerObject<game_daemon_calc_danger::BuildDangerFacts>>; NUM_LEADERS],
}

impl game_daemon_calc_danger::CalcDangerHost for PreparedSimCalcDangerHost {
    type Fault = std::convert::Infallible;

    fn preflight(&self) -> Result<(), Self::Fault> {
        Ok(())
    }

    fn leader(&self, slot: usize) -> game_daemon_calc_danger::LeaderDangerFacts {
        self.leaders[slot]
    }

    fn unit_band_end(&self, who: usize) -> i32 {
        self.units[who].len() as i32
    }

    fn build_band_end(&self, who: usize) -> i32 {
        game_daemon_calc_danger::BUILD_BAND_BASE + self.builds[who].len() as i32
    }

    fn unit(&self, who: usize, o: i32) -> game_daemon_calc_danger::UnitDangerFacts {
        self.units[who][o as usize].facts
    }

    fn build(&self, who: usize, o: i32) -> game_daemon_calc_danger::BuildDangerFacts {
        self.builds[who][(o - game_daemon_calc_danger::BUILD_BAND_BASE) as usize].facts
    }

    fn is_seen(&self, from: usize, o: i32, to: usize) -> bool {
        if o < game_daemon_calc_danger::BUILD_BAND_BASE {
            self.units[from][o as usize].seen[to]
        } else {
            self.builds[from][(o - game_daemon_calc_danger::BUILD_BAND_BASE) as usize].seen[to]
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SimCalcDangerPreflightFault {
    RegionLattice {
        reg_xs: i32,
        reg_ys: i32,
        reg_size: i32,
    },
    SparseBandsNotDense,
    MissingUnitType {
        who: usize,
        o: i32,
        row: usize,
    },
    MissingUnitRole {
        who: usize,
        o: i32,
        type_index: i32,
    },
    UnitAttackUnavailable {
        who: usize,
        o: i32,
        type_index: i32,
    },
    MissingBuild {
        who: usize,
        o: i32,
        row: usize,
    },
    MissingBuildType {
        who: usize,
        o: i32,
        row: usize,
    },
    MissingFortClass {
        who: usize,
        o: i32,
    },
    MissingTowerClass {
        who: usize,
        o: i32,
    },
    MissingLateStrengthClass {
        who: usize,
        o: i32,
    },
    InvalidVisibilityCell {
        who: usize,
        o: i32,
        fx: i32,
        fy: i32,
    },
}

fn read_build_i32(build: &production::BuildData, offset: usize) -> i32 {
    i32::from_le_bytes(
        build.other[offset..offset + 4]
            .try_into()
            .expect("fixed BuildData dword window"),
    )
}

fn unit_reaches_attack(
    leader_rows: &[game_daemon_calc_danger::LeaderDangerFacts; NUM_LEADERS],
    who: usize,
) -> bool {
    (0..NUM_LEADERS).any(|to| {
        if leader_rows[to].flags & leaders::flag::PROCESS == 0 || to as i32 == leader_rows[who].slot
        {
            return false;
        }
        let mine = leader_rows[who].diplos[to];
        let reciprocal = usize::try_from(leader_rows[who].slot)
            .ok()
            .and_then(|slot| leader_rows[to].diplos.get(slot))
            .copied();
        mine == 0 || reciprocal == Some(0)
    })
}

fn build_visibility(
    sim: &Sim,
    build: &production::BuildData,
    who: usize,
    o: i32,
    to: usize,
) -> Result<bool, SimCalcDangerPreflightFault> {
    // `WorldData::is_seen` returns before indexing a plane under option three.
    if sim.map.fog.option.0 == 3 {
        return Ok(true);
    }
    let x = read_build_i32(build, production::off::X_INTERNAL) ^ game_daemon_calc_danger::COORD_XOR;
    let y = read_build_i32(build, production::off::Y_INTERNAL) ^ game_daemon_calc_danger::COORD_XOR;
    let fx =
        crate::systems::map_terrain::FCoord::from_coord(crate::systems::map_terrain::Coord(x)).0;
    let fy =
        crate::systems::map_terrain::FCoord::from_coord(crate::systems::map_terrain::Coord(y)).0;
    if !sim.map.world.valid_f(fx, fy) {
        return Err(SimCalcDangerPreflightFault::InvalidVisibilityCell { who, o, fx, fy });
    }
    let visible = build.other[0x40] & (1u8 << to) != 0;
    Ok(sim.map.fog.is_seen(&sim.map.world, fx, fy, to as i32) || visible)
}

pub(super) fn prepare(sim: &Sim) -> Result<PreparedSimCalcDangerHost, SimCalcDangerPreflightFault> {
    let map = &sim.map.world;
    if map.reg_xs < 0 || map.reg_ys < 0 || map.reg_size != map.reg_xs.saturating_mul(map.reg_ys) {
        return Err(SimCalcDangerPreflightFault::RegionLattice {
            reg_xs: map.reg_xs,
            reg_ys: map.reg_ys,
            reg_size: map.reg_size,
        });
    }
    if !sim.world.object_bands_are_dense_equivalent() {
        return Err(SimCalcDangerPreflightFault::SparseBandsNotDense);
    }

    let leader_rows: [game_daemon_calc_danger::LeaderDangerFacts; NUM_LEADERS] =
        std::array::from_fn(|who| game_daemon_calc_danger::leader_facts(&sim.step8.leaders[who]));
    let mut units: [_; NUM_LEADERS] = std::array::from_fn(|_| Vec::new());
    let mut builds: [_; NUM_LEADERS] = std::array::from_fn(|_| Vec::new());

    for who in 0..NUM_LEADERS {
        let unit_end = sim.world.unit_mark(who).unwrap_or(0);
        let dense_builds = sim.world.objects.slot(who).band(Band::Build);
        if leader_rows[who].flags & leaders::flag::IN_GAME == 0 {
            units[who].resize(unit_end as usize, PreparedDangerObject::default());
            builds[who].resize(dense_builds.len(), PreparedDangerObject::default());
            continue;
        }
        for o in 0..unit_end {
            let Some(row) = sim.world.unit_row_at(who as i32, o) else {
                units[who].push(PreparedDangerObject::default());
                continue;
            };
            let mut facts = game_daemon_calc_danger::UnitDangerFacts {
                is_valid_unit: sim.world.units.get_flags(row) & OBJ_FLAG_ACTIVE != 0,
                is_on_map: crate::systems::air::is_on_map(sim.world.units.inside_up()[row]),
                x_internal: sim.world.units.x_internal()[row],
                y_internal: sim.world.units.y_internal()[row],
                ..game_daemon_calc_danger::UnitDangerFacts::default()
            };
            if facts.is_valid_unit && facts.is_on_map {
                let type_index = *sim
                    .unit_type
                    .get(row)
                    .ok_or(SimCalcDangerPreflightFault::MissingUnitType { who, o, row })?;
                let type_facts = sim
                    .step12_visibility
                    .types()
                    .binary_search_by_key(&type_index, |candidate| candidate.type_index)
                    .ok()
                    .map(|index| sim.step12_visibility.types()[index])
                    .ok_or(SimCalcDangerPreflightFault::MissingUnitRole { who, o, type_index })?;
                facts.role = type_facts.role;
                if facts.role & game_daemon_calc_danger::UNIT_ROLE_DANGEROUS != 0
                    && unit_reaches_attack(&leader_rows, who)
                {
                    // `UnitData::attack` is a 1,219-byte class override, not the raw static
                    // `UnitTypeData::attack` row. No canonical Sim owner supplies its result yet.
                    return Err(SimCalcDangerPreflightFault::UnitAttackUnavailable {
                        who,
                        o,
                        type_index,
                    });
                }
            }
            units[who].push(PreparedDangerObject {
                facts,
                seen: [false; NUM_LEADERS],
            });
        }

        for (offset, &row) in dense_builds.iter().enumerate() {
            let o = game_daemon_calc_danger::BUILD_BAND_BASE + offset as i32;
            let row = row as usize;
            let build = sim
                .builds
                .get(row)
                .ok_or(SimCalcDangerPreflightFault::MissingBuild { who, o, row })?;
            let mut facts = game_daemon_calc_danger::BuildDangerFacts {
                is_valid_wall: build.is_valid(),
                is_active: build.is_active(),
                city: build.city,
                build_object_flags: build.flags,
                ..game_daemon_calc_danger::BuildDangerFacts::default()
            };
            if !facts.is_valid_wall || !facts.is_active {
                builds[who].push(PreparedDangerObject::default());
                continue;
            }

            if build.city < 0 {
                let type_index = sim
                    .production_runtime
                    .build_types
                    .get(row)
                    .and_then(|value| *value)
                    .ok_or(SimCalcDangerPreflightFault::MissingBuildType { who, o, row })?;
                facts.build_flags = sim
                    .production_runtime
                    .types
                    .get(usize::try_from(type_index).unwrap_or(usize::MAX))
                    .and_then(Option::as_ref)
                    .filter(|installed| installed.type_index == type_index)
                    .map(|installed| installed.build_flags)
                    .ok_or(SimCalcDangerPreflightFault::MissingBuildType { who, o, row })?;
                if facts.build_flags & game_daemon_calc_danger::BUILD_FLAGS_STANDALONE == 0 {
                    builds[who].push(PreparedDangerObject {
                        facts,
                        seen: [false; NUM_LEADERS],
                    });
                    continue;
                }
            }

            facts.x_internal = read_build_i32(build, production::off::X_INTERNAL);
            facts.y_internal = read_build_i32(build, production::off::Y_INTERNAL);
            let view = sim.step8_env.leaders[who]
                .objects
                .band_2000
                .get(offset)
                .ok_or(SimCalcDangerPreflightFault::MissingFortClass { who, o })?;
            facts.wall_type_is_fort = view
                .wall_construct_time_inputs
                .ok_or(SimCalcDangerPreflightFault::MissingFortClass { who, o })?
                .is_fort;
            if !facts.wall_type_is_fort {
                facts.is_tower = view
                    .wall_hit_inputs
                    .ok_or(SimCalcDangerPreflightFault::MissingTowerClass { who, o })?
                    .is_tower_1b7;
            }
            if !facts.wall_type_is_fort
                && !facts.is_tower
                && facts.build_object_flags & game_daemon_calc_danger::BUILD_FLAG_FIXED_DANGER == 0
            {
                return Err(SimCalcDangerPreflightFault::MissingLateStrengthClass { who, o });
            }
            if facts.wall_type_is_fort || facts.is_tower {
                let hits = build.myhits;
                let left = hits.wrapping_sub(build.damage);
                facts.hits_left = if hits < 0 || left < 0 {
                    0
                } else {
                    left.min(hits)
                };
            }

            let mut seen = [false; NUM_LEADERS];
            for to in 0..NUM_LEADERS {
                if leader_rows[to].flags & leaders::flag::PROCESS == 0
                    || to == who
                    || leader_rows[who].diplos[to] == 2
                {
                    continue;
                }
                seen[to] = build_visibility(sim, build, who, o, to)?;
            }
            builds[who].push(PreparedDangerObject { facts, seen });
        }
    }

    Ok(PreparedSimCalcDangerHost {
        leaders: leader_rows,
        units,
        builds,
    })
}
