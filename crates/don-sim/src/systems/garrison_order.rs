// SPDX-License-Identifier: GPL-3.0-or-later
//! Source-only reconstruction of GARRISON installation and one `Unit::do_garrison` frame.
//!
//! This module is deliberately not registered in `systems/mod.rs`.  It is a pure, fail-closed
//! proof surface for the shipped branch decisions and host-call ordering.  Shared payload,
//! command, save/load, dispatcher, and live-tick wiring are deferred to the integration map in
//! `docs/assembly/garrison-order.md`.

pub const GARRISON_ORDER_INDEX: i32 = 26;
pub const UNIT_ADD_GARRISON_ORDER_VA: u32 = 0x005e_4080;
pub const UNIT_DO_GARRISON_VA: u32 = 0x005e_6b80;
pub const UNIT_KILL_GARRISON_ORDER_VA: u32 = 0x005e_2bd0;
pub const GROUP_ACTION_GARRISON_VA: u32 = 0x0070_0490;
pub const COMMAND_PROCESS_GARRISON_VA: u32 = 0x0094_8760;
pub const GARRISON_WIRE_BYTES: usize = 13;

pub const GARRISON_ORDER_SIZE: usize = 36;
pub const GARRISON_WALKED_BYTES: usize = 15;
pub const ORDER_GROUP: u8 = 4;
pub const QUEUE_FIRST: i32 = 0;
pub const QUEUE_LAST: i32 = 1;
pub const QUEUE_NEW: i32 = 2;

pub const AIRBASE_TYPE: i32 = 0x1bf;
pub const DOCK_TYPE: i32 = 0x1b0;
pub const GARRISON_FILTER_NOT_ME: i32 = 3;
pub const GARRISON_APPROACH_BASE: i32 = 0x30;
pub const GARRISON_APPROACH_SCALE: i32 = 0x60;
pub const GARRISON_DOCK_BUMP: i32 = 0x180;

/// Compiler-emitted concrete layout from the matching shipped PDB.
pub mod offsets {
    pub const OX: usize = 0x08;
    pub const WHOM: usize = 0x0c;
    pub const UID: usize = 0x10;
    pub const SEARCH: usize = 0x14;
    pub const UNIT_ORDER_VBASE: usize = 0x1c;
    pub const FLAGS: usize = 0x20;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GarrisonIdentity {
    pub o: i32,
    pub who: i32,
    pub uid: u16,
}

/// Complete checksum-visible concrete payload, excluding the virtual-base flag byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GarrisonOrderState {
    pub target: GarrisonIdentity,
    pub search: i32,
}

