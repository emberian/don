//! Isolated reversal proof for the three shipped BHS unit-creation registrations.
//!
//! This module deliberately stops before `Objects::init_unit`.  The retail receiver is
//! non-atomic and owns object allocation, group replacement/appending, formation, transport,
//! Air strafe orders, and the Aircraft Carrier payload.  A pure prefix plan is useful only if
//! those owners consume it in the instruction-derived order; it is not permission to register
//! any of the three builtins as handled.

/// SHA-256 of `ron-bin/riseofnations.exe`, the PE32 image used for this reversal.
pub const SHIPPED_EXE_SHA256: &str =
    "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079";

pub const ADD_UNIT_VA: u32 = 0x009e_2220;
pub const ADD_UNIT_BYTES: u32 = 1_428;
pub const GET_TYPE_INDEX_VA: u32 = 0x00a0_3480;
pub const CURRENT_UPGRADE_VA: u32 = 0x006e_3140;
pub const GET_GRAFT_VA: u32 = 0x0047_df40;
pub const OBJECTS_INIT_UNIT_VA: u32 = 0x0065_e0c0;
pub const GROUP_CLEAR_VA: u32 = 0x004c_e470;
pub const GROUP_ADD_VA: u32 = 0x0071_4350;
pub const SCENARIO_ADD_TO_GROUP_VA: u32 = 0x004c_e5f0;
pub const GROUP_PUSH_VA: u32 = 0x0070_f9e0;
pub const GROUP_ACTION_TRANSPORT_VA: u32 = 0x0070_2620;
pub const GROUP_ACTION_FORM_VA: u32 = 0x0070_7220;

pub const MAX_CREATE_COUNT: u32 = 2_000;
pub const TRANSPORT_BARGE_TYPE: i32 = 0x140;
pub const FIGHTER_BOMBER_TYPE: i32 = 0x134;
pub const AIRCRAFT_CARRIER_TYPE: i32 = 0x15f;
pub const AIR_NO_STRAFE_FLAG: u32 = 0x20;
pub const DOMAIN_GROUND: i32 = 0;
pub const DOMAIN_SEA: i32 = 1;
pub const DOMAIN_AIR: i32 = 2;

pub const SHIPPED_CORPUS_FILES: u32 = 363;
pub const SHIPPED_CORPUS_CALLS: u32 = 39_957;
pub const CREATE_UNIT_COHORT_CALLS: u32 = 5_272;
pub const CREATE_UNIT_COHORT_FILES: u32 = 232;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreateUnitBuiltin {
    CreateUnit,
    CreateUnitUpgrade,
    CreateUnitInGroup,
}

impl CreateUnitBuiltin {
    pub const ALL: [Self; 3] = [
        Self::CreateUnit,
        Self::CreateUnitUpgrade,
        Self::CreateUnitInGroup,
    ];

    pub const fn registration(self) -> u32 {
        match self {
            Self::CreateUnit => 508,
            Self::CreateUnitUpgrade => 509,
            Self::CreateUnitInGroup => 510,
        }
    }

    pub const fn retail_va(self) -> u32 {
        match self {
            Self::CreateUnit => 0x009f_4c50,
            Self::CreateUnitUpgrade => 0x00a0_30e0,
            Self::CreateUnitInGroup => 0x009f_4c80,
        }
    }

    pub const fn retail_bytes(self) -> u32 {
        match self {
            Self::CreateUnit => 37,
            Self::CreateUnitUpgrade => 116,
            Self::CreateUnitInGroup => 165,
        }
    }

    /// The sixth `ScenarioFuncSet::add_unit` argument.
    pub const fn skips_group_clear(self) -> bool {
        matches!(self, Self::CreateUnitInGroup)
    }

