//! Source-backed `Setup::build_units` call schedule and receipt boundary.
//!
//! This module owns the deterministic outer schedule at `0x005aafc0`: starting-citizen
//! counts, tribe/rules branches, type-resolution order, and every admitted call to
//! `Setup::place_unit`. It deliberately does not pretend that `place_unit`,
//! `Objects::init_unit`, or `Guy::init_real` is already a canonical replay producer.
//! Those callees remain represented by exact RNG/identity/container receipts.

use std::collections::BTreeSet;
use std::fmt;

use don_sim::rng::Random;

pub const SETUP_BUILD_UNITS_VA: u32 = 0x005a_afc0;
pub const SETUP_BUILD_UNITS_BYTES: u32 = 1_952;
pub const SETUP_GET_STARTING_CITIZENS_VA: u32 = 0x005a_af50;
pub const SETUP_PLACE_UNIT_VA: u32 = 0x005a_bca0;
pub const SETUP_PLACE_UNIT_BYTES: u32 = 749;
pub const PLACE_UNIT_DIRECT_RANDOM_CALL_VA: u32 = 0x005a_bd76;
pub const OBJECTS_INIT_UNIT_VA: u32 = 0x0065_e0c0;
pub const OBJECTS_INIT_UNIT_BYTES: u32 = 1_603;

pub const BASE_PEASANT_TYPE: i32 = 50;
pub const BASE_SCHOLAR_TYPE: i32 = 52;
pub const BASE_SCOUT_TYPE: i32 = 69;
pub const DUTCH_MERCHANT_TYPE: i32 = 62;

pub const TRIBE_BONUS_SCHOLARS: u8 = 5;
pub const TRIBE_BONUS_EXTRA_SCOUTS: u8 = 9;
pub const TRIBE_BONUS_EXTRA_CITIZENS: u8 = 16;
pub const TRIBE_BONUS_FEWER_CITIZENS: u8 = 19;
pub const TRIBE_BONUS_STARTING_TOWN_CITIZENS: u8 = 20;
pub const TRIBE_BONUS_DUTCH_MERCHANTS: u8 = 22;

pub const SCOUT_BASE_CALL_VA: u32 = 0x005a_b116;
pub const SCOUT_RULE_CALL_VA: u32 = 0x005a_b156;
pub const SCOUT_REVEAL_MAP_CALL_VA: u32 = 0x005a_b19f;
pub const DUTCH_MERCHANT_CALL_VAS: [u32; 2] = [0x005a_b1c8, 0x005a_b1e3];
pub const SCHOLAR_BUILDING_SEARCH_VA: u32 = 0x005a_b24d;
pub const CITIZEN_SIMPLE_CALL_VA: u32 = 0x005a_b3c2;
pub const CITIZEN_EXISTING_BUILDING_BRANCH_VA: u32 = 0x005a_b3cc;

pub const UNIT_BAND_LIMIT: i32 = 2_000;
pub const MAX_PLANNED_CALLS: i32 = 4_096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartingCitizenCounts {
    pub total: i32,
    /// For `starting_town` modes 2 and 3, the first this-many citizen iterations enter
    /// the fixed object-2001 reuse/gather arm before the later building-selection scan.
    pub fixed_building_prefix: i32,
}