impl Default for GarrisonOrderState {
    fn default() -> Self {
        Self {
            target: GarrisonIdentity {
                o: -1,
                who: -1,
                uid: u16::MAX,
            },
            search: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GarrisonInstallRequest {
    pub target_o: i32,
    pub target_who: i32,
    pub search: i32,
    pub queue_pos: i32,
    pub group_flag: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GarrisonInstallStep {
    ClearActorMask(u32),
    StoreActorC0(i32),
    CloseOrders(i32),
    ClearPartialPath,
    UpdateAction,
    AllocateOrder(i32),
    AddOrder,
    RotateFirstOrderToHead,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GarrisonInstallPlan {
    pub order: GarrisonOrderState,
    pub flags: u8,
    pub steps: Vec<GarrisonInstallStep>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GarrisonPlanError {
    MissingInstallTarget,
    InstallTargetSlotMismatch,
    MalformedTargetAddress,
    MissingTarget,
    TargetSlotMismatch,
    TargetUidNotPrevalidated,
    MissingAirbasePredicate,
    UnexpectedAirbasePredicate,
    MissingCanCarry,
    UnexpectedCanCarry,
    MissingAdmissionPredicate,
    MissingDiplomacyPredicate,
    MissingCapacityLimit,
    MissingCanGarrison,
    MissingReachability,
    MissingApproachFacts,
    UnexpectedApproachFacts,
    MissingFirstSpot,
    MissingSecondSpot,
    UnexpectedSecondSpot,
    MissingCityOwner,
    MissingCityMetric,
    MissingOccupancy,
    MissingAlternateCityFact,
    MissingAlternateResult,
    UnexpectedAlternateResult,
    MissingTerrainOwner,
    MissingTerrainAlliance,
    MissingCaptain,
    MissingTargetIsBuild,
    WireMissingTargetActive,
    WireUnexpectedTargetActive,
    UnexpectedTerrainAlliance,
    ReceiptIdentityMismatch,
    ReceiptSnapshotMismatch,
    ReceiptPlanMismatch,
}

/// Exact `Unit::add_garrison_order(ox, whom, search, queue_pos, group_flag)` after-image.
pub fn plan_garrison_install(
    request: GarrisonInstallRequest,
    target: Option<GarrisonIdentity>,
) -> Result<GarrisonInstallPlan, GarrisonPlanError> {
    let uid = if request.target_o < 0 || request.target_who < 0 {
        u16::MAX
    } else {
        let target = target.ok_or(GarrisonPlanError::MissingInstallTarget)?;
        if target.o != request.target_o || target.who != request.target_who {
            return Err(GarrisonPlanError::InstallTargetSlotMismatch);
        }
        target.uid
    };

    let mut steps = Vec::new();
    if request.queue_pos == QUEUE_NEW {
        steps.extend([
            GarrisonInstallStep::ClearActorMask(0x0400_0000),
            GarrisonInstallStep::StoreActorC0(0),
            GarrisonInstallStep::CloseOrders(0),
            GarrisonInstallStep::ClearPartialPath,
            GarrisonInstallStep::UpdateAction,
        ]);
    }
    steps.push(GarrisonInstallStep::AllocateOrder(GARRISON_ORDER_INDEX));
    steps.push(GarrisonInstallStep::AddOrder);
    if request.queue_pos == QUEUE_FIRST {
        steps.push(GarrisonInstallStep::ClearPartialPath);
        steps.push(GarrisonInstallStep::RotateFirstOrderToHead);
    }
    steps.push(GarrisonInstallStep::UpdateAction);

    Ok(GarrisonInstallPlan {
        order: GarrisonOrderState {
            target: GarrisonIdentity {
                o: request.target_o,
                who: request.target_who,
                uid,
            },
            search: request.search,
        },
        flags: if request.group_flag == 0 {
            0
        } else {
            ORDER_GROUP
        },
        steps,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GarrisonWireCommand {
    pub target_o: i32,
    pub target_who: i32,
    pub queue_pos: i32,
}

impl GarrisonWireCommand {
    pub fn decode(bytes: [u8; GARRISON_WIRE_BYTES]) -> Self {
        Self {
            target_o: i32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]),
            target_who: i32::from_le_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]),
            queue_pos: i32::from_le_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GarrisonWireFacts {
    pub package_has_group: bool,
    /// Required only when both target coordinates are nonnegative.
    pub target_active: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GarrisonWireStep {
    LogDecodedCommand,
    RecordReplayTrace,
    GroupActionGarrison {
        target_o: i32,
        target_who: i32,
        queue_pos: i32,
        search_for_alternate: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GarrisonWirePlan {
    pub consumed: usize,
    pub steps: Vec<GarrisonWireStep>,
}

/// `CommandPackage::process_garrison`: decode/log/trace, then conditionally enter the group
/// installer.  A negative component deliberately bypasses the target-active lookup.
pub fn plan_garrison_wire(
    command: GarrisonWireCommand,
    facts: GarrisonWireFacts,
) -> Result<GarrisonWirePlan, GarrisonPlanError> {
    let mut steps = vec![
        GarrisonWireStep::LogDecodedCommand,
        GarrisonWireStep::RecordReplayTrace,
    ];
    if !facts.package_has_group {
        if facts.target_active.is_some() {
            return Err(GarrisonPlanError::WireUnexpectedTargetActive);
        }
        return Ok(GarrisonWirePlan {
            consumed: GARRISON_WIRE_BYTES,
            steps,
        });
    }
    let addressed = command.target_o >= 0 && command.target_who >= 0;
    let active = if addressed {
        facts
            .target_active
            .ok_or(GarrisonPlanError::WireMissingTargetActive)?
    } else {
        if facts.target_active.is_some() {
            return Err(GarrisonPlanError::WireUnexpectedTargetActive);
        }
        true
    };
    if active {
        steps.push(GarrisonWireStep::GroupActionGarrison {
            target_o: command.target_o,
            target_who: command.target_who,
            queue_pos: command.queue_pos,
            search_for_alternate: 0,
        });
    }
    Ok(GarrisonWirePlan {
        consumed: GARRISON_WIRE_BYTES,
        steps,
    })
}

/// Exact installation call selected by the `Group::action_garrison` member loop after its
/// alliance, liveness, on-map, enter/exit, compatibility, and optional alternate-building
/// gates.  Those host-owned predicates are receipt inputs; this enum freezes the four distinct
/// call sites/paths that converge on `Unit::add_garrison_order`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GarrisonGroupInstallPath {
    /// The ordinary path preserves the caller's `search` and queue words.
    Normal,
    /// A worker in the special Queue::New arm receives Queue::First and the raw EDX value left
    /// by `ObjectData::is_worker`.  Retail does not reload the caller's search argument.
    NewWorker { post_is_worker_edx: i32 },
    /// A unit that is still unpacking receives Queue::New.
    NewUnpacking,
    /// A packing-but-not-unpacking unit receives Queue::Last, after the optional queue repair.
    NewPacking,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GarrisonGroupInstallRequest {
    pub chosen_target_o: i32,
    pub target_who: i32,
    pub caller_search: i32,
    pub caller_queue_pos: i32,
    pub path: GarrisonGroupInstallPath,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GarrisonGroupInstallCall {
    pub target_o: i32,
    pub target_who: i32,
    pub search: i32,
    pub queue_pos: i32,
    pub group_flag: i32,
}

pub fn plan_group_member_install(request: GarrisonGroupInstallRequest) -> GarrisonGroupInstallCall {
    let (search, queue_pos) = match request.path {
        GarrisonGroupInstallPath::Normal => (request.caller_search, request.caller_queue_pos),
        GarrisonGroupInstallPath::NewWorker { post_is_worker_edx } => {
            (post_is_worker_edx, QUEUE_FIRST)
        }
        GarrisonGroupInstallPath::NewUnpacking => (request.caller_search, QUEUE_NEW),
        GarrisonGroupInstallPath::NewPacking => (request.caller_search, QUEUE_LAST),
    };
    GarrisonGroupInstallCall {
        target_o: request.chosen_target_o,
        target_who: request.target_who,
        search,
        queue_pos,
        group_flag: 1,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GarrisonGroupEntryPlan {
    pub opens_first_transaction: bool,
    pub effective_queue_pos: i32,
    pub effective_search: i32,
}

/// `QueuePos::First` is implemented by a Group insertion wrapper which recursively reissues
/// `(ox,whom,QueuePos::New,0)`.  It therefore discards even a nonzero caller search word.
pub fn plan_group_entry(queue_pos: i32, search: i32) -> GarrisonGroupEntryPlan {
    if queue_pos == QUEUE_FIRST {
        GarrisonGroupEntryPlan {
            opens_first_transaction: true,
            effective_queue_pos: QUEUE_NEW,
            effective_search: 0,
        }
    } else {
        GarrisonGroupEntryPlan {
            opens_first_transaction: false,
            effective_queue_pos: queue_pos,
            effective_search: search,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GarrisonActorFacts {
    pub identity: GarrisonIdentity,
    pub x: i32,
    pub y: i32,
    pub unit_masks: u32,
    pub type_flags_2b4: u32,
    pub inside_cost: i32,
    pub local_player: bool,
    /// Actor virtual slot `+0x170(target_o,target_who)`.
    pub reaches_target: Option<bool>,
    /// `ObjectData::get_captain()` at virtual slot `+0xe4`, consumed on successful entry.
    pub captain_o: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GarrisonTargetFacts {
    pub identity: GarrisonIdentity,
    /// Raw target object address forwarded by the helicopter/Airbase-full move tail.
    pub raw_address: u32,
    pub x: i32,
    pub y: i32,
    pub owner: i32,
    /// Target `SubObjectData::is_valid_wall()` at virtual slot `+0x0c`.
    pub is_valid_wall: Option<bool>,
    /// Target `SubObjectData::is_active()` at virtual slot `+0x4c`.
    pub is_active: Option<bool>,
    pub diplomacy_allows: Option<bool>,
    pub capacity_limit: Option<i32>,
    pub actor_type_can_garrison: Option<bool>,
    /// Target `ObjectData::is(0x1bf,0)` (Airbase), queried only for helicopter actors.
    pub is_airbase: Option<bool>,
    pub can_carry_actor: Option<bool>,
    pub footprint_x: i32,
    pub footprint_y: i32,
    pub is_dock: Option<bool>,
    /// Flags at `+0x08` on the target's mutable Build object.
    pub build_flags: u32,
    pub city_index: i32,
    /// `CityData::race` at `+0x5f`.
    pub city_race: Option<i32>,
    pub hits: Option<i32>,
    pub hits_left: Option<i32>,
    pub occupied: Option<i32>,
    /// Signed owner byte from the terrain tile under the target; `-1` is a known no-owner.
    pub terrain_owner: Option<i32>,
    pub target_owner_allied_with_terrain: Option<bool>,
    /// Target `SubObjectData::is_build()` at virtual slot `+0x20` after `go_inside`.
    pub is_build: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GarrisonSpotResult {
    Found { x: i32, y: i32, post_call_ecx: u32 },
    Failed { post_call_ecx: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GarrisonApproachFacts {
    pub angle: i32,
    pub first: Option<GarrisonSpotResult>,
    pub second: Option<GarrisonSpotResult>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GarrisonAlternateFacts {
    /// Active bit on the city object addressed by target `BuildData::city`.
    pub city_object_active: Option<bool>,
    /// `Unit::find_garrison_build(city_index,target_owner)`; negative means no alternate.
    pub alternate_o: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GarrisonExecutorRequest {
    pub actor: GarrisonActorFacts,
    pub order: GarrisonOrderState,
    pub order_flags: u8,
    /// `Unit::work` owns the stale UID gate before dispatching this function.
    pub outer_uid_validated: bool,
    pub target: Option<GarrisonTargetFacts>,
    pub approach: Option<GarrisonApproachFacts>,
    pub alternate: Option<GarrisonAlternateFacts>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GarrisonSpotRequest {
    pub center_x: i32,
    pub center_y: i32,
    pub min_radius: i32,
    pub max_radius: i32,
    pub step: i32,
    pub angle: i32,
    pub filter: i32,
    pub actor_o: i32,
    pub actor_who: i32,
    pub tail_0: i32,
    pub tail_1: i32,
    pub tail_2: i32,
    pub tail_3: i32,
    pub tail_4: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GarrisonFeedback {
    AirbaseFull,
    CityOwnershipOrMetric,
    GarrisonFull,
    HostileTerritory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GarrisonHostStep {
    KillCurrentOrder {
        arg: i32,
    },
    AddStrafeOrder {
        x: i32,
        y: i32,
        target_o: i32,
        target_who: i32,
        arg5: i32,
        queue_pos: i32,
        arg7: i32,
    },
    UpdateOrder,
    StoreStrafeI32 {
        offset: usize,
        value: i32,
    },
    FindNearbySpot {
        request: GarrisonSpotRequest,
        result: GarrisonSpotResult,
    },
    AddMoveOrder {
        x: i32,
        y: i32,
        arg3: i32,
        arg4: i32,
        queue_pos: i32,
        arg6: i32,
        opaque_arg7: u32,
        tail_x: i32,
        tail_y: i32,
    },
    LocalFeedback(GarrisonFeedback),
    PlaySound {
        category: i32,
    },
    FindGarrisonBuild {
        city_index: i32,
        target_owner: i32,
        result_o: i32,
    },
    AddGarrisonOrder {
        target_o: i32,
        target_who: i32,
        search: i32,
        queue_pos: i32,
        group_flag: i32,
    },
    GoInside {
        captain_o: i32,
        target_o: i32,
        target_who: i32,
        arg3: i32,
    },
    ReadTargetIsBuild {
        value: bool,
    },
    SetOptionsRebuild {
        value: i32,
    },
    KillCaptainGarrisonOrder {
        captain_o: i32,
        arg: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GarrisonExecutorBranch {
    HelicopterAirbaseStrafe,
    HelicopterAirbaseMove,
    InvalidAdmission,
    DiplomacyRejected,
    NoGarrisonCapacity,
    IncompatibleActor,
    ApproachFirst,
    ApproachSecond,
    ApproachFailed,
    CityOwnershipRejected,
    CityMetricRejected,
    CapacityFullAlternate,
    CapacityFull,
    HostileTerritory,
    Entered,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GarrisonExecutorPlan {
    pub branch: GarrisonExecutorBranch,
    pub steps: Vec<GarrisonHostStep>,
    /// `Unit::do_garrison` has no direct call into the canonical game RNG.
    pub direct_rng_draws: usize,
}

fn plan(branch: GarrisonExecutorBranch, steps: Vec<GarrisonHostStep>) -> GarrisonExecutorPlan {
    GarrisonExecutorPlan {
        branch,
        steps,
        direct_rng_draws: 0,
    }
}

fn local_feedback(actor: GarrisonActorFacts, kind: GarrisonFeedback) -> Vec<GarrisonHostStep> {
    if actor.local_player {
        let mut steps = vec![GarrisonHostStep::LocalFeedback(kind)];
        if matches!(
            kind,
            GarrisonFeedback::GarrisonFull | GarrisonFeedback::HostileTerritory
        ) {
            steps.push(GarrisonHostStep::PlaySound { category: 0x40 });
        }
        steps
    } else {
        Vec::new()
    }
}

fn terminal(
    actor: GarrisonActorFacts,
    branch: GarrisonExecutorBranch,
    feedback: Option<GarrisonFeedback>,
    kill_first: bool,
) -> GarrisonExecutorPlan {
    let kill = GarrisonHostStep::KillCurrentOrder { arg: 0 };
    let mut steps = Vec::new();
    if kill_first {
        steps.push(kill);
    }
    if let Some(feedback) = feedback {
        steps.extend(local_feedback(actor, feedback));
    }
    if !kill_first {
        steps.push(kill);
    }
    plan(branch, steps)
}

fn spot_request(
    actor: GarrisonActorFacts,
    target: GarrisonTargetFacts,
    angle: i32,
    min_radius: i32,
    max_radius: i32,
    retry_tail: i32,
) -> GarrisonSpotRequest {
    GarrisonSpotRequest {
        center_x: target.x,
        center_y: target.y,
        min_radius,
        max_radius,
        step: 0,
        angle,
        filter: GARRISON_FILTER_NOT_ME,
        actor_o: actor.identity.o,
        actor_who: actor.identity.who,
        tail_0: 0,
        tail_1: retry_tail,
        tail_2: -1,
        tail_3: 0,
        tail_4: -1,
    }
}

fn found_move(result: GarrisonSpotResult) -> Option<GarrisonHostStep> {
    match result {
        GarrisonSpotResult::Found {
            x,
            y,
            post_call_ecx,
        } => Some(GarrisonHostStep::AddMoveOrder {
            x,
            y,
            arg3: 1,
            arg4: 0,
            queue_pos: QUEUE_FIRST,
            arg6: 0,
            opaque_arg7: post_call_ecx,
            tail_x: -1,
            tail_y: -1,
        }),
        GarrisonSpotResult::Failed { .. } => None,
    }
}

/// Complete retail branch plan for one `Unit::do_garrison` activation.
pub fn plan_garrison_executor(
    request: GarrisonExecutorRequest,
) -> Result<GarrisonExecutorPlan, GarrisonPlanError> {
    if request.order.target.o < 0 || request.order.target.who < 0 {
        return Err(GarrisonPlanError::MalformedTargetAddress);
    }
    if !request.outer_uid_validated {
        return Err(GarrisonPlanError::TargetUidNotPrevalidated);
    }
    let target = request.target.ok_or(GarrisonPlanError::MissingTarget)?;
    if target.identity != request.order.target || target.owner != target.identity.who {
        return Err(GarrisonPlanError::TargetSlotMismatch);
    }

    let actor_special = request.actor.type_flags_2b4 & 0x20 != 0;
    if actor_special {
        let airbase = target
            .is_airbase
            .ok_or(GarrisonPlanError::MissingAirbasePredicate)?;
        if airbase {
            let can_carry = target
                .can_carry_actor
                .ok_or(GarrisonPlanError::MissingCanCarry)?;
            if can_carry {
                return Ok(plan(
                    GarrisonExecutorBranch::HelicopterAirbaseStrafe,
                    vec![
                        GarrisonHostStep::KillCurrentOrder { arg: 0 },
                        GarrisonHostStep::AddStrafeOrder {
                            x: -1,
                            y: -1,
                            target_o: target.identity.o,
                            target_who: target.identity.who,
                            arg5: 1,
                            queue_pos: QUEUE_FIRST,
                            arg7: 1,
                        },
                        GarrisonHostStep::UpdateOrder,
                        GarrisonHostStep::StoreStrafeI32 {
                            offset: 0x3c,
                            value: 1,
                        },
                    ],
                ));
            }
            let mut steps = vec![
                GarrisonHostStep::KillCurrentOrder { arg: 0 },
                GarrisonHostStep::AddMoveOrder {
                    x: target.x,
                    y: target.y,
                    arg3: 1,
                    arg4: 0,
                    queue_pos: QUEUE_FIRST,
                    arg6: 0,
                    opaque_arg7: target.raw_address,
                    tail_x: -1,
                    tail_y: -1,
                },
            ];
            steps.push(GarrisonHostStep::LocalFeedback(
                GarrisonFeedback::AirbaseFull,
            ));
            return Ok(plan(GarrisonExecutorBranch::HelicopterAirbaseMove, steps));
        }
        if target.can_carry_actor.is_some() {
            return Err(GarrisonPlanError::UnexpectedCanCarry);
        }
    } else {
        if target.is_airbase.is_some() {
            return Err(GarrisonPlanError::UnexpectedAirbasePredicate);
        }
        if target.can_carry_actor.is_some() {
            return Err(GarrisonPlanError::UnexpectedCanCarry);
        }
    }

    let active = target
        .is_valid_wall
        .ok_or(GarrisonPlanError::MissingAdmissionPredicate)?;
    if !active {
        return Ok(terminal(
            request.actor,
            GarrisonExecutorBranch::InvalidAdmission,
            None,
            false,
        ));
    }
    let admitted = target
        .is_active
        .ok_or(GarrisonPlanError::MissingAdmissionPredicate)?;
    if !admitted {
        return Ok(terminal(
            request.actor,
            GarrisonExecutorBranch::InvalidAdmission,
            None,
            false,
        ));
    }
    if !target
        .diplomacy_allows
        .ok_or(GarrisonPlanError::MissingDiplomacyPredicate)?
    {
        return Ok(terminal(
            request.actor,
            GarrisonExecutorBranch::DiplomacyRejected,
            None,
            false,
        ));
    }
    let limit = target
        .capacity_limit
        .ok_or(GarrisonPlanError::MissingCapacityLimit)?;
    if limit == 0 {
        return Ok(terminal(
            request.actor,
            GarrisonExecutorBranch::NoGarrisonCapacity,
            None,
            false,
        ));
    }
    if !target
        .actor_type_can_garrison
        .ok_or(GarrisonPlanError::MissingCanGarrison)?
    {
        return Ok(terminal(
            request.actor,
            GarrisonExecutorBranch::IncompatibleActor,
            None,
            false,
        ));
    }

    let reaches = request
        .actor
        .reaches_target
        .ok_or(GarrisonPlanError::MissingReachability)?;
    if !reaches {
        let facts = request
            .approach
            .ok_or(GarrisonPlanError::MissingApproachFacts)?;
        let min_footprint = target.footprint_x.min(target.footprint_y);
        let mut min_radius = min_footprint
            .wrapping_mul(GARRISON_APPROACH_SCALE)
            .wrapping_add(GARRISON_APPROACH_BASE);
        let mut max_radius = -1;
        if target
            .is_dock
            .ok_or(GarrisonPlanError::MissingApproachFacts)?
        {
            min_radius = min_radius.wrapping_add(GARRISON_DOCK_BUMP);
            max_radius = min_radius.wrapping_add(GARRISON_DOCK_BUMP);
        }
        let first = facts.first.ok_or(GarrisonPlanError::MissingFirstSpot)?;
        let first_request = spot_request(
            request.actor,
            target,
            facts.angle,
            min_radius,
            max_radius,
            0,
        );
        let mut steps = vec![GarrisonHostStep::FindNearbySpot {
            request: first_request,
            result: first,
        }];
        if let Some(move_step) = found_move(first) {
            if facts.second.is_some() {
                return Err(GarrisonPlanError::UnexpectedSecondSpot);
            }
            steps.push(move_step);
            return Ok(plan(GarrisonExecutorBranch::ApproachFirst, steps));
        }
        let second = facts.second.ok_or(GarrisonPlanError::MissingSecondSpot)?;
        steps.push(GarrisonHostStep::FindNearbySpot {
            request: spot_request(
                request.actor,
                target,
                facts.angle,
                min_radius,
                max_radius,
                1,
            ),
            result: second,
        });
        if let Some(move_step) = found_move(second) {
            steps.push(move_step);
            return Ok(plan(GarrisonExecutorBranch::ApproachSecond, steps));
        }
        steps.push(GarrisonHostStep::KillCurrentOrder { arg: 0 });
        return Ok(plan(GarrisonExecutorBranch::ApproachFailed, steps));
    }
    if request.approach.is_some() {
        return Err(GarrisonPlanError::UnexpectedApproachFacts);
    }

    if target.build_flags & 0x20 != 0 {
        let city_race = target
            .city_race
            .ok_or(GarrisonPlanError::MissingCityOwner)?;
        if city_race != target.owner {
            return Ok(terminal(
                request.actor,
                GarrisonExecutorBranch::CityOwnershipRejected,
                Some(GarrisonFeedback::CityOwnershipOrMetric),
                false,
            ));
        }
        let hits = target.hits.ok_or(GarrisonPlanError::MissingCityMetric)?;
        let hits_left = target
            .hits_left
            .ok_or(GarrisonPlanError::MissingCityMetric)?;
        if hits_left < hits / 10 {
            return Ok(terminal(
                request.actor,
                GarrisonExecutorBranch::CityMetricRejected,
                Some(GarrisonFeedback::CityOwnershipOrMetric),
                false,
            ));
        }
    }

    let occupied = target.occupied.ok_or(GarrisonPlanError::MissingOccupancy)?;
    let actor_cost = if request.actor.unit_masks & 1 != 0 {
        0
    } else {
        request.actor.inside_cost
    };
    if occupied.wrapping_add(actor_cost) > limit {
        if request.order.search != 0 {
            if target.city_index >= 0 {
                let alternate = request
                    .alternate
                    .ok_or(GarrisonPlanError::MissingAlternateCityFact)?;
                let city_active = alternate
                    .city_object_active
                    .ok_or(GarrisonPlanError::MissingAlternateCityFact)?;
                if city_active {
                    let alternate_o = alternate
                        .alternate_o
                        .ok_or(GarrisonPlanError::MissingAlternateResult)?;
                    let mut steps = vec![GarrisonHostStep::FindGarrisonBuild {
                        city_index: target.city_index,
                        target_owner: target.owner,
                        result_o: alternate_o,
                    }];
                    if alternate_o >= 0 {
                        steps.push(GarrisonHostStep::KillCurrentOrder { arg: 0 });
                        steps.push(GarrisonHostStep::AddGarrisonOrder {
                            target_o: alternate_o,
                            target_who: target.owner,
                            search: 1,
                            queue_pos: QUEUE_FIRST,
                            group_flag: if request.order_flags & ORDER_GROUP == 0 {
                                0
                            } else {
                                1
                            },
                        });
                        return Ok(plan(GarrisonExecutorBranch::CapacityFullAlternate, steps));
                    }
                } else if alternate.alternate_o.is_some() {
                    return Err(GarrisonPlanError::UnexpectedAlternateResult);
                }
            } else if request.alternate.is_some() {
                return Err(GarrisonPlanError::UnexpectedAlternateResult);
            }
        } else if request.alternate.is_some() {
            return Err(GarrisonPlanError::UnexpectedAlternateResult);
        }
        return Ok(terminal(
            request.actor,
            GarrisonExecutorBranch::CapacityFull,
            Some(GarrisonFeedback::GarrisonFull),
            false,
        ));
    }
    if request.alternate.is_some() {
        return Err(GarrisonPlanError::UnexpectedAlternateResult);
    }

    let terrain_owner = target
        .terrain_owner
        .ok_or(GarrisonPlanError::MissingTerrainOwner)?;
    if terrain_owner >= 0 && terrain_owner != target.owner {
        let allied = target
            .target_owner_allied_with_terrain
            .ok_or(GarrisonPlanError::MissingTerrainAlliance)?;
        if !allied {
            return Ok(terminal(
                request.actor,
                GarrisonExecutorBranch::HostileTerritory,
                Some(GarrisonFeedback::HostileTerritory),
                true,
            ));
        }
    } else if target.target_owner_allied_with_terrain.is_some() {
        return Err(GarrisonPlanError::UnexpectedTerrainAlliance);
    }
    let captain_o = request
        .actor
        .captain_o
        .ok_or(GarrisonPlanError::MissingCaptain)?;
    let is_build = target
        .is_build
        .ok_or(GarrisonPlanError::MissingTargetIsBuild)?;
    let mut steps = vec![GarrisonHostStep::GoInside {
        captain_o,
        target_o: target.identity.o,
        target_who: target.identity.who,
        arg3: 0,
    }];
    steps.push(GarrisonHostStep::ReadTargetIsBuild { value: is_build });
    if is_build {
        steps.push(GarrisonHostStep::SetOptionsRebuild { value: 1 });
    }
    steps.push(GarrisonHostStep::KillCaptainGarrisonOrder { captain_o, arg: 0 });
    Ok(plan(GarrisonExecutorBranch::Entered, steps))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GarrisonHostSnapshot {
    pub actor: GarrisonIdentity,
    pub target: GarrisonIdentity,
    pub actor_version: u64,
    pub target_version: u64,
    pub queue_digest: u64,
    pub path_digest: u64,
    pub external_epoch: u64,
    pub rng_epoch: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GarrisonExecutorReceipt {
    pub snapshot: GarrisonHostSnapshot,
    pub request: GarrisonExecutorRequest,
    pub plan: GarrisonExecutorPlan,
}

/// Build an immutable receipt after host lookups and before any mutation is published.
pub fn preflight_garrison_executor(
    snapshot: GarrisonHostSnapshot,
    request: GarrisonExecutorRequest,
) -> Result<GarrisonExecutorReceipt, GarrisonPlanError> {
    if snapshot.actor != request.actor.identity || snapshot.target != request.order.target {
        return Err(GarrisonPlanError::ReceiptIdentityMismatch);
    }
    let plan = plan_garrison_executor(request)?;
    Ok(GarrisonExecutorReceipt {
        snapshot,
        request,
        plan,
    })
}

/// Validate a receipt against the current host snapshot and recompute its plan.  A caller may
/// publish all listed steps atomically only after this succeeds; failure authorizes no local or
/// external mutation and no RNG consumption.
pub fn validate_garrison_receipt(
    receipt: &GarrisonExecutorReceipt,
    current: GarrisonHostSnapshot,
) -> Result<&GarrisonExecutorPlan, GarrisonPlanError> {
    if receipt.snapshot != current {
        return Err(GarrisonPlanError::ReceiptSnapshotMismatch);
    }
    let recomputed = plan_garrison_executor(receipt.request)?;
    if recomputed != receipt.plan {
        return Err(GarrisonPlanError::ReceiptPlanMismatch);
    }
    Ok(&receipt.plan)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GarrisonOpenTail {
    GroupActionGarrisonHostAdapter,
    ObjectAndTypeVirtualPredicates,
    DiplomacyTables,
    FindNearbySpotAndOpaqueEcx,
    MoveAndStrafeInsertion,
    CityMetricsAndTerrainOwner,
    FindAlternateGarrisonBuild,
    GoInsideAndCanonicalCaptain,
    LocalProductFeedback,
    ConcretePayloadSaveAndLiveTickAdapter,
}

pub const GARRISON_OPEN_TAILS: &[GarrisonOpenTail] = &[
    GarrisonOpenTail::GroupActionGarrisonHostAdapter,
    GarrisonOpenTail::ObjectAndTypeVirtualPredicates,
    GarrisonOpenTail::DiplomacyTables,
    GarrisonOpenTail::FindNearbySpotAndOpaqueEcx,
    GarrisonOpenTail::MoveAndStrafeInsertion,
    GarrisonOpenTail::CityMetricsAndTerrainOwner,
    GarrisonOpenTail::FindAlternateGarrisonBuild,
    GarrisonOpenTail::GoInsideAndCanonicalCaptain,
    GarrisonOpenTail::LocalProductFeedback,
    GarrisonOpenTail::ConcretePayloadSaveAndLiveTickAdapter,
];