    pub const fn resolves_current_upgrade(self) -> bool {
        !matches!(self, Self::CreateUnit)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreateUnitCensusRow {
    pub builtin: CreateUnitBuiltin,
    pub calls: u32,
    pub files: u32,
}

pub const CREATE_UNIT_CENSUS: [CreateUnitCensusRow; 3] = [
    CreateUnitCensusRow {
        builtin: CreateUnitBuiltin::CreateUnit,
        calls: 548,
        files: 91,
    },
    CreateUnitCensusRow {
        builtin: CreateUnitBuiltin::CreateUnitUpgrade,
        calls: 3_046,
        files: 182,
    },
    CreateUnitCensusRow {
        builtin: CreateUnitBuiltin::CreateUnitInGroup,
        calls: 1_678,
        files: 109,
    },
];

/// Inputs supplied by the BHS call after the VM has checked the registered signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateUnitRequest<'a> {
    pub who: i32,
    pub x: i32,
    pub y: i32,
    pub requested_type_name: &'a str,
    pub count: i32,
}

/// Retail derives both coordinate representations before the first validity branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreateUnitCoords {
    /// Arithmetic `x >> 2`, passed to `WorldData::is_valid` / `is_ocean`.
    pub world_x: i32,
    pub world_y: i32,
    /// `x * 192 + 96` with 32-bit x86 wrapping, passed to placement/allocation.
    pub coord_x: i32,
    pub coord_y: i32,
}

impl CreateUnitCoords {
    pub const fn from_script(x: i32, y: i32) -> Self {
        Self {
            world_x: x >> 2,
            world_y: y >> 2,
            coord_x: x.wrapping_mul(192).wrapping_add(96),
            coord_y: y.wrapping_mul(192).wrapping_add(96),
        }
    }
}

