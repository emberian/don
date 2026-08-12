//! Canonical live-Unit transaction for BHS builtin 455, `find_num_idle_unit`.
//!
//! Retail `ScenarioFuncSet::find_num_idle_unit` is the 449-byte body at `0x009F3390`.
//! After the ordered `Types[806]` name lookup and Leader validity gates it walks the owner's
//! Unit band and admits a row only in this instruction order:
//!
//! 1. `UnitData::is_valid_unit` (`vtable +0x08`, `0x0046CDA0`) — `flags & 1`;
//! 2. `UnitData::is_on_map` (`vtable +0xBC`, `0x0046CE30`) — bit 15 of `inside_up`;
//! 3. `ObjectData::is(requested_type, 0)` (`vtable +0xB8`, `0x00653790`);
//! 4. `UnitData::order_type() == NONE` (`0x00616E80`);
//! 5. `UnitData::is_captain()` (`vtable +0xE8`, `0x0046CEB0`) — `o_up < 0`.
//!
//! The transaction reads the canonical [`crate::tick::Sim`] Unit registry, columns, order
//! lists and type sidecar. It returns a receipt only after preflighting every reached row, so a
//! desynchronised projection cannot turn into a plausible scalar count.

use crate::objects::Band;
use crate::order::OrderIndex;
use crate::tick::Sim;
use crate::world::OBJ_FLAG_ACTIVE;

use super::bhs_type_table::{TypeBuiltinState, TypeDomain, NUM_LEADERS, NUM_TYPES, UNIT_END};

