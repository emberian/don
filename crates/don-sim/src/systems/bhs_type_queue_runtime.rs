//! Canonical Build-queue census for BHS builtin 436, `num_type_queued`.
//!
//! Retail `ScenarioFuncSet::num_type_queued` is the 499-byte handler at `0x009F25E0`.
//! It validates a one-based Leader and a live Build object through `valid_build_o`
//! (`0x009E3300`), then walks `BuildData::queued` logical slots. An empty type string or
//! any string beginning with a space counts every non-empty slot. Otherwise retail resolves
//! the first internal Type name through `get_type_index(..., 0)` (`0x00A03480`) and applies
//! the non-strict `TypeData::is(requested, 0)` relation to each queued Type.
//!
//! This adapter reads the canonical World Build band, `Sim::builds` queue records, and the
//! canonical BHS Type table in one preflighted, read-only transaction. It does not consult or
//! reconstruct Leader aggregate queue counters because the retail handler does not use them.

use crate::objects::{Band, BUILD_BAND_BASE, WALL_BAND_BASE};
use crate::tick::Sim;

use super::bhs_type_table::{TypeBuiltinState, NUM_LEADERS, NUM_TYPES};
use super::production::flag;

pub const NUM_TYPE_QUEUED_BUILTIN: u32 = 436;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeQueueCountRequest {
    /// Retail one-based Leader argument.
    pub who: i32,
    /// Owner-local Build object index (`2000..2999`).
    pub build_object: i32,
    /// Internal Type name. Empty or beginning with a space is retail's wildcard form.
    pub type_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeQueueCountStatus {
    Counted,
    InvalidLeader,
    InactiveLeader,
    InvalidBuild,
    InvalidType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeQueueCandidateReceipt {
    pub slot: usize,
    /// `None` is the exact `BuildQueue::type_at(slot) == -1` sentinel.
    pub current_type: Option<i32>,
    pub type_match: bool,
    pub counted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeQueueCountReceipt {
    pub request: TypeQueueCountRequest,
    pub status: TypeQueueCountStatus,
    pub requested_owner: Option<usize>,
    pub build_row: Option<usize>,
    pub wildcard: bool,
    pub requested_type: Option<usize>,
    pub logical_slots: usize,
    pub allocated_slots: usize,
    pub candidates: Vec<TypeQueueCandidateReceipt>,
    pub returned: i32,
}

impl TypeQueueCountReceipt {
    fn rejected(
        request: TypeQueueCountRequest,
        status: TypeQueueCountStatus,
        requested_owner: Option<usize>,
        build_row: Option<usize>,
        wildcard: bool,
    ) -> Self {
        Self {
            request,
            status,
            requested_owner,
            build_row,
            wildcard,
            requested_type: None,
            logical_slots: 0,
            allocated_slots: 0,
            candidates: Vec::new(),
            returned: -1,
        }
    }

    pub fn validates(&self) -> bool {
        match self.status {
            TypeQueueCountStatus::Counted => {
                self.requested_owner.is_some()
                    && self.build_row.is_some()
                    && (self.wildcard == self.requested_type.is_none())
                    && self.candidates.len() == self.logical_slots
                    && self.returned >= 0
                    && self.returned
                        == self
                            .candidates
                            .iter()
                            .filter(|candidate| candidate.counted)
                            .count() as i32
                    && self.candidates.iter().all(|candidate| {
                        candidate.counted
                            == if self.wildcard {
                                candidate.current_type.is_some()
                            } else {
                                candidate.current_type.is_some() && candidate.type_match
                            }
                    })
            }
            _ => {
                self.requested_type.is_none()
                    && self.logical_slots == 0
                    && self.allocated_slots == 0
                    && self.candidates.is_empty()
                    && self.returned == -1
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeQueueCountError {
    NonAsciiQuery,
    LeaderMirrorMismatch {
        owner: usize,
        type_flags: i32,
        victory_flags: i32,
        step8_flags: u32,
    },
    BuildRowOutOfRange {
        owner: usize,
        object_index: i32,
        row: usize,
        builds: usize,
    },
    BuildIdentityMismatch {
        owner: usize,
        object_index: i32,
        row: usize,
        build_owner: u8,
        build_object: i16,
    },
    MissingQueueType {
        owner: usize,
        object_index: i32,
        row: usize,
        slot: usize,
    },
    InvalidQueueType {
        owner: usize,
        object_index: i32,
        row: usize,
        slot: usize,
        current_type: i32,
    },
}

fn first_type(types: &TypeBuiltinState, query: &str) -> Result<Option<usize>, TypeQueueCountError> {
    if !query.is_ascii() {
        return Err(TypeQueueCountError::NonAsciiQuery);
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

/// Execute BHS builtin 436 as one read-only transaction over the canonical Build queue.
pub fn apply_sim_type_queue_count_transaction(
    sim: &Sim,
    types: &TypeBuiltinState,
    request: TypeQueueCountRequest,
) -> Result<TypeQueueCountReceipt, TypeQueueCountError> {
    let wildcard = request.type_name.is_empty() || request.type_name.starts_with(' ');
    if !wildcard && !request.type_name.is_ascii() {
        return Err(TypeQueueCountError::NonAsciiQuery);
    }
    let owner0 = request.who.wrapping_sub(1);
    if !(0..NUM_LEADERS as i32).contains(&owner0) {
        return Ok(TypeQueueCountReceipt::rejected(
            request,
            TypeQueueCountStatus::InvalidLeader,
            None,
            None,
            wildcard,
        ));
    }
    let owner = owner0 as usize;
    let type_flags = types.leaders[owner].leader_flags;
    let victory_flags = sim.vic_leaders.slots[owner].leader_flags;
    let step8_flags = sim.step8.leaders[owner].flags;
    if type_flags != victory_flags || type_flags as u32 != step8_flags {
        return Err(TypeQueueCountError::LeaderMirrorMismatch {
            owner,
            type_flags,
            victory_flags,
            step8_flags,
        });
    }
    if type_flags & 3 != 3 {
        return Ok(TypeQueueCountReceipt::rejected(
            request,
            TypeQueueCountStatus::InactiveLeader,
            Some(owner),
            None,
            wildcard,
        ));
    }

    if !(BUILD_BAND_BASE as i32..WALL_BAND_BASE as i32).contains(&request.build_object) {
        return Ok(TypeQueueCountReceipt::rejected(
            request,
            TypeQueueCountStatus::InvalidBuild,
            Some(owner),
            None,
            wildcard,
        ));
    }
    let build_slot = (request.build_object - BUILD_BAND_BASE as i32) as usize;
    let Some(&row) = sim
        .world
        .objects
        .slot(owner)
        .band(Band::Build)
        .get(build_slot)
    else {
        return Ok(TypeQueueCountReceipt::rejected(
            request,
            TypeQueueCountStatus::InvalidBuild,
            Some(owner),
            None,
            wildcard,
        ));
    };
    let row = row as usize;
    let Some(build) = sim.builds.get(row) else {
        return Err(TypeQueueCountError::BuildRowOutOfRange {
            owner,
            object_index: request.build_object,
            row,
            builds: sim.builds.len(),
        });
    };
    if build.who as usize != owner || i32::from(build.object_id()) != request.build_object {
        return Err(TypeQueueCountError::BuildIdentityMismatch {
            owner,
            object_index: request.build_object,
            row,
            build_owner: build.who,
            build_object: build.object_id(),
        });
    }
    if build.flags & flag::VALID == 0 {
        return Ok(TypeQueueCountReceipt::rejected(
            request,
            TypeQueueCountStatus::InvalidBuild,
            Some(owner),
            Some(row),
            wildcard,
        ));
    }

    let requested_type = if wildcard {
        None
    } else {
        let Some(requested_type) = first_type(types, &request.type_name)? else {
            return Ok(TypeQueueCountReceipt::rejected(
                request,
                TypeQueueCountStatus::InvalidType,
                Some(owner),
                Some(row),
                false,
            ));
        };
        Some(requested_type)
    };

    let logical_slots = usize::from(build.queue.queued);
    let allocated_slots = build.queue.num();
    let mut candidates = Vec::with_capacity(logical_slots);
    for slot in 0..logical_slots {
        let current_type = build.queue.type_at(slot);
        let current_type = if current_type == -1 {
            None
        } else {
            Some(current_type)
        };
        let type_match = if let Some(requested_type) = requested_type {
            let Some(current_type) = current_type else {
                return Err(TypeQueueCountError::MissingQueueType {
                    owner,
                    object_index: request.build_object,
                    row,
                    slot,
                });
            };
            let current_slot = usize::try_from(current_type).map_err(|_| {
                TypeQueueCountError::InvalidQueueType {
                    owner,
                    object_index: request.build_object,
                    row,
                    slot,
                    current_type,
                }
            })?;
            if current_slot >= NUM_TYPES {
                return Err(TypeQueueCountError::InvalidQueueType {
                    owner,
                    object_index: request.build_object,
                    row,
                    slot,
                    current_type,
                });
            }
            types
                .types
                .row(current_slot)
                .is_list
                .iter()
                .any(|&candidate| usize::from(candidate) == requested_type)
        } else {
            false
        };
        let counted = if wildcard {
            current_type.is_some()
        } else {
            type_match
        };
        candidates.push(TypeQueueCandidateReceipt {
            slot,
            current_type,
            type_match,
            counted,
        });
    }

    let returned = candidates
        .iter()
        .filter(|candidate| candidate.counted)
        .count() as i32;
    let receipt = TypeQueueCountReceipt {
        request,
        status: TypeQueueCountStatus::Counted,
        requested_owner: Some(owner),
        build_row: Some(row),
        wildcard,
        requested_type,
        logical_slots,
        allocated_slots,
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
    use crate::systems::production::{off, BuildData, BuildQueue, BuildQueueEntry};
    use crate::systems::save_load::{load_sim, save_sim};

    const CITIZEN: i32 = 50;
    const DERIVED_CITIZEN: i32 = 52;
    const UNIVERSITY: i32 = 420;

    fn types() -> TypeBuiltinState {
        let mut rows: Vec<_> = (0..NUM_TYPES).map(TypeRow::empty).collect();
        rows[CITIZEN as usize].name = "Citizen".into();
        rows[DERIVED_CITIZEN as usize].name = "Upgraded Citizen".into();
        rows[DERIVED_CITIZEN as usize].is_list.push(CITIZEN as u16);
        rows[UNIVERSITY as usize].name = "University".into();
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

    fn entry(type_index: i32) -> BuildQueueEntry {
        BuildQueueEntry {
            type_index: type_index as i16,
            ..BuildQueueEntry::default()
        }
    }

    fn fixture() -> Sim {
        let mut sim = Sim::new(436, 8);
        sim.step8.leaders[0].flags = 3;
        sim.vic_leaders.slots[0].leader_flags = 3;
        let mut build = BuildData {
            flags: flag::VALID | flag::ACTIVE,
            who: 0,
            gather_down: -1,
            city: -1,
            city_down: -1,
            wonder: -1,
            dock: -1,
            attack_ox: -1,
            attack_whom: -1,
            queue: BuildQueue {
                queued: 3,
                entries: vec![entry(DERIVED_CITIZEN), entry(CITIZEN), entry(UNIVERSITY)],
            },
            ..BuildData::default()
        };
        build.other[off::OBJECT_ID..off::OBJECT_ID + 2]
            .copy_from_slice(&(BUILD_BAND_BASE as i16).to_le_bytes());
        build.other[0x28..0x2a].copy_from_slice(&(-1i16).to_le_bytes());
        sim.spawn_build(0, build);
        sim
    }

    fn request(type_name: &str) -> TypeQueueCountRequest {
        TypeQueueCountRequest {
            who: 1,
            build_object: BUILD_BAND_BASE as i32,
            type_name: type_name.into(),
        }
    }

    #[test]
    fn counts_non_strict_queued_types_and_both_wildcard_forms() {
        let types = types();
        let sim = fixture();
        let typed =
            apply_sim_type_queue_count_transaction(&sim, &types, request("cItIzEn")).unwrap();
        let empty = apply_sim_type_queue_count_transaction(&sim, &types, request("")).unwrap();
        let leading_space =
            apply_sim_type_queue_count_transaction(&sim, &types, request(" Ω is ignored")).unwrap();
        assert_eq!(typed.returned, 2);
        assert_eq!(empty.returned, 3);
        assert_eq!(leading_space.returned, 3);
        assert!(!typed.wildcard);
        assert!(empty.wildcard && leading_space.wildcard);
        assert!(typed.validates() && empty.validates() && leading_space.validates());
    }

    #[test]
    fn transaction_is_read_only_and_rejects_invalid_inputs_with_retail_minus_one() {
        let types = types();
        let sim = fixture();
        let queue_before = sim.builds[0].queue.clone();
        for (request, status) in [
            (
                TypeQueueCountRequest {
                    who: 9,
                    ..request("Citizen")
                },
                TypeQueueCountStatus::InvalidLeader,
            ),
            (
                TypeQueueCountRequest {
                    build_object: 1999,
                    ..request("Citizen")
                },
                TypeQueueCountStatus::InvalidBuild,
            ),
            (request("Absent"), TypeQueueCountStatus::InvalidType),
        ] {
            let receipt = apply_sim_type_queue_count_transaction(&sim, &types, request).unwrap();
            assert_eq!(receipt.status, status);
            assert_eq!(receipt.returned, -1);
            assert!(receipt.validates());
        }
        assert_eq!(sim.builds[0].queue.queued, queue_before.queued);
        assert_eq!(sim.builds[0].queue.entries, queue_before.entries);
    }

    #[test]
    fn wildcard_preserves_missing_slot_sentinel_but_typed_count_fails_closed() {
        let types = types();
        let mut sim = fixture();
        sim.builds[0].queue.entries.truncate(2);
        let wildcard = apply_sim_type_queue_count_transaction(&sim, &types, request("")).unwrap();
        assert_eq!(wildcard.returned, 2);
        assert_eq!(wildcard.candidates[2].current_type, None);
        assert!(matches!(
            apply_sim_type_queue_count_transaction(&sim, &types, request("Citizen")),
            Err(TypeQueueCountError::MissingQueueType { slot: 2, .. })
        ));
    }

    #[test]
    fn build_queue_survives_save_load_and_keeps_the_same_census() {
        let types = types();
        let mut sim = fixture();
        // Activation mirrors are restored by their independent owners after load.
        sim.step8.leaders[0].flags = 0;
        sim.vic_leaders.slots[0].leader_flags = 0;
        let bytes = save_sim(&sim).unwrap();
        let mut loaded = load_sim(&bytes).unwrap();
        loaded.step8.leaders[0].flags = 3;
        loaded.vic_leaders.slots[0].leader_flags = 3;
        let receipt =
            apply_sim_type_queue_count_transaction(&loaded, &types, request("Citizen")).unwrap();
        assert_eq!(receipt.returned, 2);
        assert_eq!(receipt.logical_slots, 3);
        assert_eq!(receipt.allocated_slots, 3);
        assert_eq!(loaded.builds[0].queue.entries[0].type_index, 52);
    }
}
