//! Exact owned prefix for BHS builtin 520, `place_building_with_cost`.
//!
//! The 123-byte wrapper at `0x009F54A0` validates the one-based Leader, resolves a live
//! City, and delegates its center object to builtin 521.  The 348-byte builtin-521 body
//! validates that Build, resolves a Build Type, derives the captured-City constraint, and
//! calls `TypeData::can_pay_cost` (`0x00667570`).  A zero cost result returns zero.  A
//! nonzero result enters `Leader::produce_building` (`0x006E1400`, 7,406 bytes).
//!
//! This module owns that complete read-only prefix.  Dynamic `TypeData::get_cost` results
//! and `LeaderData::is_possible_good` answers are admitted as a revisioned PE after-image;
//! live resources still come from, and are reconciled across, the canonical production and
//! Sim Leader owners.  The success receipt deliberately stops before `produce_building`:
//! that body owns placement search, Build initialization, City/World links, builder Group
//! selection, resource payment, and BUILD_AT orders.  Returning a scalar success without
//! those mutations would be a simulation bug.

use crate::objects::{Band, BUILD_BAND_BASE, WALL_BAND_BASE};
use crate::tick::Sim;

use super::bhs_type_table::{TypeBuiltinState, TypeDomain, NUM_LEADERS, NUM_TYPES};
use super::production::{flag, runtime::LiveProductionRuntime, runtime::LiveTypeClass};

pub const PLACE_BUILDING_WITH_COST_BUILTIN: u32 = 520;
pub const PLACE_ORPHAN_BUILDING_WITH_COST_BUILTIN: u32 = 521;
pub const PLACE_BUILDING_WITH_COST_VA: u32 = 0x009f_54a0;
pub const PLACE_ORPHAN_BUILDING_WITH_COST_VA: u32 = 0x009f_5520;
pub const GET_CITY_INDEX_VA: u32 = 0x009e_2ba0;
pub const VALID_BUILD_O_VA: u32 = 0x009e_3300;
pub const GET_TYPE_INDEX_VA: u32 = 0x00a0_3480;
pub const TYPE_CAN_PAY_COST_VA: u32 = 0x0066_7570;
pub const LEADER_PRODUCE_BUILDING_VA: u32 = 0x006e_1400;
pub const LEADER_PRODUCE_BUILDING_BYTES: u32 = 7_406;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceBuildingRequest {
    /// Retail one-based Leader argument.
    pub who: i32,
    pub type_name: String,
    pub city_name: String,
}

/// Provenance of the six `TypeData::get_cost` calls and possible-good gates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceBuildingCostSource {
    TypeDataCanPayCostPeAfterImage,
}

/// One exact dynamic-cost after-image, selected by every input that reaches `can_pay_cost`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceBuildingCostEntry {
    pub owner: u8,
    pub type_index: i32,
    pub origin_build_object: i16,
    /// `-1` for an ordinary center; captured active centers contribute `BuildData::city`.
    pub city_constraint: i32,
    pub source: PlaceBuildingCostSource,
    /// Exact `LeaderData::is_possible_good(good, 1)` result in good-index order.
    pub possible_goods: [bool; 6],
    /// Exact `TypeData::get_cost(good, owner, city_constraint, ..., 1)` results.
    pub resolved_costs: [i32; 6],
}

