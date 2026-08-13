//! Exact frame-zero Citizen census for the four City bytes written by
//! `Leader::plan_strategy`.
//!
//! Retail clears every active City's `peasant_dist`, `free`, `busy`, and `gatherers`, then
//! walks the owning Leader's Unit band. Only type 50/51 (Citizen/Korean Citizen) reaches
//! the City mutation. An empty `UnitData::get_action` is the free arm; the other arms need
//! concrete order payloads and remain a typed refusal here.
//!
//! This transaction does not infer starting Units from the number of players. Its authority
//! is constructed from the already validated `Setup::build_units` plan/receipt pair, then
//! joined to the canonical sparse Unit identities, generated columns, ptype projection,
//! replay-carried `UnitTypeData::control_cost`, World region plane, and Sim-owned City pool.
//! No recorded checksum is an input.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use don_sim::systems::map_terrain::{Coord, WCoord};
use don_sim::systems::tech_cities::{self, CityRecord, NUM_PLAYERS};
use don_sim::tick::Sim;
use don_sim::world::Handle;

use crate::cities_runtime::{check_sim_owned_cities, CitiesRuntimeError};
use crate::groups_pre_pair_unit_authority::ReplayUnitTypeFacts;
use crate::setup_units_producer::{
    validate_build_units_prefix_receipt, BuildUnitsPlan, BuildUnitsPrefixReceipt,
    BuildUnitsReceiptError, PlacementOutcomeReceipt, StableUnitIdentityReceipt, StartingUnitPhase,
};

pub const LEADER_PLAN_STRATEGY_VA: u32 = 0x006b_9620;
pub const CITY_CENSUS_CLEAR_LOOP_BEGIN_VA: u32 = 0x006b_9746;
pub const CITY_CENSUS_UNIT_LOOP_BEGIN_VA: u32 = 0x006b_9e2c;
pub const OBJECTS_FIND_CITY_VA: u32 = 0x0065_ba90;
pub const UNIT_GET_ACTION_VA: u32 = 0x0060_8450;
pub const VECTOR_DIST_VA: u32 = 0x0046_cff0;

pub const CITIZEN_TYPE: i32 = 50;
pub const KOREAN_CITIZEN_TYPE: i32 = 51;
pub const INITIAL_PEASANT_DIST: i16 = 100;
pub const COORDS_PER_WCOORD: i32 = 0x300;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartingCityUnitCensusSource {
    /// Exact `Setup::build_units` schedule plus its source-produced allocation receipts.
    ValidatedBuildUnitsReceipts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ExpectedStartingUnit {
    identity: StableUnitIdentityReceipt,
    phase: StartingUnitPhase,
    type_index: i32,
}

/// Revision-free immutable join of setup allocation receipts and replay-carried type rows.
///
/// The inputs themselves carry their source spans and stable allocation identities; the
/// constructor revalidates each complete plan/receipt and the empty-owner allocation sequence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartingCityUnitCensusAuthority {
    pub source: StartingCityUnitCensusSource,
    units: BTreeMap<(usize, i32), ExpectedStartingUnit>,
    types: BTreeMap<i32, ReplayUnitTypeFacts>,
    owners: BTreeSet<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartingCityUnitCensusAuthorityError {
    SetupCountMismatch {
        plans: usize,
        receipts: usize,
    },
    SetupReceipt {
        setup: usize,
        source: BuildUnitsReceiptError,
    },
    StoppedSetupPlan {
        setup: usize,
    },
    EmptySetupPlan {
        setup: usize,
    },
    MixedSetupOwner {
        setup: usize,
    },
    OwnerOutOfRange {
        setup: usize,
        owner: i32,
    },
    DuplicateOwner {
        owner: usize,
    },
    NonSpawnedStartingCall {
        setup: usize,
        ordinal: usize,
    },
    NonSequentialAllocation {
        setup: usize,
        ordinal: usize,
        expected_mark: i32,
    },
    DuplicateStartingUnit {
        owner: usize,
        o: i32,
    },
    MissingTypeFacts {
        type_index: i32,
    },
    DuplicateTypeFacts {
        type_index: i32,
    },
}

impl fmt::Display for StartingCityUnitCensusAuthorityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "starting City Unit-census authority refused: {self:?}")
    }
}

impl std::error::Error for StartingCityUnitCensusAuthorityError {}