pub const FIND_NUM_IDLE_UNIT_BUILTIN: u32 = 455;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdleUnitCensusRequest {
    /// Retail's one-based Leader argument.
    pub who: i32,
    /// Internal Type name, compared case-insensitively in ascending table order.
    pub type_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdleUnitCensusStatus {
    Counted,
    InvalidType,
    InvalidLeader,
    InactiveLeader,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdleUnitCandidateReceipt {
    pub object_index: i32,
    pub row: usize,
    pub current_type: i32,
    pub valid_unit: bool,
    pub on_map: bool,
    pub type_match: bool,
    pub idle: bool,
    pub captain: bool,
    pub counted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdleUnitCensusReceipt {
    pub request: IdleUnitCensusRequest,
    pub status: IdleUnitCensusStatus,
    pub owner: Option<usize>,
    pub requested_type: Option<usize>,
    pub candidates: Vec<IdleUnitCandidateReceipt>,
    /// Exact builtin return: `-1` on validation failure, otherwise the live census.
    pub returned: i32,
}

impl IdleUnitCensusReceipt {
    fn rejected(
        request: IdleUnitCensusRequest,
        status: IdleUnitCensusStatus,
        owner: Option<usize>,
        requested_type: Option<usize>,
    ) -> Self {
        Self {
            request,
            status,
            owner,
            requested_type,
            candidates: Vec::new(),
            returned: -1,
        }
    }

    pub fn validates(&self) -> bool {
        match self.status {
            IdleUnitCensusStatus::Counted => {
                self.owner.is_some()
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
                            == (candidate.valid_unit
                                && candidate.on_map
                                && candidate.type_match
                                && candidate.idle
                                && candidate.captain)
                    })
            }
            IdleUnitCensusStatus::InvalidType => {
                self.requested_type.is_none() && self.candidates.is_empty() && self.returned == -1
            }
            IdleUnitCensusStatus::InvalidLeader => {
                self.owner.is_none() && self.candidates.is_empty() && self.returned == -1
            }
            IdleUnitCensusStatus::InactiveLeader => {
                self.owner.is_some() && self.candidates.is_empty() && self.returned == -1
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdleUnitCensusError {
    NonAsciiTypeName,
    LeaderMirrorMismatch {
        owner: usize,
        type_flags: i32,
        victory_flags: i32,
        step8_flags: u32,
    },
    UnitRowOutOfRange {
        owner: usize,
        object_index: usize,
        row: usize,
        unit_rows: usize,
    },
    UnitOwnerMismatch {
        owner: usize,
        object_index: usize,
        row: usize,
        row_owner: usize,
    },
    MissingUnitType {
        owner: usize,
        object_index: usize,
        row: usize,
    },
    UnitTypeMirrorMismatch {
        owner: usize,
        object_index: usize,
        row: usize,
        sim_type: i32,
        world_type: i32,
    },
    InvalidUnitType {
        owner: usize,
        object_index: usize,
        row: usize,
        current_type: i32,
    },
}

fn first_type(types: &TypeBuiltinState, query: &str) -> Result<Option<usize>, IdleUnitCensusError> {
    if !query.is_ascii() {
        return Err(IdleUnitCensusError::NonAsciiTypeName);
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

/// Execute builtin 455 as one read-only atomic transaction over the live Sim/type owners.
pub fn apply_sim_idle_unit_census_transaction(
    sim: &Sim,
    types: &TypeBuiltinState,
    request: IdleUnitCensusRequest,
) -> Result<IdleUnitCensusReceipt, IdleUnitCensusError> {
    // Retail resolves the Type string before validating the Leader argument.
    let Some(requested_type) = first_type(types, &request.type_name)? else {
        return Ok(IdleUnitCensusReceipt::rejected(
            request,
            IdleUnitCensusStatus::InvalidType,
            None,
            None,
        ));
    };

    let owner0 = request.who.wrapping_sub(1);
    if !(0..NUM_LEADERS as i32).contains(&owner0) {
        return Ok(IdleUnitCensusReceipt::rejected(
            request,
            IdleUnitCensusStatus::InvalidLeader,
            None,
            Some(requested_type),
        ));
    }
    let owner = owner0 as usize;
    let type_flags = types.leaders[owner].leader_flags;
    let victory_flags = sim.vic_leaders.slots[owner].leader_flags;
    let step8_flags = sim.step8.leaders[owner].flags;
    if type_flags != victory_flags || type_flags as u32 != step8_flags {
        return Err(IdleUnitCensusError::LeaderMirrorMismatch {
            owner,
            type_flags,
            victory_flags,
            step8_flags,
        });
    }
    if type_flags & 3 != 3 {
        return Ok(IdleUnitCensusReceipt::rejected(
            request,
            IdleUnitCensusStatus::InactiveLeader,
            Some(owner),
            Some(requested_type),
        ));
    }

    // `is_unit_type` precedes the explicit `[50, 414)` gate in retail.
    if requested_type < 50
        || requested_type >= UNIT_END
        || types.types.row(requested_type).domain() != TypeDomain::Unit
    {
        return Ok(IdleUnitCensusReceipt::rejected(
            request,
            IdleUnitCensusStatus::InvalidType,
            Some(owner),
            None,
        ));
    }

    let band = sim.world.objects.slot(owner).band(Band::Unit);
    let mut candidates = Vec::with_capacity(band.len());
    for (object_index, &raw_row) in band.iter().enumerate() {
        let row = raw_row as usize;
        if row >= sim.world.units.len() {
            return Err(IdleUnitCensusError::UnitRowOutOfRange {
                owner,
                object_index,
                row,
                unit_rows: sim.world.units.len(),
            });
        }
        let row_owner = sim.world.units.get_who(row) as usize;
        if row_owner != owner {
            return Err(IdleUnitCensusError::UnitOwnerMismatch {
                owner,
                object_index,
                row,
                row_owner,
            });
        }
        let Some(&current_type) = sim.unit_type.get(row) else {
            return Err(IdleUnitCensusError::MissingUnitType {
                owner,
                object_index,
                row,
            });
        };
        let world_type = sim.world.unit_type_id(row).expect("row was proved live");
        if current_type != world_type {
            return Err(IdleUnitCensusError::UnitTypeMirrorMismatch {
                owner,
                object_index,
                row,
                sim_type: current_type,
                world_type,
            });
        }
        let Ok(current_type_index) = usize::try_from(current_type) else {
            return Err(IdleUnitCensusError::InvalidUnitType {
                owner,
                object_index,
                row,
                current_type,
            });
        };
        if current_type_index >= NUM_TYPES
            || types.types.row(current_type_index).domain() != TypeDomain::Unit
        {
            return Err(IdleUnitCensusError::InvalidUnitType {
                owner,
                object_index,
                row,
                current_type,
            });
        }

        let valid_unit = sim.world.units.get_flags(row) & OBJ_FLAG_ACTIVE != 0;
        let on_map = sim.world.units.inside_up()[row] < 0;
        let type_match = types
            .types
            .row(current_type_index)
            .is_list
            .iter()
            .any(|&candidate| usize::from(candidate) == requested_type);
        let idle = sim.world.orders(row).order_type() == OrderIndex::None;
        let captain = sim.world.units.o_up()[row] < 0;
        let counted = valid_unit && on_map && type_match && idle && captain;
        candidates.push(IdleUnitCandidateReceipt {
            object_index: object_index as i32,
            row,
            current_type,
            valid_unit,
            on_map,
            type_match,
            idle,
            captain,
            counted,
        });
    }

    let returned = candidates
        .iter()
        .filter(|candidate| candidate.counted)
        .count() as i32;
    let receipt = IdleUnitCensusReceipt {
        request,
        status: IdleUnitCensusStatus::Counted,
        owner: Some(owner),
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
    use crate::order::Order;
    use crate::systems::bhs_type_table::{
        LeaderTypeMasks, TribeRoster, TypeBackup, TypeBuiltinState, TypeRow, TypeTable,
        BUILD_BEGIN, BUILD_END, NUM_TRIBES, REGULAR_UNIT_BEGIN, REGULAR_UNIT_END,
    };

    fn types() -> TypeBuiltinState {
        let mut rows: Vec<_> = (0..NUM_TYPES).map(TypeRow::empty).collect();
        rows[50].name = "Citizen".into();
        rows[51].name = "Citizen".into();
        rows[51].is_list.push(50);
        rows[52].name = "Scout".into();
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

    fn sim() -> Sim {
        let mut sim = Sim::new(455, 8);
        sim.step8.leaders[0].flags = 3;
        sim.vic_leaders.slots[0].leader_flags = 3;
        sim
    }

    #[test]
    fn counts_only_valid_on_map_related_idle_captains_in_owner_band_order() {
        let types = types();
        let mut sim = sim();
        let counted = sim.spawn_unit(0, 51, 0, 0, 1).unwrap();
        let busy = sim.spawn_unit(0, 50, 0, 0, 1).unwrap();
        let contained = sim.spawn_unit(0, 50, 0, 0, 1).unwrap();
        let member = sim.spawn_unit(0, 50, 0, 0, 1).unwrap();
        sim.spawn_unit(0, 52, 0, 0, 1).unwrap();
        let other_owner = sim.spawn_unit(1, 50, 0, 0, 1).unwrap();
        sim.world.issue(busy, Order::move_to(1, 1, 0));
        let contained_row = sim.world.row_of(contained).unwrap();
        sim.world.units.inside_up_mut()[contained_row] = 0;
        let member_row = sim.world.row_of(member).unwrap();
        sim.world.units.o_up_mut()[member_row] = 0;

        let receipt = apply_sim_idle_unit_census_transaction(
            &sim,
            &types,
            IdleUnitCensusRequest {
                who: 1,
                type_name: "cItIzEn".into(),
            },
        )
        .unwrap();

        assert_eq!(receipt.returned, 1);
        assert!(receipt.validates());
        assert_eq!(receipt.candidates.len(), 5);
        assert_eq!(
            receipt.candidates[0].row,
            sim.world.row_of(counted).unwrap()
        );
        assert!(receipt.candidates[0].counted);
        assert!(!receipt.candidates[1].idle);
        assert!(!receipt.candidates[2].on_map);
        assert!(!receipt.candidates[3].captain);
        assert!(!receipt.candidates[4].type_match);
        assert_eq!(
            sim.world
                .units
                .get_who(sim.world.row_of(other_owner).unwrap()),
            1
        );
    }

    #[test]
    fn duplicate_names_resolve_to_the_first_type_row() {
        let types = types();
        let mut sim = sim();
        sim.spawn_unit(0, 51, 0, 0, 1).unwrap();
        let receipt = apply_sim_idle_unit_census_transaction(
            &sim,
            &types,
            IdleUnitCensusRequest {
                who: 1,
                type_name: "Citizen".into(),
            },
        )
        .unwrap();
        assert_eq!(receipt.requested_type, Some(50));
        assert_eq!(
            receipt.returned, 1,
            "row 51 is related to first-match row 50"
        );
    }

    #[test]
    fn invalid_type_and_inactive_leader_return_minus_one_without_a_census() {
        let mut types = types();
        let mut sim = sim();
        let missing = apply_sim_idle_unit_census_transaction(
            &sim,
            &types,
            IdleUnitCensusRequest {
                who: 1,
                type_name: "missing".into(),
            },
        )
        .unwrap();
        assert_eq!(missing.status, IdleUnitCensusStatus::InvalidType);
        assert_eq!(missing.returned, -1);

        types.leaders[0].leader_flags = 1;
        sim.step8.leaders[0].flags = 1;
        sim.vic_leaders.slots[0].leader_flags = 1;
        let inactive = apply_sim_idle_unit_census_transaction(
            &sim,
            &types,
            IdleUnitCensusRequest {
                who: 1,
                type_name: "Citizen".into(),
            },
        )
        .unwrap();
        assert_eq!(inactive.status, IdleUnitCensusStatus::InactiveLeader);
        assert_eq!(inactive.returned, -1);
    }

    #[test]
    fn type_sidecar_desync_fails_closed_instead_of_returning_a_scalar() {
        let types = types();
        let mut sim = sim();
        sim.spawn_unit(0, 50, 0, 0, 1).unwrap();
        sim.unit_type[0] = 51;
        assert!(matches!(
            apply_sim_idle_unit_census_transaction(
                &sim,
                &types,
                IdleUnitCensusRequest {
                    who: 1,
                    type_name: "Citizen".into(),
                },
            ),
            Err(IdleUnitCensusError::UnitTypeMirrorMismatch { .. })
        ));
    }
}