/// Installed search/cost authority. A zero revision or digest never authorizes a call.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlaceBuildingCostAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub entries: Vec<PlaceBuildingCostEntry>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceBuildingStatus {
    InvalidLeader,
    InactiveLeader,
    InvalidCity,
    InvalidOriginBuild,
    InvalidType,
    Unaffordable,
    /// Every owned gate passed. No scalar result exists until the native transaction is owned.
    ReadyForLeaderProduceBuilding,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderProduceBuildingBoundary {
    pub va: u32,
    pub bytes: u32,
    pub owner: u8,
    pub type_index: i32,
    pub origin_build_object: i16,
    /// Literal third argument passed by builtin 521.
    pub mode: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceBuildingReceipt {
    pub request: PlaceBuildingRequest,
    pub status: PlaceBuildingStatus,
    pub owner: Option<usize>,
    pub city_slot: Option<usize>,
    pub origin_build_row: Option<usize>,
    pub origin_build_object: Option<i16>,
    pub type_index: Option<usize>,
    pub city_constraint: Option<i32>,
    pub authority_revision: Option<u64>,
    pub possible_goods: Option<[bool; 6]>,
    pub resolved_costs: Option<[i32; 6]>,
    pub resources: Option<[i32; 6]>,
    /// Exact normal-mode `TypeData::can_pay_cost` result; builtin 521 tests only zero/nonzero.
    pub can_pay_result: Option<i32>,
    pub continuation: Option<LeaderProduceBuildingBoundary>,
    /// `Some(-1)` and `Some(0)` are complete retail terminal arms. Ready is `None`.
    pub returned: Option<i32>,
}

impl PlaceBuildingReceipt {
    fn rejected(
        request: PlaceBuildingRequest,
        status: PlaceBuildingStatus,
        owner: Option<usize>,
        city_slot: Option<usize>,
        origin_build_row: Option<usize>,
    ) -> Self {
        Self {
            request,
            status,
            owner,
            city_slot,
            origin_build_row,
            origin_build_object: None,
            type_index: None,
            city_constraint: None,
            authority_revision: None,
            possible_goods: None,
            resolved_costs: None,
            resources: None,
            can_pay_result: None,
            continuation: None,
            returned: Some(-1),
        }
    }

    pub fn validates(&self) -> bool {
        match self.status {
            PlaceBuildingStatus::InvalidLeader
            | PlaceBuildingStatus::InactiveLeader
            | PlaceBuildingStatus::InvalidCity
            | PlaceBuildingStatus::InvalidOriginBuild
            | PlaceBuildingStatus::InvalidType => {
                self.returned == Some(-1)
                    && self.can_pay_result.is_none()
                    && self.continuation.is_none()
            }
            PlaceBuildingStatus::Unaffordable => {
                self.owner.is_some()
                    && self.city_slot.is_some()
                    && self.origin_build_row.is_some()
                    && self.type_index.is_some()
                    && self.can_pay_result == Some(0)
                    && self.continuation.is_none()
                    && self.returned == Some(0)
            }
            PlaceBuildingStatus::ReadyForLeaderProduceBuilding => {
                self.owner.is_some()
                    && self.city_slot.is_some()
                    && self.origin_build_row.is_some()
                    && self.type_index.is_some()
                    && self.can_pay_result.is_some_and(|result| result != 0)
                    && self.continuation.is_some()
                    && self.returned.is_none()
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaceBuildingError {
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
        owner: usize,
        city_slot: usize,
        city_owner: i8,
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
    TypeOwnerMismatch {
        type_index: usize,
    },
    LeaderResourceMirrorMismatch {
        owner: usize,
    },
    MissingAuthority,
    MissingAuthorityRevision,
    MissingCompositionDigest,
    AmbiguousAuthority,
    InvalidCostAuthority,
}

fn string_eq(left: &str, right: &str) -> Result<bool, PlaceBuildingError> {
    if !left.is_ascii() || !right.is_ascii() {
        return Err(PlaceBuildingError::NonAsciiQuery);
    }
    Ok(left.len() == right.len() && left.eq_ignore_ascii_case(right))
}

fn first_type(types: &TypeBuiltinState, query: &str) -> Result<Option<usize>, PlaceBuildingError> {
    if !query.is_ascii() {
        return Err(PlaceBuildingError::NonAsciiQuery);
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

fn normal_can_pay_result(
    possible_goods: [bool; 6],
    costs: [i32; 6],
    resources: [i32; 6],
) -> Result<i32, PlaceBuildingError> {
    if costs.iter().any(|&cost| cost < 0) {
        return Err(PlaceBuildingError::InvalidCostAuthority);
    }
    let mut result = -1;
    for good in 0..6 {
        if !possible_goods[good] || costs[good] == 0 {
            continue;
        }
        let quotient = resources[good].wrapping_div(costs[good]);
        if quotient == 0 {
            return Ok(0);
        }
        result = result.max(quotient);
    }
    Ok(if result == -1 { 10 } else { result })
}

/// Execute every owned #520/#521 gate without mutating any canonical owner.
pub fn apply_sim_place_building_with_cost_prefix(
    sim: &Sim,
    production: &LiveProductionRuntime,
    types: &TypeBuiltinState,
    authority: &PlaceBuildingCostAuthority,
    request: PlaceBuildingRequest,
) -> Result<PlaceBuildingReceipt, PlaceBuildingError> {
    let owner0 = request.who.wrapping_sub(1);
    if !(0..NUM_LEADERS as i32).contains(&owner0) {
        return Ok(PlaceBuildingReceipt::rejected(
            request,
            PlaceBuildingStatus::InvalidLeader,
            None,
            None,
            None,
        ));
    }
    let owner = owner0 as usize;
    let type_flags = types.leaders[owner].leader_flags;
    let victory_flags = sim.vic_leaders.slots[owner].leader_flags;
    let step8_flags = sim.step8.leaders[owner].flags;
    if type_flags != victory_flags || type_flags as u32 != step8_flags {
        return Err(PlaceBuildingError::LeaderMirrorMismatch {
            owner,
            type_flags,
            victory_flags,
            step8_flags,
        });
    }
    if type_flags & 3 != 3 {
        return Ok(PlaceBuildingReceipt::rejected(
            request,
            PlaceBuildingStatus::InactiveLeader,
            Some(owner),
            None,
            None,
        ));
    }

    let mark = sim.cities.city_mark[owner];
    let mark_usize = usize::try_from(mark).map_err(|_| PlaceBuildingError::InvalidCityMark {
        owner,
        mark,
        slots: sim.cities.slots[owner].len(),
    })?;
    if mark_usize > sim.cities.slots[owner].len() {
        return Err(PlaceBuildingError::InvalidCityMark {
            owner,
            mark,
            slots: sim.cities.slots[owner].len(),
        });
    }
    let mut city_slot = None;
    for (slot, city) in sim.cities.slots[owner][..mark_usize].iter().enumerate() {
        if city.active()
            && (string_eq(&city.id, &request.city_name)?
                || string_eq(&city.name, &request.city_name)?)
        {
            city_slot = Some(slot);
            break;
        }
    }
    let Some(city_slot) = city_slot else {
        return Ok(PlaceBuildingReceipt::rejected(
            request,
            PlaceBuildingStatus::InvalidCity,
            Some(owner),
            None,
            None,
        ));
    };
    let city = &sim.cities.slots[owner][city_slot];
    if city.who != owner as i8 {
        return Err(PlaceBuildingError::InvalidCityOwner {
            owner,
            city_slot,
            city_owner: city.who,
        });
    }

    let object_index = city.o;
    if !(BUILD_BAND_BASE as i16..WALL_BAND_BASE as i16).contains(&object_index) {
        return Ok(PlaceBuildingReceipt::rejected(
            request,
            PlaceBuildingStatus::InvalidOriginBuild,
            Some(owner),
            Some(city_slot),
            None,
        ));
    }
    let object_slot = (object_index - BUILD_BAND_BASE as i16) as usize;
    let Some(&build_row) = sim
        .world
        .objects
        .slot(owner)
        .band(Band::Build)
        .get(object_slot)
    else {
        return Ok(PlaceBuildingReceipt::rejected(
            request,
            PlaceBuildingStatus::InvalidOriginBuild,
            Some(owner),
            Some(city_slot),
            None,
        ));
    };
    let build_row = build_row as usize;
    let Some(build) = sim.builds.get(build_row) else {
        return Err(PlaceBuildingError::BuildRowOutOfRange {
            owner,
            object_index,
            row: build_row,
            builds: sim.builds.len(),
        });
    };
    if build.who as usize != owner || build.object_id() != object_index {
        return Err(PlaceBuildingError::BuildIdentityMismatch {
            owner,
            object_index,
            row: build_row,
            build_owner: build.who,
            build_object: build.object_id(),
        });
    }
    if build.flags & flag::VALID == 0 {
        return Ok(PlaceBuildingReceipt::rejected(
            request,
            PlaceBuildingStatus::InvalidOriginBuild,
            Some(owner),
            Some(city_slot),
            Some(build_row),
        ));
    }

    let Some(type_index) = first_type(types, &request.type_name)? else {
        return Ok(PlaceBuildingReceipt::rejected(
            request,
            PlaceBuildingStatus::InvalidType,
            Some(owner),
            Some(city_slot),
            Some(build_row),
        ));
    };
    if type_index >= NUM_TYPES || types.types.row(type_index).domain() != TypeDomain::Build {
        return Ok(PlaceBuildingReceipt::rejected(
            request,
            PlaceBuildingStatus::InvalidType,
            Some(owner),
            Some(city_slot),
            Some(build_row),
        ));
    }
    let Some(type_facts) = production.types.get(type_index).and_then(Option::as_ref) else {
        return Err(PlaceBuildingError::TypeOwnerMismatch { type_index });
    };
    if type_facts.type_index != type_index as i32 || type_facts.class != LiveTypeClass::Building {
        return Err(PlaceBuildingError::TypeOwnerMismatch { type_index });
    }

    let city_constraint =
        if build.flags & (flag::ACTIVE | flag::CAPTURED) == (flag::ACTIVE | flag::CAPTURED) {
            i32::from(build.city)
        } else {
            -1
        };
    if authority.revision == 0 {
        return Err(PlaceBuildingError::MissingAuthorityRevision);
    }
    if authority.composition_digest == [0; 32] {
        return Err(PlaceBuildingError::MissingCompositionDigest);
    }
    let mut entries = authority.entries.iter().filter(|entry| {
        entry.owner as usize == owner
            && entry.type_index == type_index as i32
            && entry.origin_build_object == object_index
            && entry.city_constraint == city_constraint
    });
    let Some(cost) = entries.next() else {
        return Err(PlaceBuildingError::MissingAuthority);
    };
    if entries.next().is_some() {
        return Err(PlaceBuildingError::AmbiguousAuthority);
    }
    if cost.source != PlaceBuildingCostSource::TypeDataCanPayCostPeAfterImage {
        return Err(PlaceBuildingError::InvalidCostAuthority);
    }

    let resources = production.leaders[owner].resources;
    if resources != sim.leaders[owner].econ.stockpile
        || resources != sim.step8.leaders[owner].econ.stockpile
        || resources != sim.vic_leaders.slots[owner].economy.bucket
    {
        return Err(PlaceBuildingError::LeaderResourceMirrorMismatch { owner });
    }
    let can_pay_result =
        normal_can_pay_result(cost.possible_goods, cost.resolved_costs, resources)?;
    let (status, continuation, returned) = if can_pay_result == 0 {
        (PlaceBuildingStatus::Unaffordable, None, Some(0))
    } else {
        (
            PlaceBuildingStatus::ReadyForLeaderProduceBuilding,
            Some(LeaderProduceBuildingBoundary {
                va: LEADER_PRODUCE_BUILDING_VA,
                bytes: LEADER_PRODUCE_BUILDING_BYTES,
                owner: owner as u8,
                type_index: type_index as i32,
                origin_build_object: object_index,
                mode: 0,
            }),
            None,
        )
    };
    let receipt = PlaceBuildingReceipt {
        request,
        status,
        owner: Some(owner),
        city_slot: Some(city_slot),
        origin_build_row: Some(build_row),
        origin_build_object: Some(object_index),
        type_index: Some(type_index),
        city_constraint: Some(city_constraint),
        authority_revision: Some(authority.revision),
        possible_goods: Some(cost.possible_goods),
        resolved_costs: Some(cost.resolved_costs),
        resources: Some(resources),
        can_pay_result: Some(can_pay_result),
        continuation,
        returned,
    };
    debug_assert!(receipt.validates());
    Ok(receipt)
}