/// Read-only facts reached by the exact wrapper/add-unit prefix.
///
/// Every method is intentionally fallible.  Missing type, Leader, map, graft, or relation
/// ownership is a typed frontier failure rather than an invitation to approximate retail.
pub trait CreateUnitFacts {
    fn resolve_type(&self, name: &str) -> Option<i32>;
    fn canonical_type_name(&self, type_id: i32) -> Option<&str>;
    fn leader_flags(&self, leader_slot: usize) -> Option<u32>;
    fn current_upgrade(&self, leader_slot: usize, type_id: i32) -> Option<i32>;
    fn leader_graft(&self, leader_slot: usize, type_id: i32) -> Option<i32>;
    fn is_unit_type(&self, type_id: i32) -> Option<bool>;
    fn is_transport_barge_relation(&self, type_id: i32) -> Option<bool>;
    fn domain(&self, type_id: i32) -> Option<i32>;
    fn unit_flags(&self, type_id: i32) -> Option<u32>;
    fn world_valid(&self, world_x: i32, world_y: i32) -> Option<bool>;
    fn world_is_ocean(&self, world_x: i32, world_y: i32) -> Option<bool>;
    fn can_transport(&self, leader_slot: usize) -> Option<bool>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreateUnitPrefixError {
    RequestedTypeMissing,
    PlayerOutOfRange,
    LeaderFactsMissing,
    WrapperLeaderNotInGame,
    UpgradeFactsMissing,
    UpgradeNameMissing,
    CoreLeaderNotActive,
    CountOutOfRange,
    EffectiveTypeMissing,
    UnitTypeFactsMissing,
    NotAUnitType,
    WorldFactsMissing,
    InvalidWorldCoordinate,
    GraftFactsMissing,
    RelationFactsMissing,
    DomainFactsMissing,
    UnitFlagsMissing,
    TransportFactsMissing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreateUnitRoute {
    DirectGround,
    DirectSea,
    DirectAir {
        install_strafe_order: bool,
    },
    GroundViaTransport,
    /// Reached only after the wrapper-selected group-clear policy has already run.
    RejectAfterGroupPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateUnitPrefixPlan {
    pub builtin: CreateUnitBuiltin,
    pub leader_slot: usize,
    pub effective_type_name: String,
    pub effective_type: i32,
    pub graft_type: i32,
    pub count: u32,
    pub coords: CreateUnitCoords,
    /// `create_unit` and `create_unit_upgrade` call `clear_group(who - 1)`; the in-group form
    /// does not.  This numeric key is deliberately not conflated with `group_append_key`.
    pub clear_group_key: Option<i32>,
    /// Every successful allocation calls `add_to_group(who, object_id)`.  Retail therefore
    /// clears numeric key `who - 1` but appends numeric key `who`; the difference is real.
    pub group_append_key: i32,
    pub route: CreateUnitRoute,
}

/// The exact prefix state at the instruction immediately before the optional
/// `ScenarioFuncSet::clear_group` call.
///
/// Keeping this boundary separate is load-bearing for an executable adapter: retail clears
/// the numeric scenario group before it asks the effective type for the Transport Barge
/// relation, domain, Air flags, or the Leader transport predicate.  Missing authority for any
/// of those later reads therefore must not roll the clear back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateUnitPreGroupPlan {
    pub builtin: CreateUnitBuiltin,
    pub leader_slot: usize,
    pub effective_type_name: String,
    pub effective_type: i32,
    pub graft_type: i32,
    pub count: u32,
    pub coords: CreateUnitCoords,
    pub clear_group_key: Option<i32>,
    pub group_append_key: i32,
}

fn leader_slot(who: i32) -> Result<usize, CreateUnitPrefixError> {
    let slot = who.wrapping_sub(1);
    if (slot as u32) > 7 {
        Err(CreateUnitPrefixError::PlayerOutOfRange)
    } else {
        Ok(slot as usize)
    }
}

/// Recover the pure prefix common to registrations 508--510.
///
/// The ordering matters.  The two upgrade wrappers resolve the requested name before checking
/// `who`, require only Leader flag bit 0, and substitute the canonical current-upgrade name.
/// `add_unit` then repeats player validation, requires both low Leader bits, checks the unsigned
/// count bound, resolves the (possibly substituted) name, checks `is_unit_type`, validates the
/// world coordinate, and only then resolves the Leader graft.
pub fn plan_create_unit_pre_group(
    facts: &impl CreateUnitFacts,
    builtin: CreateUnitBuiltin,
    request: &CreateUnitRequest<'_>,
) -> Result<CreateUnitPreGroupPlan, CreateUnitPrefixError> {
    let effective_name = if builtin.resolves_current_upgrade() {
        let requested = facts
            .resolve_type(request.requested_type_name)
            .ok_or(CreateUnitPrefixError::RequestedTypeMissing)?;
        let slot = leader_slot(request.who)?;
        let flags = facts
            .leader_flags(slot)
            .ok_or(CreateUnitPrefixError::LeaderFactsMissing)?;
        if flags & 1 == 0 {
            return Err(CreateUnitPrefixError::WrapperLeaderNotInGame);
        }
        let upgraded = facts
            .current_upgrade(slot, requested)
            .ok_or(CreateUnitPrefixError::UpgradeFactsMissing)?;
        facts
            .canonical_type_name(upgraded)
            .ok_or(CreateUnitPrefixError::UpgradeNameMissing)?
            .to_owned()
    } else {
        request.requested_type_name.to_owned()
    };

    let slot = leader_slot(request.who)?;
    let flags = facts
        .leader_flags(slot)
        .ok_or(CreateUnitPrefixError::LeaderFactsMissing)?;
    if flags & 3 != 3 {
        return Err(CreateUnitPrefixError::CoreLeaderNotActive);
    }
    if (request.count as u32) > MAX_CREATE_COUNT {
        return Err(CreateUnitPrefixError::CountOutOfRange);
    }

    let effective_type = facts
        .resolve_type(&effective_name)
        .ok_or(CreateUnitPrefixError::EffectiveTypeMissing)?;
    match facts.is_unit_type(effective_type) {
        Some(true) => {}
        Some(false) => return Err(CreateUnitPrefixError::NotAUnitType),
        None => return Err(CreateUnitPrefixError::UnitTypeFactsMissing),
    }

    let coords = CreateUnitCoords::from_script(request.x, request.y);
    match facts.world_valid(coords.world_x, coords.world_y) {
        Some(true) => {}
        Some(false) => return Err(CreateUnitPrefixError::InvalidWorldCoordinate),
        None => return Err(CreateUnitPrefixError::WorldFactsMissing),
    }
    let graft_type = facts
        .leader_graft(slot, effective_type)
        .ok_or(CreateUnitPrefixError::GraftFactsMissing)?;

    let clear_group_key = (!builtin.skips_group_clear()).then_some(slot as i32);
    Ok(CreateUnitPreGroupPlan {
        builtin,
        leader_slot: slot,
        effective_type_name: effective_name,
        effective_type,
        graft_type,
        count: request.count as u32,
        coords,
        clear_group_key,
        group_append_key: request.who,
    })
}

/// Resolve the first reads after the optional numeric-group clear.
///
/// This function is pure, but callers must remember that retail has already applied
/// `pre.clear_group_key` when it runs.  An authority error here is therefore compatible with a
/// persistent clear side effect and must not be treated as an atomic prefix failure.
pub fn plan_create_unit_route(
    facts: &impl CreateUnitFacts,
    pre: &CreateUnitPreGroupPlan,
) -> Result<CreateUnitRoute, CreateUnitPrefixError> {
    let route = if facts
        .is_transport_barge_relation(pre.effective_type)
        .ok_or(CreateUnitPrefixError::RelationFactsMissing)?
    {
        CreateUnitRoute::RejectAfterGroupPolicy
    } else {
        let domain = facts
            .domain(pre.effective_type)
            .ok_or(CreateUnitPrefixError::DomainFactsMissing)?;
        let ocean = facts
            .world_is_ocean(pre.coords.world_x, pre.coords.world_y)
            .ok_or(CreateUnitPrefixError::WorldFactsMissing)?;
        match (domain, ocean) {
            (DOMAIN_GROUND, false) => CreateUnitRoute::DirectGround,
            (DOMAIN_SEA, true) => CreateUnitRoute::DirectSea,
            (DOMAIN_AIR, _) => {
                let unit_flags = facts
                    .unit_flags(pre.effective_type)
                    .ok_or(CreateUnitPrefixError::UnitFlagsMissing)?;
                CreateUnitRoute::DirectAir {
                    install_strafe_order: unit_flags & AIR_NO_STRAFE_FLAG == 0,
                }
            }
            (DOMAIN_GROUND, true) => {
                if facts
                    .can_transport(pre.leader_slot)
                    .ok_or(CreateUnitPrefixError::TransportFactsMissing)?
                {
                    CreateUnitRoute::GroundViaTransport
                } else {
                    CreateUnitRoute::RejectAfterGroupPolicy
                }
            }
            _ => CreateUnitRoute::RejectAfterGroupPolicy,
        }
    };

    Ok(route)
}

/// Recover the full pure prefix common to registrations 508--510.
///
/// Executable integrations should normally call [`plan_create_unit_pre_group`], apply the
/// selected clear, then call [`plan_create_unit_route`] so a missing post-clear owner cannot
/// accidentally make the native side effect atomic.  This combined helper remains the compact
/// proof/test interface.
pub fn plan_create_unit_prefix(
    facts: &impl CreateUnitFacts,
    builtin: CreateUnitBuiltin,
    request: &CreateUnitRequest<'_>,
) -> Result<CreateUnitPrefixPlan, CreateUnitPrefixError> {
    let pre = plan_create_unit_pre_group(facts, builtin, request)?;
    let route = plan_create_unit_route(facts, &pre)?;

    Ok(CreateUnitPrefixPlan {
        builtin: pre.builtin,
        leader_slot: pre.leader_slot,
        effective_type_name: pre.effective_type_name,
        effective_type: pre.effective_type,
        graft_type: pre.graft_type,
        count: pre.count,
        coords: pre.coords,
        clear_group_key: pre.clear_group_key,
        group_append_key: pre.group_append_key,
        route,
    })
}

/// Retail overwrites its return local after every `Objects::init_unit` attempt.
///
/// This is an object id on success, not a boolean status.  There is no rollback: earlier
/// successful allocations remain even when the final attempt returns `-1`.
pub fn retail_last_init_result(results: impl IntoIterator<Item = i32>) -> i32 {
    results.into_iter().last().unwrap_or(-1)
}

pub fn potential_corpus_percentage_points() -> f64 {
    f64::from(CREATE_UNIT_COHORT_CALLS) * 100.0 / f64::from(SHIPPED_CORPUS_CALLS)
}
