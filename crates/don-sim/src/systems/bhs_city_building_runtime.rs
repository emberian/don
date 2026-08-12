//! Canonical City/Build transaction for BHS builtin 386, `num_city_buildings`.
//!
//! Retail `ScenarioFuncSet::num_city_buildings` is the 129-byte handler at `0x009F0150`.
//! It validates the one-based Leader, resolves a City by id then name through
//! `ScenarioFuncSet::get_city_index` (`0x009E2BA0`), resolves the first internal Type name
//! through `get_type_index(..., 0)` (`0x00A03480`), then calls
//! `CityData::count_buildings(type, 0, argument == 0)` (`0x00739390`).
//!
//! `count_buildings` walks the canonical `CityData::o` / `BuildData::city_down` chain. Every
//! live row must be valid; argument zero additionally requires `BuildData::is_active` (finished),
//! and the type test is the non-strict `ObjectData::is(type, 0)` relation. This adapter reads the
//! checksum-owned CityPool, Sim Build rows, World Build registry, production type owner, and the
//! canonical BHS Type relation in one preflighted, read-only transaction.

use crate::objects::{Band, BUILD_BAND_BASE};
use crate::tick::Sim;

use super::bhs_type_table::{TypeBuiltinState, TypeDomain, NUM_LEADERS, NUM_TYPES};
use super::production::{flag, runtime::LiveProductionRuntime};

