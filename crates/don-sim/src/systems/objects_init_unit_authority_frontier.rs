// SPDX-License-Identifier: GPL-3.0-or-later
//! Source-only transaction model for the all-`-1` `Objects::init_unit` path used by
//! BHS create-unit registrations 508--510.
//!
//! This is deliberately a strict receipt validator rather than a `World` adapter. Retail
//! owns a sparse, reusable per-owner Unit band, runs the complete `Unit::init` vcall, corrects
//! Leader accounting for subordinate squad members, performs another nearby-placement call,
//! and commits links in a non-atomic order. `World::allocate_typed_at` does none of that.

pub const OBJECTS_INIT_UNIT_VA: u32 = 0x0065_e0c0;
pub const OBJECTS_INIT_UNIT_BYTES: u32 = 1_603;
pub const OBJECTS_FIND_FREE_VA: u32 = 0x0065_ad60;
pub const OBJECTS_FIND_FREE_BYTES: u32 = 1_101;
pub const OBJECTS_FIND_FREE_CALL_VA: u32 = 0x0065_e137;
pub const UNIT_INIT_VA: u32 = 0x0061_2100;
pub const UNIT_INIT_BYTES: u32 = 3_732;
pub const UNIT_INIT_VTABLE_OFFSET: u32 = 0x8c;
pub const LEADER_TRACK_UNIT_TYPE_VA: u32 = 0x006e_0dd0;
pub const LEADER_TRACK_UNIT_TYPE_BYTES: u32 = 326;
pub const UNIT_FIND_NEARBY_SPOT_VA: u32 = 0x0061_de70;
pub const UNIT_FIND_NEARBY_SPOT_BYTES: u32 = 1_433;
pub const UNIT_SET_NEW_LOCATION_VA: u32 = 0x005f_8d20;
pub const UNIT_SET_NEW_LOCATION_BYTES: u32 = 1_757;
pub const UNIT_GET_CAPTAIN_VA: u32 = 0x0061_0ab0;

pub const OWNER_SLOTS: i32 = 10;
pub const PLAYABLE_OWNER_SLOTS: i32 = 8;
pub const UNIT_BAND_START: i32 = 0;
pub const UNIT_BAND_LIMIT: i32 = 2_000;
pub const UNIT_MARK_OFFSET: u32 = 0x15c;
pub const UNIT_PREVIOUS_OFFSET: u32 = 0x8e;
pub const UNIT_NEXT_OFFSET: u32 = 0x90;
pub const UNIT_MASKS_OFFSET: u32 = 0x68;
pub const OBJECT_ANGLE_OFFSET: u32 = 0x50;
pub const OBJECT_TYPE_NEW_BLOCK_RADIUS_OFFSET: u32 = 0x248;
pub const UNIT_TYPE_CONTROL_COST_OFFSET: u32 = 0x2f0;
pub const UNIT_TYPE_UBER_SIZE_OFFSET: u32 = 0x308;
pub const FIGHTER_BOMBER_TYPE: i32 = 0x134;
pub const GOVERNMENT_HERO_FLAG: u32 = 0x0400_0000;
pub const LEADER_SPECIAL_TYPE_PRESENT_FLAG: u32 = 0x0002_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BhsInitUnitRequest {
    pub owner: i32,
    pub type_index: i32,
    pub x: i32,
    pub y: i32,
    pub exact_o: i32,
    pub external_previous: i32,
    pub external_next: i32,
}