impl StartingCityUnitCensusAuthority {
    /// Bind complete ordinary setup schedules to exact replay-carried UnitType rows.
    ///
    /// The supported setup begins from an empty per-owner Unit band, so every successful
    /// `Objects::init_unit` span must consume a consecutive range beginning at object zero.
    pub fn from_build_units(
        plans: &[BuildUnitsPlan],
        receipts: &[BuildUnitsPrefixReceipt],
        type_facts: &[ReplayUnitTypeFacts],
    ) -> Result<Self, StartingCityUnitCensusAuthorityError> {
        if plans.len() != receipts.len() {
            return Err(StartingCityUnitCensusAuthorityError::SetupCountMismatch {
                plans: plans.len(),
                receipts: receipts.len(),
            });
        }
        let mut types = BTreeMap::new();
        for facts in type_facts {
            if types.insert(facts.type_index, *facts).is_some() {
                return Err(StartingCityUnitCensusAuthorityError::DuplicateTypeFacts {
                    type_index: facts.type_index,
                });
            }
        }

        let mut units = BTreeMap::new();
        let mut owners = BTreeSet::new();
        for (setup, (plan, receipt)) in plans.iter().zip(receipts).enumerate() {
            validate_build_units_prefix_receipt(plan, receipt).map_err(|source| {
                StartingCityUnitCensusAuthorityError::SetupReceipt { setup, source }
            })?;
            if plan.stop.is_some() {
                return Err(StartingCityUnitCensusAuthorityError::StoppedSetupPlan { setup });
            }
            let Some(first) = plan.calls.first() else {
                return Err(StartingCityUnitCensusAuthorityError::EmptySetupPlan { setup });
            };
            if plan.calls.iter().any(|call| call.owner != first.owner) {
                return Err(StartingCityUnitCensusAuthorityError::MixedSetupOwner { setup });
            }
            let owner = usize::try_from(first.owner).map_err(|_| {
                StartingCityUnitCensusAuthorityError::OwnerOutOfRange {
                    setup,
                    owner: first.owner,
                }
            })?;
            if owner >= NUM_PLAYERS {
                return Err(StartingCityUnitCensusAuthorityError::OwnerOutOfRange {
                    setup,
                    owner: first.owner,
                });
            }
            if !owners.insert(owner) {
                return Err(StartingCityUnitCensusAuthorityError::DuplicateOwner { owner });
            }

            let mut expected_mark = 0i32;
            for (ordinal, (call, placement)) in
                plan.calls.iter().zip(&receipt.placements).enumerate()
            {
                let PlacementOutcomeReceipt::Spawned(init) = &placement.outcome else {
                    return Err(
                        StartingCityUnitCensusAuthorityError::NonSpawnedStartingCall {
                            setup,
                            ordinal,
                        },
                    );
                };
                let member_count = i32::try_from(init.members.len()).unwrap_or(i32::MAX);
                if init.unit_mark_before != expected_mark
                    || init.unit_mark_after != expected_mark.checked_add(member_count).unwrap_or(-1)
                    || init.returned_captain_o != expected_mark
                {
                    return Err(
                        StartingCityUnitCensusAuthorityError::NonSequentialAllocation {
                            setup,
                            ordinal,
                            expected_mark,
                        },
                    );
                }
                for (member_index, member) in init.members.iter().enumerate() {
                    let expected_o = expected_mark + member_index as i32;
                    if member.identity.o != expected_o {
                        return Err(
                            StartingCityUnitCensusAuthorityError::NonSequentialAllocation {
                                setup,
                                ordinal,
                                expected_mark,
                            },
                        );
                    }
                    if !types.contains_key(&member.ptype_index) {
                        return Err(StartingCityUnitCensusAuthorityError::MissingTypeFacts {
                            type_index: member.ptype_index,
                        });
                    }
                    let expected = ExpectedStartingUnit {
                        identity: member.identity,
                        phase: call.phase,
                        type_index: member.ptype_index,
                    };
                    if units.insert((owner, expected_o), expected).is_some() {
                        return Err(
                            StartingCityUnitCensusAuthorityError::DuplicateStartingUnit {
                                owner,
                                o: expected_o,
                            },
                        );
                    }
                }
                expected_mark = init.unit_mark_after;
            }
        }
        Ok(Self {
            source: StartingCityUnitCensusSource::ValidatedBuildUnitsReceipts,
            units,
            types,
            owners,
        })
    }