pub fn starting_citizen_counts(starting_town: i32) -> Option<StartingCitizenCounts> {
    Some(match starting_town {
        0 => StartingCitizenCounts {
            total: 3,
            fixed_building_prefix: 0,
        },
        1 => StartingCitizenCounts {
            total: 4,
            fixed_building_prefix: 0,
        },
        2 => StartingCitizenCounts {
            total: 5,
            fixed_building_prefix: 2,
        },
        3 => StartingCitizenCounts {
            total: 10,
            fixed_building_prefix: 5,
        },
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StartingUnitBonuses {
    pub scholars: bool,
    pub extra_scouts: bool,
    pub extra_citizens: bool,
    pub fewer_citizens: bool,
    pub starting_town_citizens: bool,
    pub dutch_merchants: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StartingUnitRuleFacts {
    /// `Rules+0x680`, looped only while positive and only with tribe bonus 9.
    pub bonus_scouts: i32,
    /// `Rules+0x5e4`. Any nonzero value reaches the scholar building search.
    pub bonus_scholars: i32,
    /// `Rules+0x7b0`, added with wrapping i32 arithmetic under tribe bonus 16.
    pub bonus_citizens: i32,
    /// `Rules+0x878`; in `starting_town` mode 2, bonus 20 adds `value - 3` only
    /// when value is above 3.
    pub starting_town_citizens: i32,
}

/// One base/nation/current-upgrade chain.
///
/// `build_units` first selects `base` or `nation_variant` through `tribe_can_type`, then
/// calls `LeaderData::current_upgrade`. `Setup::place_unit` immediately calls
/// `current_upgrade` a second time. Both results are retained because collapsing them loses
/// the exact type lookup chronology.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeResolutionFacts {
    pub base: i32,
    pub tribe_can_base: bool,
    pub nation_variant: i32,
    pub build_units_upgrade: i32,
    pub place_unit_upgrade: i32,
    pub uber_size: i32,
    pub squad_size: i32,
    pub crew_size: i32,
}

impl TypeResolutionFacts {
    pub const fn selected_before_upgrade(self) -> i32 {
        if self.tribe_can_base {
            self.base
        } else {
            self.nation_variant
        }
    }

    fn validate(self, expected_base: i32) -> bool {
        let total_guys = self.squad_size.checked_add(self.crew_size);
        self.base == expected_base
            && self.selected_before_upgrade() >= 0
            && self.build_units_upgrade >= 0
            && self.place_unit_upgrade >= 0
            && self.uber_size > 0
            && self.squad_size >= 0
            && self.squad_size <= i8::MAX as i32
            && self.crew_size >= 0
            && matches!(total_guys, Some(0..=128))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartingUnitTypeFacts {
    pub scout: TypeResolutionFacts,
    pub citizen: TypeResolutionFacts,
    /// Type 62 is already the nation-specific Dutch merchant selected directly at the two
    /// `push 0x3e` sites; `tribe_can_base` must therefore be true.
    pub dutch_merchant: TypeResolutionFacts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildUnitsInputs {
    pub owner: i32,
    /// Index into World start arrays; retained even though `build_units` immediately reads
    /// the corresponding tile pair before issuing any Unit call.
    pub start_index: i32,
    /// Center Build object returned by `Setup::build_cities`.
    pub center_city_o: i32,
    pub start_tile_x: i32,
    pub start_tile_y: i32,
    /// `Game+0x2c` / `GameInfo+0x20`, named `starting_town` by the PDB.
    pub starting_town: i32,
    /// `Game+0x2d` / `GameInfo+0x21`; values 7 and 8 add 8 and 12 citizens respectively.
    pub starting_resources: u8,
    /// `Game+0x30` / `GameInfo+0x24`; values above one add one bonus-9 Scout.
    pub reveal_map: u8,
    pub bonuses: StartingUnitBonuses,
    pub rules: StartingUnitRuleFacts,
    pub types: StartingUnitTypeFacts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartingUnitPhase {
    BaseScout,
    BonusScout { index: i32 },
    BonusScoutRevealMap,
    DutchMerchant { index: i32 },
    Citizen { index: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaceUnitCall {
    pub ordinal: u32,
    pub call_va: u32,
    pub phase: StartingUnitPhase,
    pub owner: i32,
    pub center_city_o: i32,
    /// The grid start is converted with exact wrapping `tile * 0x300 + 0x180` arithmetic.
    /// `place_unit` replaces these with center-Build coordinates when `center_city_o >= 0`.
    pub requested_x: i32,
    pub requested_y: i32,
    pub base_type: i32,
    pub selected_before_upgrade: i32,
    pub build_units_upgrade: i32,
    pub place_unit_upgrade: i32,
    pub uber_size: i32,
    pub squad_size: i32,
    pub crew_size: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildUnitsStop {
    /// `find_building(..., TypeIndex 0x1a4)` plus `go_inside` is not yet owned.
    ScholarBuildingAndContainment {
        boundary_va: u32,
        scholar_count: i32,
    },
    /// `starting_town` modes 2/3 select existing Builds, query virtual type
    /// predicates/gatherer capacity, spawn at those positions, and install gather orders.
    ExistingBuildingCitizenSelection {
        boundary_va: u32,
        fixed_building_prefix: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildUnitsPlan {
    pub calls: Vec<PlaceUnitCall>,
    pub starting_citizens: StartingCitizenCounts,
    pub citizens_after_modifiers: i32,
    pub stop: Option<BuildUnitsStop>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildUnitsPlanError {
    OwnerOutOfRange { owner: i32 },
    CenterCityOutsideBuildBand { center_city_o: i32 },
    StartIndexNegative { start_index: i32 },
    UnsupportedStartingTown { starting_town: i32 },
    InvalidTypeFacts { type_name: &'static str },
    PlannedCallLimit { calls: i32, limit: i32 },
}

impl fmt::Display for BuildUnitsPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Setup::build_units plan refused: {self:?}")
    }
}

impl std::error::Error for BuildUnitsPlanError {}

fn call_count_guard(current: usize, additional: i32) -> Result<(), BuildUnitsPlanError> {
    let current = i32::try_from(current).unwrap_or(i32::MAX);
    let calls = current.saturating_add(additional.max(0));
    if calls > MAX_PLANNED_CALLS {
        return Err(BuildUnitsPlanError::PlannedCallLimit {
            calls,
            limit: MAX_PLANNED_CALLS,
        });
    }
    Ok(())
}

fn append_call(
    calls: &mut Vec<PlaceUnitCall>,
    inputs: BuildUnitsInputs,
    phase: StartingUnitPhase,
    call_va: u32,
    facts: TypeResolutionFacts,
) {
    let ordinal = calls.len() as u32;
    calls.push(PlaceUnitCall {
        ordinal,
        call_va,
        phase,
        owner: inputs.owner,
        center_city_o: inputs.center_city_o,
        requested_x: inputs.start_tile_x.wrapping_mul(0x300).wrapping_add(0x180),
        requested_y: inputs.start_tile_y.wrapping_mul(0x300).wrapping_add(0x180),
        base_type: facts.base,
        selected_before_upgrade: facts.selected_before_upgrade(),
        build_units_upgrade: facts.build_units_upgrade,
        place_unit_upgrade: facts.place_unit_upgrade,
        uber_size: facts.uber_size,
        squad_size: facts.squad_size,
        crew_size: facts.crew_size,
    });
}

/// Recover the exact outer call schedule up to the first still-unowned producer.
///
/// This is a planner, so returning a stopped prefix performs no partial mutation. A future
/// host may execute `calls` only if it also owns the reported `stop` or deliberately uses
/// the prefix as an offline evidence artifact.
pub fn build_units_plan(inputs: BuildUnitsInputs) -> Result<BuildUnitsPlan, BuildUnitsPlanError> {
    if !(0..8).contains(&inputs.owner) {
        return Err(BuildUnitsPlanError::OwnerOutOfRange {
            owner: inputs.owner,
        });
    }
    if !(2_000..3_000).contains(&inputs.center_city_o) {
        return Err(BuildUnitsPlanError::CenterCityOutsideBuildBand {
            center_city_o: inputs.center_city_o,
        });
    }
    if inputs.start_index < 0 {
        return Err(BuildUnitsPlanError::StartIndexNegative {
            start_index: inputs.start_index,
        });
    }
    let starting_citizens = starting_citizen_counts(inputs.starting_town).ok_or(
        BuildUnitsPlanError::UnsupportedStartingTown {
            starting_town: inputs.starting_town,
        },
    )?;
    for (name, facts, base) in [
        ("scout", inputs.types.scout, BASE_SCOUT_TYPE),
        ("citizen", inputs.types.citizen, BASE_PEASANT_TYPE),
        (
            "dutch_merchant",
            inputs.types.dutch_merchant,
            DUTCH_MERCHANT_TYPE,
        ),
    ] {
        if !facts.validate(base) || (name == "dutch_merchant" && !facts.tribe_can_base) {
            return Err(BuildUnitsPlanError::InvalidTypeFacts { type_name: name });
        }
    }

    let mut citizens = starting_citizens.total;
    if inputs.starting_resources == 7 {
        citizens = citizens.wrapping_add(8);
    }
    if inputs.starting_resources == 8 {
        citizens = citizens.wrapping_add(12);
    }
    if inputs.bonuses.starting_town_citizens
        && inputs.starting_town == 2
        && inputs.rules.starting_town_citizens > 3
    {
        citizens = citizens.wrapping_add(inputs.rules.starting_town_citizens.wrapping_sub(3));
    }
    if inputs.bonuses.fewer_citizens {
        if inputs.starting_town == 2 {
            citizens = citizens.wrapping_sub(3);
        } else if inputs.starting_town == 3 {
            citizens = citizens.wrapping_sub(5);
        }
    }

    let mut calls = Vec::new();
    if inputs.starting_town != 0 {
        call_count_guard(calls.len(), 1)?;
        append_call(
            &mut calls,
            inputs,
            StartingUnitPhase::BaseScout,
            SCOUT_BASE_CALL_VA,
            inputs.types.scout,
        );

        if inputs.bonuses.extra_scouts && inputs.rules.bonus_scouts > 0 {
            call_count_guard(calls.len(), inputs.rules.bonus_scouts)?;
            for index in 0..inputs.rules.bonus_scouts {
                append_call(
                    &mut calls,
                    inputs,
                    StartingUnitPhase::BonusScout { index },
                    SCOUT_RULE_CALL_VA,
                    inputs.types.scout,
                );
            }
        }
        if inputs.bonuses.extra_scouts && inputs.reveal_map > 1 {
            call_count_guard(calls.len(), 1)?;
            append_call(
                &mut calls,
                inputs,
                StartingUnitPhase::BonusScoutRevealMap,
                SCOUT_REVEAL_MAP_CALL_VA,
                inputs.types.scout,
            );
        }
        if inputs.bonuses.dutch_merchants {
            call_count_guard(calls.len(), 2)?;
            for (index, call_va) in DUTCH_MERCHANT_CALL_VAS.into_iter().enumerate() {
                append_call(
                    &mut calls,
                    inputs,
                    StartingUnitPhase::DutchMerchant {
                        index: index as i32,
                    },
                    call_va,
                    inputs.types.dutch_merchant,
                );
            }
        }
    }

    if inputs.bonuses.scholars && inputs.rules.bonus_scholars != 0 {
        return Ok(BuildUnitsPlan {
            calls,
            starting_citizens,
            citizens_after_modifiers: citizens,
            stop: Some(BuildUnitsStop::ScholarBuildingAndContainment {
                boundary_va: SCHOLAR_BUILDING_SEARCH_VA,
                scholar_count: inputs.rules.bonus_scholars,
            }),
        });
    }

    if inputs.bonuses.extra_citizens {
        citizens = citizens.wrapping_add(inputs.rules.bonus_citizens);
    }

    if inputs.starting_town >= 2 {
        return Ok(BuildUnitsPlan {
            calls,
            starting_citizens,
            citizens_after_modifiers: citizens,
            stop: Some(BuildUnitsStop::ExistingBuildingCitizenSelection {
                boundary_va: CITIZEN_EXISTING_BUILDING_BRANCH_VA,
                fixed_building_prefix: starting_citizens.fixed_building_prefix,
            }),
        });
    }

    call_count_guard(calls.len(), citizens)?;
    for index in 0..citizens.max(0) {
        append_call(
            &mut calls,
            inputs,
            StartingUnitPhase::Citizen { index },
            CITIZEN_SIMPLE_CALL_VA,
            inputs.types.citizen,
        );
    }
    Ok(BuildUnitsPlan {
        calls,
        starting_citizens,
        citizens_after_modifiers: citizens,
        stop: None,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectRandomDrawReceipt {
    pub call_va: u32,
    pub state_before: i32,
    pub returned: i32,
    pub state_after: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InitUnitRngSpan {
    pub body_va: u32,
    pub body_bytes: u32,
    pub state_before: i32,
    pub state_after: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlacementRngEvent {
    DirectOffset(DirectRandomDrawReceipt),
    InitUnit(InitUnitRngSpan),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct StableUnitIdentityReceipt {
    pub id: u32,
    pub generation: u32,
    pub owner: i32,
    pub o: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineContainerShapeReceipt {
    pub length: i32,
    pub capacity: i32,
    pub increment: i16,
    pub flags: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuyIdentityReceipt {
    pub slot: i32,
    pub who: i8,
    pub o: i16,
    pub guy_num: i8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitMemberAuthorityReceipt {
    pub identity: StableUnitIdentityReceipt,
    pub ptype_index: i32,
    pub launching_is_null: bool,
    pub path: EngineContainerShapeReceipt,
    pub order_count: i32,
    pub guys: EngineContainerShapeReceipt,
    pub guy_mark: i8,
    pub guy_identities: Vec<GuyIdentityReceipt>,
    /// Stable key installed in `UnitsWalkAuthority`.
    pub units_authority_key: (u32, u32),
    /// Stable key installed in `GuysWalkAuthority`.
    pub guys_authority_key: (u32, u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitUnitAuthorityReceipt {
    /// Evidence that the existing 1,603-byte detailed receipt validator admitted the
    /// transaction. The surrounding fields retain the effects needed by replay channels.
    pub validated_body_va: u32,
    pub validated_body_bytes: u32,
    pub unit_mark_before: i32,
    pub unit_mark_after: i32,
    pub returned_captain_o: i32,
    pub members: Vec<UnitMemberAuthorityReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlacementOutcomeReceipt {
    Spawned(InitUnitAuthorityReceipt),
    QueuedAtCenterBuild {
        city_o: i32,
        trained_type: i32,
        returned: i32,
    },
    Failed {
        returned: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceUnitReceipt {
    pub call: PlaceUnitCall,
    pub rng_before: i32,
    pub rng_after: i32,
    pub rng_events: Vec<PlacementRngEvent>,
    pub outcome: PlacementOutcomeReceipt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildUnitsPrefixReceipt {
    pub rng_initial: i32,
    pub rng_final: i32,
    pub placements: Vec<PlaceUnitReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildUnitsReceiptError {
    CountMismatch,
    CallMismatch { ordinal: usize },
    RngChain { ordinal: usize },
    InvalidDirectDraw { ordinal: usize },
    InvalidInitSpan { ordinal: usize },
    InitSpanOutcomeMismatch { ordinal: usize },
    InvalidInitAuthority { ordinal: usize },
    InvalidMemberAuthority { ordinal: usize, member: usize },
    DuplicateIdentity { ordinal: usize, member: usize },
    InvalidQueuedOutcome { ordinal: usize },
}

fn validate_member(call: PlaceUnitCall, member: &UnitMemberAuthorityReceipt) -> bool {
    let expected_guys = call.squad_size.wrapping_add(call.crew_size);
    let id = member.identity;
    if id.owner != call.owner
        || !(0..UNIT_BAND_LIMIT).contains(&id.o)
        || member.ptype_index != call.place_unit_upgrade
        || !member.launching_is_null
        || member.path.length != 0
        || member.path.capacity != 10
        || member.path.increment != -1
        || member.order_count != 0
        || member.guys.length != expected_guys
        || member.guys.capacity < member.guys.length
        || i32::from(member.guy_mark) != call.squad_size
        || member.units_authority_key != (id.id, id.generation)
        || member.guys_authority_key != (id.id, id.generation)
        || member.guy_identities.len() != expected_guys as usize
    {
        return false;
    }
    member.guy_identities.iter().enumerate().all(|(slot, guy)| {
        guy.slot == slot as i32
            && guy.who == call.owner as i8
            && guy.o == id.o as i16
            && guy.guy_num == slot as i8
    })
}

/// Validate a receipt sequence without fitting or consulting a recorded checksum.
///
/// Direct `Random::get(0, 0xffff)` results are recomputed from their pre-state. Nested
/// `Objects::init_unit` RNG is retained as a separately bounded span because Guy animation
/// selection remains the next producer. Stable sparse identity and both channel-authority
/// keys are checked for every linked member.
pub fn validate_build_units_prefix_receipt(
    plan: &BuildUnitsPlan,
    receipt: &BuildUnitsPrefixReceipt,
) -> Result<(), BuildUnitsReceiptError> {
    if receipt.placements.len() != plan.calls.len() {
        return Err(BuildUnitsReceiptError::CountMismatch);
    }
    let mut rng = receipt.rng_initial;
    let mut identities = BTreeSet::new();

    for (ordinal, (expected, placement)) in
        plan.calls.iter().zip(receipt.placements.iter()).enumerate()
    {
        if placement.call != *expected {
            return Err(BuildUnitsReceiptError::CallMismatch { ordinal });
        }
        if placement.rng_before != rng {
            return Err(BuildUnitsReceiptError::RngChain { ordinal });
        }
        let mut event_rng = rng;
        let mut init_spans = 0usize;
        for event in &placement.rng_events {
            match *event {
                PlacementRngEvent::DirectOffset(draw) => {
                    if draw.call_va != PLACE_UNIT_DIRECT_RANDOM_CALL_VA
                        || draw.state_before != event_rng
                    {
                        return Err(BuildUnitsReceiptError::InvalidDirectDraw { ordinal });
                    }
                    let mut expected_rng = Random::new(event_rng);
                    let returned = expected_rng.get(0, 0xffff);
                    if draw.returned != returned || draw.state_after != expected_rng.state() {
                        return Err(BuildUnitsReceiptError::InvalidDirectDraw { ordinal });
                    }
                    event_rng = draw.state_after;
                }
                PlacementRngEvent::InitUnit(span) => {
                    init_spans += 1;
                    if init_spans != 1
                        || span.body_va != OBJECTS_INIT_UNIT_VA
                        || span.body_bytes != OBJECTS_INIT_UNIT_BYTES
                        || span.state_before != event_rng
                    {
                        return Err(BuildUnitsReceiptError::InvalidInitSpan { ordinal });
                    }
                    event_rng = span.state_after;
                }
            }
        }
        if placement.rng_after != event_rng {
            return Err(BuildUnitsReceiptError::RngChain { ordinal });
        }

        match &placement.outcome {
            PlacementOutcomeReceipt::Spawned(init) => {
                if init_spans != 1 {
                    return Err(BuildUnitsReceiptError::InitSpanOutcomeMismatch { ordinal });
                }
                if init.validated_body_va != OBJECTS_INIT_UNIT_VA
                    || init.validated_body_bytes != OBJECTS_INIT_UNIT_BYTES
                    || init.members.len() != expected.uber_size as usize
                    || init.members.first().map(|m| m.identity.o) != Some(init.returned_captain_o)
                    || init.unit_mark_after < init.unit_mark_before
                {
                    return Err(BuildUnitsReceiptError::InvalidInitAuthority { ordinal });
                }
                for (member_index, member) in init.members.iter().enumerate() {
                    if !validate_member(*expected, member) {
                        return Err(BuildUnitsReceiptError::InvalidMemberAuthority {
                            ordinal,
                            member: member_index,
                        });
                    }
                    if !identities.insert(member.identity) {
                        return Err(BuildUnitsReceiptError::DuplicateIdentity {
                            ordinal,
                            member: member_index,
                        });
                    }
                }
            }
            PlacementOutcomeReceipt::QueuedAtCenterBuild {
                city_o,
                trained_type,
                ..
            } => {
                if init_spans != 0
                    || expected.center_city_o < 0
                    || *city_o != expected.center_city_o
                    || *trained_type != expected.place_unit_upgrade
                {
                    return Err(BuildUnitsReceiptError::InvalidQueuedOutcome { ordinal });
                }
            }
            PlacementOutcomeReceipt::Failed { .. } => {
                if init_spans != 0 {
                    return Err(BuildUnitsReceiptError::InitSpanOutcomeMismatch { ordinal });
                }
            }
        }
        rng = placement.rng_after;
    }
    if rng != receipt.rng_final {
        return Err(BuildUnitsReceiptError::RngChain {
            ordinal: plan.calls.len(),
        });
    }
    Ok(())
}