impl BhsInitUnitRequest {
    pub const fn is_bhs_shape(self) -> bool {
        self.owner >= 0
            && self.owner < OWNER_SLOTS
            && self.exact_o == -1
            && self.external_previous == -1
            && self.external_next == -1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitTypeAuthorityFacts {
    pub uber_size: i32,
    pub control_cost: i32,
    pub is_fighter_bomber: bool,
    /// Result of the virtual `UnitData::is_gov_hero`; the concrete fast path is the flag below.
    pub is_government_hero: bool,
    pub track: TrackUnitTypeFacts,
}

impl UnitTypeAuthorityFacts {
    pub const fn tracks_linked_member(self) -> bool {
        self.control_cost != 0 || self.is_fighter_bomber || self.is_government_hero
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrainingWhere {
    Barracks,
    Stable,
    Factory,
    Dock,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrackUnitTypeFacts {
    pub has_attack: bool,
    pub training_where: TrainingWhere,
    /// Retail uses domain `2` for the fallback air counter.
    pub domain: i32,
    pub is_peasant: bool,
    pub is_scholar: bool,
    pub role_has_scout_bit: bool,
    pub is_type_0x42: bool,
    pub is_type_0x4b: bool,
    /// Read through the allocated member only on the `is_type_0x42` path.
    pub member_former_type_is_0x34: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct LeaderAccounting {
    pub flags: u32,
    pub num_units_for_type: u16,
    pub units_built: i32,
    pub active: i32,
    pub control: i32,
    pub peasants: i32,
    pub scholars: i32,
    pub scouts: i32,
    pub scholar_militia: i32,
    pub barracks_units: i32,
    pub stable_units: i32,
    pub factory_units: i32,
    pub combat_units: i32,
    pub dock_units: i32,
    pub air_units: i32,
}

impl LeaderAccounting {
    /// Apply `Leader::track_unit_type(type, -1, member)` and the three direct corrections at
    /// `0x0065E251..0x0065E25D`. Every arithmetic operation deliberately wraps like x86.
    pub fn after_linked_member(
        mut self,
        type_facts: UnitTypeAuthorityFacts,
        unit_masks: u32,
    ) -> Self {
        self.num_units_for_type = self.num_units_for_type.wrapping_sub(1);

        if type_facts.track.has_attack {
            match type_facts.track.training_where {
                TrainingWhere::Barracks => {
                    self.barracks_units = self.barracks_units.wrapping_sub(1);
                    self.combat_units = self.combat_units.wrapping_sub(1);
                }
                TrainingWhere::Stable => {
                    self.stable_units = self.stable_units.wrapping_sub(1);
                    self.combat_units = self.combat_units.wrapping_sub(1);
                }
                TrainingWhere::Factory => {
                    self.factory_units = self.factory_units.wrapping_sub(1);
                }
                TrainingWhere::Dock => {
                    self.dock_units = self.dock_units.wrapping_sub(1);
                }
                TrainingWhere::Other if type_facts.track.domain == 2 => {
                    self.air_units = self.air_units.wrapping_sub(1);
                }
                TrainingWhere::Other => {}
            }
        }

        if type_facts.track.is_peasant {
            self.peasants = self.peasants.wrapping_sub(1);
        } else if type_facts.track.is_scholar {
            self.scholars = self.scholars.wrapping_sub(1);
        } else if type_facts.track.role_has_scout_bit {
            self.scouts = self.scouts.wrapping_sub(1);
        }

        if type_facts.track.is_type_0x42 {
            if type_facts.track.member_former_type_is_0x34 {
                self.scholar_militia = self.scholar_militia.wrapping_sub(1);
            }
        } else if type_facts.track.is_type_0x4b {
            if self.num_units_for_type == 0 {
                self.flags &= !LEADER_SPECIAL_TYPE_PRESENT_FLAG;
            } else {
                self.flags |= LEADER_SPECIAL_TYPE_PRESENT_FLAG;
            }
        }

        if unit_masks & 1 == 0 {
            self.control = self.control.wrapping_sub(type_facts.control_cost);
        }
        self.active = self.active.wrapping_sub(1);
        self.units_built = self.units_built.wrapping_sub(1);
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReuseCandidateFacts {
    pub index: i32,
    pub flags: u8,
    pub hold_frames: u16,
    pub is_unit: bool,
    pub o_up: i16,
}

impl ReuseCandidateFacts {
    pub const fn is_reusable(self) -> bool {
        self.flags & 1 == 0 && self.hold_frames == 0 && (!self.is_unit || self.o_up < 0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitBandStorageClass {
    Unit,
    Animal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindFreeDisposition {
    Reused {
        candidate: ReuseCandidateFacts,
        all_lower_indices_ineligible: bool,
    },
    /// The sparse object slot already existed above a previously lowered high-water mark.
    ExtendedExistingStorage,
    /// Retail constructed an object, stored it in `Objects::lists`, made the `Units` row valid,
    /// and stored the object's Unit projection in the parallel `Units::lists` band.
    ConstructedAndRegistered {
        class: UnitBandStorageClass,
        object_list_registered: bool,
        unit_projection_registered: bool,
    },
    CapacityFailure {
        all_existing_indices_ineligible: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindFreeUnitReceipt {
    pub ordinal: u32,
    pub owner: i32,
    pub start: i32,
    pub limit: i32,
    pub exact_o: i32,
    pub cursor_before: i32,
    pub cursor_after: i32,
    pub returned: i32,
    pub disposition: FindFreeDisposition,
}

impl FindFreeUnitReceipt {
    pub fn validate_bhs(self, ordinal: u32, owner: i32, cursor: i32) -> bool {
        if self.ordinal != ordinal
            || self.owner != owner
            || self.start != UNIT_BAND_START
            || self.limit != UNIT_BAND_LIMIT
            || self.exact_o != -1
            || self.cursor_before != cursor
        {
            return false;
        }

        match self.disposition {
            FindFreeDisposition::Reused {
                candidate,
                all_lower_indices_ineligible,
            } => {
                all_lower_indices_ineligible
                    && candidate.index == self.returned
                    && candidate.index >= UNIT_BAND_START
                    && candidate.index < cursor
                    && candidate.is_reusable()
                    && self.cursor_after == cursor
            }
            FindFreeDisposition::ExtendedExistingStorage => {
                cursor < UNIT_BAND_LIMIT
                    && self.returned == cursor
                    && self.cursor_after == cursor.wrapping_add(1)
            }
            FindFreeDisposition::ConstructedAndRegistered {
                class,
                object_list_registered,
                unit_projection_registered,
            } => {
                let expected_class = if owner < PLAYABLE_OWNER_SLOTS {
                    UnitBandStorageClass::Unit
                } else {
                    UnitBandStorageClass::Animal
                };
                cursor < UNIT_BAND_LIMIT
                    && self.returned == cursor
                    && self.cursor_after == cursor.wrapping_add(1)
                    && class == expected_class
                    && object_list_registered
                    && unit_projection_registered
            }
            FindFreeDisposition::CapacityFailure {
                all_existing_indices_ineligible,
            } => {
                cursor >= UNIT_BAND_LIMIT
                    && all_existing_indices_ineligible
                    && self.returned == -1
                    && self.cursor_after == cursor
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompleteBody {
    UnitInit3732Bytes,
    LeaderTrackUnitType326Bytes,
    FindNearbySpot1433Bytes,
    SetNewLocation1757Bytes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitAfterInit {
    pub owner: i32,
    pub o: i32,
    pub type_index: i32,
    pub x: i32,
    pub y: i32,
    pub angle: i32,
    pub unit_masks: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitInitReceipt {
    pub ordinal: u32,
    pub owner: i32,
    pub type_index: i32,
    pub o: i32,
    pub x: i32,
    pub y: i32,
    /// `Objects::init_unit` discards this return even when it is negative.
    pub returned: i32,
    pub extent: CompleteBody,
    pub after: UnitAfterInit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderCorrectionReceipt {
    pub ordinal: u32,
    pub owner: i32,
    pub type_index: i32,
    pub member_o: i32,
    pub extent: CompleteBody,
    pub before: LeaderAccounting,
    pub after: LeaderAccounting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptainFacts {
    pub owner: i32,
    pub o: i32,
    pub x: i32,
    pub y: i32,
    pub angle: i32,
    /// Read from the captain's current type at `ObjectTypeData +0x248`.
    pub new_block_radius: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolveCaptainReceipt {
    pub ordinal: u32,
    pub from_owner: i32,
    pub from_o: i32,
    pub returned: i32,
    pub captain: CaptainFacts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InternalNearbyRequest {
    pub ordinal: u32,
    /// The receiver remains the originally requested UnitType.
    pub receiver_type: i32,
    pub origin_x: i32,
    pub origin_y: i32,
    pub min_radius: i32,
    pub max_radius: i32,
    pub first_filter: i32,
    pub angle: i32,
    pub filter: i32,
    pub member_o: i32,
    pub owner: i32,
    pub tail: [i32; 5],
}

impl InternalNearbyRequest {
    pub fn from_captain(
        ordinal: u32,
        receiver_type: i32,
        member_o: i32,
        captain: CaptainFacts,
    ) -> Self {
        Self {
            ordinal,
            receiver_type,
            origin_x: captain.x,
            origin_y: captain.y,
            min_radius: captain.new_block_radius.wrapping_mul(0x30),
            max_radius: captain
                .new_block_radius
                .wrapping_mul(0x60)
                .wrapping_add(0xc0),
            first_filter: -1,
            angle: captain.angle,
            filter: 3,
            member_o,
            owner: captain.owner,
            tail: [0, 0, -1, 0, -1],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InternalNearbyReceipt {
    pub request: InternalNearbyRequest,
    pub returned: i32,
    pub output_x: i32,
    pub output_y: i32,
    pub extent: CompleteBody,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetNewLocationReceipt {
    pub ordinal: u32,
    pub member_o: i32,
    pub x: i32,
    pub y: i32,
    pub tail: [i32; 2],
    /// The caller discards this return.
    pub returned: i32,
    pub extent: CompleteBody,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InitUnitStep {
    FindFree(FindFreeUnitReceipt),
    UnitInit(UnitInitReceipt),
    SetPrevious {
        ordinal: u32,
        member_o: i32,
        previous_o: i32,
    },
    LeaderCorrection(LeaderCorrectionReceipt),
    ResolveCaptain(ResolveCaptainReceipt),
    Nearby(InternalNearbyReceipt),
    SetNewLocation(SetNewLocationReceipt),
    SetNext {
        ordinal: u32,
        previous_o: i32,
        member_o: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetailedInitUnitReceipt {
    pub request: BhsInitUnitRequest,
    pub type_facts: UnitTypeAuthorityFacts,
    pub steps: Vec<InitUnitStep>,
    pub returned: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedInitUnitEffects {
    pub initialized_members: Vec<i32>,
    pub terminal_find_free_failure: Option<i32>,
    pub returned_captain_or_failure: i32,
    pub unit_mark_before: i32,
    pub unit_mark_after: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitUnitReceiptError {
    NonBhsRequest,
    NonPositiveUberSize,
    MissingStep,
    WrongStep,
    InvalidFindFree,
    InvalidUnitInit,
    InvalidLink,
    InvalidLeaderCorrection,
    InvalidCaptain,
    InvalidNearby,
    InvalidLocation,
    InvalidReturn,
    TrailingSteps,
}

impl DetailedInitUnitReceipt {
    pub fn validate(&self) -> Result<ValidatedInitUnitEffects, InitUnitReceiptError> {
        if !self.request.is_bhs_shape() {
            return Err(InitUnitReceiptError::NonBhsRequest);
        }
        if self.type_facts.uber_size <= 0 {
            return Err(InitUnitReceiptError::NonPositiveUberSize);
        }

        let mut at = 0usize;
        let mut cursor = match self.steps.first() {
            Some(InitUnitStep::FindFree(find)) => find.cursor_before,
            Some(_) => return Err(InitUnitReceiptError::WrongStep),
            None => return Err(InitUnitReceiptError::MissingStep),
        };
        let unit_mark_before = cursor;
        let mut previous = -1;
        let mut initialized_members = Vec::with_capacity(self.type_facts.uber_size as usize);

        for ordinal in 0..self.type_facts.uber_size as u32 {
            let find = take_find(&self.steps, &mut at)?;
            if !find.validate_bhs(ordinal, self.request.owner, cursor) {
                return Err(InitUnitReceiptError::InvalidFindFree);
            }
            cursor = find.cursor_after;
            if find.returned < 0 {
                if at != self.steps.len() {
                    return Err(InitUnitReceiptError::TrailingSteps);
                }
                if self.returned != -1 {
                    return Err(InitUnitReceiptError::InvalidReturn);
                }
                return Ok(ValidatedInitUnitEffects {
                    initialized_members,
                    terminal_find_free_failure: Some(-1),
                    returned_captain_or_failure: -1,
                    unit_mark_before,
                    unit_mark_after: cursor,
                });
            }

            let member_o = find.returned;
            let init = take_unit_init(&self.steps, &mut at)?;
            let expected_after = UnitAfterInit {
                owner: self.request.owner,
                o: member_o,
                type_index: self.request.type_index,
                x: self.request.x,
                y: self.request.y,
                angle: init.after.angle,
                unit_masks: init.after.unit_masks,
            };
            if init.ordinal != ordinal
                || init.owner != self.request.owner
                || init.type_index != self.request.type_index
                || init.o != member_o
                || init.x != self.request.x
                || init.y != self.request.y
                || init.extent != CompleteBody::UnitInit3732Bytes
                || init.after != expected_after
            {
                return Err(InitUnitReceiptError::InvalidUnitInit);
            }
            // Load-bearing: `init.returned` is not inspected.

            match self.steps.get(at) {
                Some(InitUnitStep::SetPrevious {
                    ordinal: link_ordinal,
                    member_o: link_member,
                    previous_o,
                }) if *link_ordinal == ordinal
                    && *link_member == member_o
                    && *previous_o == previous => {}
                Some(_) => return Err(InitUnitReceiptError::InvalidLink),
                None => return Err(InitUnitReceiptError::MissingStep),
            }
            at += 1;

            if previous >= 0 {
                if self.type_facts.tracks_linked_member() {
                    let correction = take_leader(&self.steps, &mut at)?;
                    if correction.ordinal != ordinal
                        || correction.owner != self.request.owner
                        || correction.type_index != self.request.type_index
                        || correction.member_o != member_o
                        || correction.extent != CompleteBody::LeaderTrackUnitType326Bytes
                        || correction.after
                            != correction
                                .before
                                .after_linked_member(self.type_facts, init.after.unit_masks)
                    {
                        return Err(InitUnitReceiptError::InvalidLeaderCorrection);
                    }
                }

                let captain = take_captain(&self.steps, &mut at)?;
                if !valid_captain(captain, ordinal, self.request.owner, member_o) {
                    return Err(InitUnitReceiptError::InvalidCaptain);
                }
                let nearby = take_nearby(&self.steps, &mut at)?;
                let expected_nearby = InternalNearbyRequest::from_captain(
                    ordinal,
                    self.request.type_index,
                    member_o,
                    captain.captain,
                );
                if nearby.request != expected_nearby
                    || nearby.extent != CompleteBody::FindNearbySpot1433Bytes
                {
                    return Err(InitUnitReceiptError::InvalidNearby);
                }
                let (location_x, location_y) = if nearby.returned == 0 {
                    (nearby.output_x, nearby.output_y)
                } else {
                    (init.after.x, init.after.y)
                };
                let location = take_location(&self.steps, &mut at)?;
                if location.ordinal != ordinal
                    || location.member_o != member_o
                    || location.x != location_x
                    || location.y != location_y
                    || location.tail != [1, 1]
                    || location.extent != CompleteBody::SetNewLocation1757Bytes
                {
                    return Err(InitUnitReceiptError::InvalidLocation);
                }
                // Load-bearing: `location.returned` is also discarded.

                match self.steps.get(at) {
                    Some(InitUnitStep::SetNext {
                        ordinal: link_ordinal,
                        previous_o,
                        member_o: link_member,
                    }) if *link_ordinal == ordinal
                        && *previous_o == previous
                        && *link_member == member_o => {}
                    Some(_) => return Err(InitUnitReceiptError::InvalidLink),
                    None => return Err(InitUnitReceiptError::MissingStep),
                }
                at += 1;
            }

            initialized_members.push(member_o);
            previous = member_o;
        }

        let final_captain = take_captain(&self.steps, &mut at)?;
        if !valid_captain(
            final_captain,
            self.type_facts.uber_size as u32,
            self.request.owner,
            previous,
        ) {
            return Err(InitUnitReceiptError::InvalidCaptain);
        }
        if at != self.steps.len() {
            return Err(InitUnitReceiptError::TrailingSteps);
        }
        if self.returned != final_captain.returned {
            return Err(InitUnitReceiptError::InvalidReturn);
        }

        Ok(ValidatedInitUnitEffects {
            initialized_members,
            terminal_find_free_failure: None,
            returned_captain_or_failure: self.returned,
            unit_mark_before,
            unit_mark_after: cursor,
        })
    }
}

fn take_find(
    steps: &[InitUnitStep],
    at: &mut usize,
) -> Result<FindFreeUnitReceipt, InitUnitReceiptError> {
    take(steps, at, |step| match step {
        InitUnitStep::FindFree(value) => Some(*value),
        _ => None,
    })
}

fn take_unit_init(
    steps: &[InitUnitStep],
    at: &mut usize,
) -> Result<UnitInitReceipt, InitUnitReceiptError> {
    take(steps, at, |step| match step {
        InitUnitStep::UnitInit(value) => Some(*value),
        _ => None,
    })
}

fn take_leader(
    steps: &[InitUnitStep],
    at: &mut usize,
) -> Result<LeaderCorrectionReceipt, InitUnitReceiptError> {
    take(steps, at, |step| match step {
        InitUnitStep::LeaderCorrection(value) => Some(*value),
        _ => None,
    })
}

fn take_captain(
    steps: &[InitUnitStep],
    at: &mut usize,
) -> Result<ResolveCaptainReceipt, InitUnitReceiptError> {
    take(steps, at, |step| match step {
        InitUnitStep::ResolveCaptain(value) => Some(*value),
        _ => None,
    })
}

fn take_nearby(
    steps: &[InitUnitStep],
    at: &mut usize,
) -> Result<InternalNearbyReceipt, InitUnitReceiptError> {
    take(steps, at, |step| match step {
        InitUnitStep::Nearby(value) => Some(*value),
        _ => None,
    })
}

fn take_location(
    steps: &[InitUnitStep],
    at: &mut usize,
) -> Result<SetNewLocationReceipt, InitUnitReceiptError> {
    take(steps, at, |step| match step {
        InitUnitStep::SetNewLocation(value) => Some(*value),
        _ => None,
    })
}

fn take<T: Copy>(
    steps: &[InitUnitStep],
    at: &mut usize,
    project: impl FnOnce(&InitUnitStep) -> Option<T>,
) -> Result<T, InitUnitReceiptError> {
    let step = steps.get(*at).ok_or(InitUnitReceiptError::MissingStep)?;
    let value = project(step).ok_or(InitUnitReceiptError::WrongStep)?;
    *at += 1;
    Ok(value)
}

fn valid_captain(receipt: ResolveCaptainReceipt, ordinal: u32, owner: i32, from_o: i32) -> bool {
    receipt.ordinal == ordinal
        && receipt.from_owner == owner
        && receipt.from_o == from_o
        && receipt.returned >= 0
        && receipt.returned == receipt.captain.o
        && receipt.captain.owner == owner
}