    pub fn unit_count(&self) -> usize {
        self.units.len()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartingCityUnitDisposition {
    NonCitizen,
    MaskedOut,
    ZeroControlCost,
    FreeCitizenNoCity,
    FreeCitizenAtCity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartingCityUnitReceipt {
    pub owner: usize,
    pub o: i32,
    pub row: usize,
    pub handle: Handle,
    pub phase: StartingUnitPhase,
    pub type_index: i32,
    pub control_cost: i32,
    pub disposition: StartingCityUnitDisposition,
    pub city_slot: Option<usize>,
    pub distance_coord: Option<i32>,
    pub distance_wcoord: Option<i16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartingCityCensusMutationReceipt {
    pub owner: usize,
    pub slot: usize,
    pub before: [u8; tech_cities::CITY_POD_LEN],
    pub after: [u8; tech_cities::CITY_POD_LEN],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartingCityUnitCensusReceipt {
    pub source: StartingCityUnitCensusSource,
    pub owners_walked: u32,
    pub cities_cleared: u32,
    pub units_walked: u32,
    pub scouts_joined: u32,
    pub citizens_joined: u32,
    pub free_citizens: u32,
    pub busy_citizens: u32,
    pub gatherers: u32,
    pub city_assignments: u32,
    pub units: Vec<StartingCityUnitReceipt>,
    pub cities: Vec<StartingCityCensusMutationReceipt>,
    pub city_pod_bytes_changed: u32,
    pub city_unit_census_complete: bool,
    /// Terrain census and the remaining pre-checkpoint setup schedule are separate owners.
    pub first_checksum_city_image_ready: bool,
    pub world_writes: u32,
    pub build_or_registry_writes: u32,
    pub main_rng_draws: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartingCityUnitCensusError {
    Cities(CitiesRuntimeError),
    NonDenseUnitOwner,
    UnitProjectionLength {
        world_rows: usize,
        sim_rows: usize,
    },
    ActiveOwnerMissingAuthority {
        owner: usize,
    },
    AuthorityOwnerInactive {
        owner: usize,
    },
    UnitMarkMismatch {
        owner: usize,
        expected: i32,
        actual: i32,
    },
    MissingCanonicalUnit {
        owner: usize,
        o: i32,
    },
    CanonicalHandleMismatch {
        owner: usize,
        o: i32,
        expected: Handle,
        actual: Option<Handle>,
    },
    CanonicalTypeMismatch {
        owner: usize,
        o: i32,
        receipt: i32,
        world: Option<i32>,
        sim: Option<i32>,
    },
    CanonicalIdentityMismatch {
        owner: usize,
        o: i32,
        row_owner: u8,
        row_o: i16,
    },
    InactiveStartingUnit {
        owner: usize,
        o: i32,
        flags: u8,
    },
    UnsupportedCitizenContainment {
        owner: usize,
        o: i32,
        inside_up: i16,
    },
    UnsupportedCitizenOrder {
        owner: usize,
        o: i32,
        order: don_sim::order::OrderIndex,
    },
    UnitOutsideWorld {
        owner: usize,
        o: i32,
        x: i32,
        y: i32,
    },
    DistanceOutsideSignedRange {
        owner: usize,
        o: i32,
        distance: u32,
    },
    CountOverflow,
}

impl fmt::Display for StartingCityUnitCensusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "starting City Unit census refused: {self:?}")
    }
}

impl std::error::Error for StartingCityUnitCensusError {}

impl From<CitiesRuntimeError> for StartingCityUnitCensusError {
    fn from(value: CitiesRuntimeError) -> Self {
        Self::Cities(value)
    }
}

fn add_count(value: &mut u32) -> Result<(), StartingCityUnitCensusError> {
    *value = value
        .checked_add(1)
        .ok_or(StartingCityUnitCensusError::CountOverflow)?;
    Ok(())
}

fn active_city_mark(sim: &Sim, owner: usize) -> usize {
    // `check_sim_owned_cities` already proved nonnegative, in-bounds marks.
    sim.cities.city_mark[owner] as usize
}

fn closest_same_region_city(
    cities: &[CityRecord],
    mark: usize,
    owner: usize,
    o: i32,
    region: i16,
    x: i32,
    y: i32,
) -> Result<Option<(usize, i32)>, StartingCityUnitCensusError> {
    let mut closest = None;
    let mut closest_distance = 99_999_999i32;
    for (slot, city) in cities.iter().take(mark).enumerate() {
        if !city.active() || city.reg != region {
            continue;
        }
        let distance = tech_cities::vector_dist(x.wrapping_sub(city.x), y.wrapping_sub(city.y));
        let distance = i32::try_from(distance).map_err(|_| {
            StartingCityUnitCensusError::DistanceOutsideSignedRange { owner, o, distance }
        })?;
        // `ObjectsData::find_city` uses `<=`: the later City wins an exact tie.
        if distance <= closest_distance {
            closest_distance = distance;
            closest = Some((slot, distance));
        }
    }
    Ok(closest)
}

/// Execute the source-owned portion of the first frame's `Leader::plan_strategy` City census.
///
/// The mutation is atomic. All setup receipts, stable Unit identities, ptype joins, World
/// coordinates/regions, Citizen order states, and City joins are preflighted against a cloned
/// pool. `Sim::cities` is replaced only after every active Leader succeeds.
pub fn apply_starting_city_unit_census(
    sim: &mut Sim,
    authority: &StartingCityUnitCensusAuthority,
) -> Result<StartingCityUnitCensusReceipt, StartingCityUnitCensusError> {
    check_sim_owned_cities(sim)?;
    if !sim.world.object_bands_are_dense_equivalent() {
        return Err(StartingCityUnitCensusError::NonDenseUnitOwner);
    }
    let world_rows = sim.world.units.len();
    if sim.unit_type.len() != world_rows {
        return Err(StartingCityUnitCensusError::UnitProjectionLength {
            world_rows,
            sim_rows: sim.unit_type.len(),
        });
    }
    for owner in 0..NUM_PLAYERS {
        if sim.leaders[owner].active && !authority.owners.contains(&owner) {
            return Err(StartingCityUnitCensusError::ActiveOwnerMissingAuthority { owner });
        }
    }
    for &owner in &authority.owners {
        if !sim.leaders[owner].active {
            return Err(StartingCityUnitCensusError::AuthorityOwnerInactive { owner });
        }
    }

    let mut staged = sim.cities.clone();
    let mut receipt = StartingCityUnitCensusReceipt {
        source: authority.source,
        owners_walked: 0,
        cities_cleared: 0,
        units_walked: 0,
        scouts_joined: 0,
        citizens_joined: 0,
        free_citizens: 0,
        busy_citizens: 0,
        gatherers: 0,
        city_assignments: 0,
        units: Vec::with_capacity(authority.units.len()),
        cities: Vec::new(),
        city_pod_bytes_changed: 0,
        city_unit_census_complete: false,
        first_checksum_city_image_ready: false,
        world_writes: 0,
        build_or_registry_writes: 0,
        main_rng_draws: 0,
    };

    for &owner in &authority.owners {
        add_count(&mut receipt.owners_walked)?;
        let expected_mark = authority
            .units
            .range((owner, i32::MIN)..=(owner, i32::MAX))
            .next_back()
            .map_or(0, |((_, o), _)| o + 1);
        let actual_mark = sim.world.unit_mark(owner).unwrap_or(-1);
        if actual_mark != expected_mark {
            return Err(StartingCityUnitCensusError::UnitMarkMismatch {
                owner,
                expected: expected_mark,
                actual: actual_mark,
            });
        }

        let city_mark = active_city_mark(sim, owner);
        for slot in 0..city_mark {
            let city = &mut staged.slots[owner][slot];
            if !city.active() {
                continue;
            }
            city.peasant_dist = INITIAL_PEASANT_DIST;
            city.free = 0;
            city.busy = 0;
            city.gatherers = 0;
            add_count(&mut receipt.cities_cleared)?;
        }

        for o in 0..expected_mark {
            let expected = authority
                .units
                .get(&(owner, o))
                .expect("authority construction proved a consecutive allocation");
            let row = sim
                .world
                .unit_row_at(owner as i32, o)
                .ok_or(StartingCityUnitCensusError::MissingCanonicalUnit { owner, o })?;
            let expected_handle = Handle {
                id: expected.identity.id,
                generation: expected.identity.generation,
            };
            let actual_handle = sim.world.handle_at_row(row);
            if actual_handle != Some(expected_handle) {
                return Err(StartingCityUnitCensusError::CanonicalHandleMismatch {
                    owner,
                    o,
                    expected: expected_handle,
                    actual: actual_handle,
                });
            }
            let row_owner = sim.world.units.get_who(row);
            let row_o = sim.world.units.o()[row];
            if row_owner as usize != owner || i32::from(row_o) != o {
                return Err(StartingCityUnitCensusError::CanonicalIdentityMismatch {
                    owner,
                    o,
                    row_owner,
                    row_o,
                });
            }
            let flags = sim.world.units.get_flags(row);
            if flags & don_sim::world::OBJ_FLAG_ACTIVE == 0 {
                return Err(StartingCityUnitCensusError::InactiveStartingUnit { owner, o, flags });
            }
            let world_type = sim.world.unit_type_id(row);
            let sim_type = sim.unit_type.get(row).copied();
            if world_type != Some(expected.type_index) || sim_type != Some(expected.type_index) {
                return Err(StartingCityUnitCensusError::CanonicalTypeMismatch {
                    owner,
                    o,
                    receipt: expected.type_index,
                    world: world_type,
                    sim: sim_type,
                });
            }
            let type_facts = authority
                .types
                .get(&expected.type_index)
                .expect("authority construction joined every ptype");

            add_count(&mut receipt.units_walked)?;
            if matches!(
                expected.phase,
                StartingUnitPhase::BaseScout
                    | StartingUnitPhase::BonusScout { .. }
                    | StartingUnitPhase::BonusScoutRevealMap
            ) {
                add_count(&mut receipt.scouts_joined)?;
            }
            let citizen = matches!(expected.type_index, CITIZEN_TYPE | KOREAN_CITIZEN_TYPE);
            if citizen {
                add_count(&mut receipt.citizens_joined)?;
            }

            let mut unit_receipt = StartingCityUnitReceipt {
                owner,
                o,
                row,
                handle: expected_handle,
                phase: expected.phase,
                type_index: expected.type_index,
                control_cost: type_facts.control_cost,
                disposition: StartingCityUnitDisposition::NonCitizen,
                city_slot: None,
                distance_coord: None,
                distance_wcoord: None,
            };
            if sim.world.units.get_unit_masks(row) & 1 != 0 {
                unit_receipt.disposition = StartingCityUnitDisposition::MaskedOut;
                receipt.units.push(unit_receipt);
                continue;
            }
            if type_facts.control_cost == 0 {
                unit_receipt.disposition = StartingCityUnitDisposition::ZeroControlCost;
                receipt.units.push(unit_receipt);
                continue;
            }
            if !citizen {
                receipt.units.push(unit_receipt);
                continue;
            }

            let inside_up = sim.world.units.inside_up()[row];
            if inside_up >= 0 {
                return Err(StartingCityUnitCensusError::UnsupportedCitizenContainment {
                    owner,
                    o,
                    inside_up,
                });
            }
            let order = sim.world.orders(row).order_type();
            if order != don_sim::order::OrderIndex::None {
                return Err(StartingCityUnitCensusError::UnsupportedCitizenOrder {
                    owner,
                    o,
                    order,
                });
            }

            let x = sim.world.pos_x()[row];
            let y = sim.world.pos_y()[row];
            let wx = WCoord::from_coord(Coord(x)).0;
            let wy = WCoord::from_coord(Coord(y)).0;
            if wx < 0 || wy < 0 || wx >= sim.map.world.xs || wy >= sim.map.world.ys {
                return Err(StartingCityUnitCensusError::UnitOutsideWorld { owner, o, x, y });
            }
            let region = sim.map.world.wdata(wx, wy).region;
            let nearest =
                closest_same_region_city(&staged.slots[owner], city_mark, owner, o, region, x, y)?;
            add_count(&mut receipt.free_citizens)?;
            unit_receipt.disposition = StartingCityUnitDisposition::FreeCitizenNoCity;
            if let Some((slot, distance)) = nearest {
                let city = &mut staged.slots[owner][slot];
                city.free = city.free.wrapping_add(1);
                let distance_wcoord = (distance / COORDS_PER_WCOORD) as i16;
                if city.peasant_dist > distance_wcoord {
                    city.peasant_dist = distance_wcoord;
                }
                add_count(&mut receipt.city_assignments)?;
                unit_receipt.disposition = StartingCityUnitDisposition::FreeCitizenAtCity;
                unit_receipt.city_slot = Some(slot);
                unit_receipt.distance_coord = Some(distance);
                unit_receipt.distance_wcoord = Some(distance_wcoord);
            }
            receipt.units.push(unit_receipt);
        }
    }

    for &owner in &authority.owners {
        let mark = active_city_mark(sim, owner);
        for slot in 0..mark {
            let before = sim.cities.slots[owner][slot].pod_bytes();
            let after = staged.slots[owner][slot].pod_bytes();
            if !staged.slots[owner][slot].active() {
                continue;
            }
            receipt.city_pod_bytes_changed = receipt
                .city_pod_bytes_changed
                .checked_add(
                    before
                        .iter()
                        .zip(after.iter())
                        .filter(|(lhs, rhs)| lhs != rhs)
                        .count() as u32,
                )
                .ok_or(StartingCityUnitCensusError::CountOverflow)?;
            receipt.cities.push(StartingCityCensusMutationReceipt {
                owner,
                slot,
                before,
                after,
            });
        }
    }
    receipt.city_unit_census_complete = true;
    sim.cities = staged;
    Ok(receipt)
}