pub const NUM_CITY_BUILDINGS_BUILTIN: u32 = 386;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CityBuildingCountRequest {
    /// Retail one-based Leader argument.
    pub who: i32,
    pub city_name: String,
    pub type_name: String,
    /// Retail's fourth argument. Zero means finished/active buildings only.
    pub include_unfinished: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CityBuildingCountStatus {
    Counted,
    InvalidLeader,
    InactiveLeader,
    InvalidCity,
    InvalidType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityBuildingCandidateReceipt {
    pub object_index: i16,
    pub row: usize,
    pub current_type: Option<i32>,
    pub valid: bool,
    pub active: bool,
    pub type_match: bool,
    pub counted: bool,
    pub next_object: i16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CityBuildingCountReceipt {
    pub request: CityBuildingCountRequest,
    pub status: CityBuildingCountStatus,
    pub requested_owner: Option<usize>,
    pub city_slot: Option<usize>,
    pub city_owner: Option<usize>,
    pub requested_type: Option<usize>,
    pub candidates: Vec<CityBuildingCandidateReceipt>,
    pub returned: i32,
}

impl CityBuildingCountReceipt {
    fn rejected(
        request: CityBuildingCountRequest,
        status: CityBuildingCountStatus,
        requested_owner: Option<usize>,
        city_slot: Option<usize>,
        requested_type: Option<usize>,
    ) -> Self {
        Self {
            request,
            status,
            requested_owner,
            city_slot,
            city_owner: None,
            requested_type,
            candidates: Vec::new(),
            returned: -1,
        }
    }

    pub fn validates(&self) -> bool {
        match self.status {
            CityBuildingCountStatus::Counted => {
                self.requested_owner.is_some()
                    && self.city_slot.is_some()
                    && self.city_owner.is_some()
                    && self.requested_type.is_some()
                    && self.returned >= 0
                    && self.returned
                        == self
                            .candidates
                            .iter()
                            .filter(|candidate| candidate.counted)
                            .count() as i32
                    && self.candidates.iter().all(|candidate| {
                        candidate.counted
                            == (candidate.valid
                                && (self.request.include_unfinished != 0 || candidate.active)
                                && candidate.type_match)
                    })
            }
            _ => self.candidates.is_empty() && self.returned == -1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CityBuildingCountError {
    NonAsciiQuery,
    LeaderMirrorMismatch {
        owner: usize,
        type_flags: i32,
        victory_flags: i32,
        step8_flags: u32,
    },
    InvalidCityMark {
        owner: usize,
        mark: i32,
        slots: usize,
    },
    InvalidCityOwner {
        requested_owner: usize,
        city_slot: usize,
        city_owner: i8,
    },
    BuildObjectOutOfRange {
        owner: usize,
        object_index: i16,
    },
    BuildRowOutOfRange {
        owner: usize,
        object_index: i16,
        row: usize,
        builds: usize,
    },
    BuildIdentityMismatch {
        owner: usize,
        object_index: i16,
        row: usize,
        build_owner: u8,
        build_object: i16,
    },
    MissingBuildType {
        owner: usize,
        object_index: i16,
        row: usize,
    },
    InvalidBuildType {
        owner: usize,
        object_index: i16,
        row: usize,
        current_type: i32,
    },
    CityChainCycle {
        owner: usize,
        object_index: i16,
    },
}

fn string_eq(left: &str, right: &str) -> Result<bool, CityBuildingCountError> {
    if !left.is_ascii() || !right.is_ascii() {
        return Err(CityBuildingCountError::NonAsciiQuery);
    }
    Ok(left.len() == right.len() && left.eq_ignore_ascii_case(right))
}

fn first_type(
    types: &TypeBuiltinState,
    query: &str,
) -> Result<Option<usize>, CityBuildingCountError> {
    if !query.is_ascii() {
        return Err(CityBuildingCountError::NonAsciiQuery);
    }
    if query.is_empty() {
        return Ok(None);
    }
    Ok(types
        .types
        .rows()
        .iter()
        .position(|row| row.name.len() == query.len() && row.name.eq_ignore_ascii_case(query)))
}

fn build_row(sim: &Sim, owner: usize, object_index: i16) -> Result<usize, CityBuildingCountError> {
    let slot = i32::from(object_index)
        .checked_sub(BUILD_BAND_BASE as i32)
        .and_then(|slot| usize::try_from(slot).ok())
        .ok_or(CityBuildingCountError::BuildObjectOutOfRange {
            owner,
            object_index,
        })?;
    let row = sim
        .world
        .objects
        .slot(owner)
        .band(Band::Build)
        .get(slot)
        .copied()
        .ok_or(CityBuildingCountError::BuildObjectOutOfRange {
            owner,
            object_index,
        })? as usize;
    if row >= sim.builds.len() {
        return Err(CityBuildingCountError::BuildRowOutOfRange {
            owner,
            object_index,
            row,
            builds: sim.builds.len(),
        });
    }
    let build = &sim.builds[row];
    if build.who as usize != owner || build.object_id() != object_index {
        return Err(CityBuildingCountError::BuildIdentityMismatch {
            owner,
            object_index,
            row,
            build_owner: build.who,
            build_object: build.object_id(),
        });
    }
    Ok(row)
}

/// Execute BHS builtin 386 as one read-only transaction over canonical City/Build owners.
pub fn apply_sim_city_building_count_transaction(
    sim: &Sim,
    production: &LiveProductionRuntime,
    types: &TypeBuiltinState,
    request: CityBuildingCountRequest,
) -> Result<CityBuildingCountReceipt, CityBuildingCountError> {
    let owner0 = request.who.wrapping_sub(1);
    if !(0..NUM_LEADERS as i32).contains(&owner0) {
        return Ok(CityBuildingCountReceipt::rejected(
            request,
            CityBuildingCountStatus::InvalidLeader,
            None,
            None,
            None,
        ));
    }
    let requested_owner = owner0 as usize;
    let type_flags = types.leaders[requested_owner].leader_flags;
    let victory_flags = sim.vic_leaders.slots[requested_owner].leader_flags;
    let step8_flags = sim.step8.leaders[requested_owner].flags;
    if type_flags != victory_flags || type_flags as u32 != step8_flags {
        return Err(CityBuildingCountError::LeaderMirrorMismatch {
            owner: requested_owner,
            type_flags,
            victory_flags,
            step8_flags,
        });
    }
    if type_flags & 1 == 0 {
        return Ok(CityBuildingCountReceipt::rejected(
            request,
            CityBuildingCountStatus::InactiveLeader,
            Some(requested_owner),
            None,
            None,
        ));
    }

    let mark = sim.cities.city_mark[requested_owner];
    let mark_usize =
        usize::try_from(mark).map_err(|_| CityBuildingCountError::InvalidCityMark {
            owner: requested_owner,
            mark,
            slots: sim.cities.slots[requested_owner].len(),
        })?;
    if mark_usize > sim.cities.slots[requested_owner].len() {
        return Err(CityBuildingCountError::InvalidCityMark {
            owner: requested_owner,
            mark,
            slots: sim.cities.slots[requested_owner].len(),
        });
    }
    let mut city_slot = None;
    for (slot, city) in sim.cities.slots[requested_owner][..mark_usize]
        .iter()
        .enumerate()
    {
        if city.active()
            && (string_eq(&city.id, &request.city_name)?
                || string_eq(&city.name, &request.city_name)?)
        {
            city_slot = Some(slot);
            break;
        }
    }
    let Some(city_slot) = city_slot else {
        return Ok(CityBuildingCountReceipt::rejected(
            request,
            CityBuildingCountStatus::InvalidCity,
            Some(requested_owner),
            None,
            None,
        ));
    };

    let Some(requested_type) = first_type(types, &request.type_name)? else {
        return Ok(CityBuildingCountReceipt::rejected(
            request,
            CityBuildingCountStatus::InvalidType,
            Some(requested_owner),
            Some(city_slot),
            None,
        ));
    };
    let city = &sim.cities.slots[requested_owner][city_slot];
    if !(0..NUM_LEADERS as i8).contains(&city.who) {
        return Err(CityBuildingCountError::InvalidCityOwner {
            requested_owner,
            city_slot,
            city_owner: city.who,
        });
    }
    let city_owner = city.who as usize;
    let mut object_index = city.o;
    let mut visited = Vec::new();
    let mut candidates = Vec::new();
    while object_index >= 0 {
        if visited.contains(&object_index) {
            return Err(CityBuildingCountError::CityChainCycle {
                owner: city_owner,
                object_index,
            });
        }
        visited.push(object_index);
        let row = build_row(sim, city_owner, object_index)?;
        let build = &sim.builds[row];
        let valid = build.flags & flag::VALID != 0;
        let active = build.flags & flag::ACTIVE != 0;
        let mut current_type = None;
        let mut type_match = false;
        if valid {
            let type_index = production.build_types.get(row).copied().flatten().ok_or(
                CityBuildingCountError::MissingBuildType {
                    owner: city_owner,
                    object_index,
                    row,
                },
            )?;
            let type_slot = usize::try_from(type_index).map_err(|_| {
                CityBuildingCountError::InvalidBuildType {
                    owner: city_owner,
                    object_index,
                    row,
                    current_type: type_index,
                }
            })?;
            if type_slot >= NUM_TYPES || types.types.row(type_slot).domain() != TypeDomain::Build {
                return Err(CityBuildingCountError::InvalidBuildType {
                    owner: city_owner,
                    object_index,
                    row,
                    current_type: type_index,
                });
            }
            current_type = Some(type_index);
            type_match = types
                .types
                .row(type_slot)
                .is_list
                .iter()
                .any(|&candidate| usize::from(candidate) == requested_type);
        }
        let counted = valid && (request.include_unfinished != 0 || active) && type_match;
        candidates.push(CityBuildingCandidateReceipt {
            object_index,
            row,
            current_type,
            valid,
            active,
            type_match,
            counted,
            next_object: build.city_down,
        });
        object_index = build.city_down;
    }

    let returned = candidates
        .iter()
        .filter(|candidate| candidate.counted)
        .count() as i32;
    let receipt = CityBuildingCountReceipt {
        request,
        status: CityBuildingCountStatus::Counted,
        requested_owner: Some(requested_owner),
        city_slot: Some(city_slot),
        city_owner: Some(city_owner),
        requested_type: Some(requested_type),
        candidates,
        returned,
    };
    debug_assert!(receipt.validates());
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::bhs_type_table::{
        LeaderTypeMasks, TribeRoster, TypeBackup, TypeRow, TypeTable, BUILD_BEGIN, BUILD_END,
        NUM_TRIBES, REGULAR_UNIT_BEGIN, REGULAR_UNIT_END,
    };
    use crate::systems::production::BuildData;
    use crate::systems::save_load::{load_sim, save_sim};

    const CITY_TYPE: i32 = 414;
    const FARM: i32 = 417;
    const FARM_DERIVED: i32 = 421;

    fn types() -> TypeBuiltinState {
        let mut rows: Vec<_> = (0..NUM_TYPES).map(TypeRow::empty).collect();
        rows[CITY_TYPE as usize].name = "Small City".into();
        rows[FARM as usize].name = "Farm".into();
        rows[FARM_DERIVED as usize].name = "Farm Variant".into();
        rows[FARM_DERIVED as usize].is_list.push(FARM as u16);
        let backups = rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                ((REGULAR_UNIT_BEGIN..REGULAR_UNIT_END).contains(&index)
                    || (BUILD_BEGIN..BUILD_END).contains(&index))
                .then(|| TypeBackup::capture_pristine(row))
            })
            .collect();
        TypeBuiltinState::new(
            TypeTable::new(rows, backups).unwrap(),
            TribeRoster::new((0..NUM_TRIBES).map(|i| format!("Tribe {i}")).collect()).unwrap(),
            std::array::from_fn(|owner| LeaderTypeMasks {
                leader_flags: if owner == 0 { 3 } else { 0 },
                ..LeaderTypeMasks::default()
            }),
        )
    }

    fn set_object(build: &mut BuildData, object: i16) {
        build.other[super::super::production::off::OBJECT_ID
            ..super::super::production::off::OBJECT_ID + 2]
            .copy_from_slice(&object.to_le_bytes());
        build.other[0x28..0x2a].copy_from_slice(&(-1i16).to_le_bytes());
        build.gather_down = -1;
        build.wonder = -1;
        build.dock = -1;
        build.attack_ox = -1;
        build.attack_whom = -1;
    }

    fn fixture() -> (Sim, LiveProductionRuntime) {
        let mut sim = Sim::new(386, 8);
        sim.step8.leaders[0].flags = 3;
        sim.vic_leaders.slots[0].leader_flags = 3;

        let mut center = BuildData {
            flags: flag::VALID | flag::ACTIVE,
            city: 0,
            city_down: BUILD_BAND_BASE as i16 + 1,
            ..BuildData::default()
        };
        set_object(&mut center, BUILD_BAND_BASE as i16);
        let center_row = sim.spawn_build(0, center);
        let mut farm = BuildData {
            flags: flag::VALID | flag::ACTIVE,
            city: 0,
            city_down: BUILD_BAND_BASE as i16 + 2,
            ..BuildData::default()
        };
        set_object(&mut farm, BUILD_BAND_BASE as i16 + 1);
        let farm_row = sim.spawn_build(0, farm);
        let mut unfinished = BuildData {
            flags: flag::VALID,
            city: 0,
            city_down: -1,
            ..BuildData::default()
        };
        set_object(&mut unfinished, BUILD_BAND_BASE as i16 + 2);
        let unfinished_row = sim.spawn_build(0, unfinished);

        sim.cities.city_mark[0] = 1;
        let city = &mut sim.cities.slots[0][0];
        city.city_flags = 1;
        city.city = 0;
        city.o = BUILD_BAND_BASE as i16;
        city.who = 0;
        city.name = "Athens".into();
        city.id = "capital_0".into();

        let mut production = LiveProductionRuntime::default();
        production.register_build(center_row, CITY_TYPE);
        production.register_build(farm_row, FARM);
        production.register_build(unfinished_row, FARM_DERIVED);
        (sim, production)
    }

    fn request(include_unfinished: i32) -> CityBuildingCountRequest {
        CityBuildingCountRequest {
            who: 1,
            city_name: "aThEnS".into(),
            type_name: "fArM".into(),
            include_unfinished,
        }
    }

    #[test]
    fn counts_non_strict_valid_builds_and_applies_the_finished_gate_only_for_zero() {
        let types = types();
        let (sim, production) = fixture();
        let finished =
            apply_sim_city_building_count_transaction(&sim, &production, &types, request(0))
                .unwrap();
        let all = apply_sim_city_building_count_transaction(&sim, &production, &types, request(1))
            .unwrap();
        assert_eq!(finished.returned, 1);
        assert_eq!(all.returned, 2);
        assert!(finished.validates());
        assert!(all.validates());
        assert_eq!(all.candidates.len(), 3);
        assert!(!all.candidates[0].type_match);
        assert!(all.candidates[1].counted);
        assert!(all.candidates[2].counted);
        assert!(!all.candidates[2].active);
    }

    #[test]
    fn transaction_is_read_only_and_id_precedes_name_in_city_resolution() {
        let types = types();
        let (mut sim, production) = fixture();
        sim.cities.slots[0][0].id = "Athens".into();
        sim.cities.slots[0][0].name = "Wrong".into();
        let cities_before = sim.cities.clone();
        let builds_before: Vec<_> = sim.builds.iter().map(BuildData::image).collect();
        let types_before = production.build_types.clone();
        let receipt =
            apply_sim_city_building_count_transaction(&sim, &production, &types, request(1))
                .unwrap();
        assert_eq!(receipt.returned, 2);
        assert_eq!(sim.cities.slots, cities_before.slots);
        assert_eq!(sim.cities.city_mark, cities_before.city_mark);
        assert_eq!(
            sim.builds.iter().map(BuildData::image).collect::<Vec<_>>(),
            builds_before
        );
        assert_eq!(production.build_types, types_before);
    }

    #[test]
    fn missing_projection_and_cycles_fail_closed() {
        let types = types();
        let (mut sim, mut production) = fixture();
        production.build_types[1] = None;
        assert!(matches!(
            apply_sim_city_building_count_transaction(&sim, &production, &types, request(1)),
            Err(CityBuildingCountError::MissingBuildType { row: 1, .. })
        ));

        production.register_build(1, FARM);
        sim.builds[2].city_down = BUILD_BAND_BASE as i16 + 1;
        assert!(matches!(
            apply_sim_city_building_count_transaction(&sim, &production, &types, request(1)),
            Err(CityBuildingCountError::CityChainCycle { .. })
        ));
    }

    #[test]
    fn city_build_chain_survives_save_load_and_reinstalled_type_projection() {
        let types = types();
        let (mut sim, _) = fixture();
        // The supported save surface requires every live Build to be completed.
        sim.builds[2].flags |= flag::ACTIVE;
        // Leader activation hosts are reinstalled independently from the City/Build save owner.
        sim.step8.leaders[0].flags = 0;
        sim.vic_leaders.slots[0].leader_flags = 0;
        let bytes = save_sim(&sim).unwrap();
        let mut loaded = load_sim(&bytes).unwrap();
        loaded.step8.leaders[0].flags = 3;
        loaded.vic_leaders.slots[0].leader_flags = 3;
        let mut production = LiveProductionRuntime::default();
        production.register_build(0, CITY_TYPE);
        production.register_build(1, FARM);
        production.register_build(2, FARM_DERIVED);
        let receipt =
            apply_sim_city_building_count_transaction(&loaded, &production, &types, request(0))
                .unwrap();
        assert_eq!(receipt.returned, 2);
        assert_eq!(receipt.candidates.len(), 3);
        assert_eq!(loaded.cities.slots[0][0].name, "Athens");
        assert_eq!(loaded.builds[0].city_down, BUILD_BAND_BASE as i16 + 1);
    }
}
