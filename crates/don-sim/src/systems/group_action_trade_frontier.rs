// SPDX-License-Identifier: GPL-3.0-or-later
//! Transaction frontier for retail `Group::action_trade`.
//!
//! This file is deliberately source-only.  It recovers the complete action-level decision
//! tree and the transitive `Unit::add_trade_order` payload without installing a command,
//! order, tick, save, or schema adapter.  Planning is mutation-free: a product host first
//! resolves one coherent fact snapshot, obtains an ordered transaction, and commits every
//! effect or none of them.

pub const GROUP_ACTION_TRADE_VA: u32 = 0x0070_1cc0;
pub const GROUP_ACTION_TRADE_BYTES: usize = 1_022;
pub const GROUP_ACTION_TRADE_END_VA: u32 = 0x0070_20be;
pub const GROUP_ACTION_TRADE_INSTRUCTIONS: usize = 299;
pub const GROUP_ACTION_BEGIN_VA: u32 = 0x0071_4100;
pub const GROUP_ACTION_HALT_VA: u32 = 0x0070_d0c0;
pub const GROUP_SET_UP_INSERT_VA: u32 = 0x0070_e520;
pub const GROUP_FINISH_INSERT_VA: u32 = 0x0070_e620;
pub const ORDER_LIST_CLEAR_VA: u32 = 0x0046_f600;
pub const GROUPDATA_COUNT_VA: u32 = 0x0071_1720;
pub const WORLD_GET_TREGION_VA: u32 = 0x006b_52e0;
pub const REGION_IS_COAST_VA: u32 = 0x0068_0f90;
pub const UNIT_ADD_TRADE_ORDER_VA: u32 = 0x005e_4dc0;
pub const UNIT_CLOSE_ORDERS_VA: u32 = 0x005e_37f0;
pub const UNIT_CLEAR_PARTIAL_PATH_VA: u32 = 0x005e_3920;
pub const UNIT_UPDATE_ACTION_VA: u32 = 0x0060_a870;
pub const ORDERS_GET_OBJ_VA: u32 = 0x0073_0ac0;
pub const ORDER_LIST_ADD_VA: u32 = 0x0046_d5a0;
pub const UNIT_TRANSPORT_TYPE_VA: u32 = 0x0046_f790;
pub const UNIT_CAN_EVER_TRANSPORT_VA: u32 = 0x0046_f290;

pub const QUEUE_FIRST: i32 = 0;
pub const QUEUE_LAST: i32 = 1;
pub const QUEUE_NEW: i32 = 2;
pub const TRADE_ROUTE_ORDER_INDEX: i32 = 15;
pub const TRADE_ROUTE_ORDER_SIZE: usize = 52;
pub const TRADE_GROUP_FLAG: u8 = 0x04;
pub const UNIT_ACTIVE_ORDER_MASK: u32 = 0x0400_0000;
pub const UNIT_HAS_TRADE_ROUTE_MASK: u32 = 0x0000_0200;
pub const UNIT_TRANSPORT_ROUTE_MASK: u32 = 0x0080_0000;
pub const TRADE_COUNT_INDEX: i32 = 0x11;
pub const TRADE_COUNT_LAND_TYPE: i32 = 0x3b;
pub const TRADE_COUNT_SEA_TYPE: i32 = 0x13e;
pub const ACTION_TRADE_READS_CITY: bool = false;
pub const ACTION_TRADE_READS_GOOD: bool = false;
pub const ACTION_TRADE_READS_RNG: bool = false;
pub const QUEUE_FIRST_COPIES_TRADE_ROUTE: bool = false;

pub const RETAIL_EXE_SHA256: &str =
    "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079";
pub const RETAIL_PDB_SHA256: &str =
    "334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5";

/// The exact binary/PDB/decoder tuple which admits this planner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetailTradeEvidence {
    pub executable_sha256: &'static str,
    pub pdb_sha256: &'static str,
    pub function_va: u32,
    pub function_bytes: usize,
    pub function_end_va: u32,
    pub capstone_instructions: usize,
}

impl RetailTradeEvidence {
    pub const SHIPPED: Self = Self {
        executable_sha256: RETAIL_EXE_SHA256,
        pdb_sha256: RETAIL_PDB_SHA256,
        function_va: GROUP_ACTION_TRADE_VA,
        function_bytes: GROUP_ACTION_TRADE_BYTES,
        function_end_va: GROUP_ACTION_TRADE_END_VA,
        capstone_instructions: GROUP_ACTION_TRADE_INSTRUCTIONS,
    };

    fn is_shipped(self) -> bool {
        self == Self::SHIPPED
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectKey {
    pub o: i32,
    pub who: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectIdentity {
    pub key: ObjectKey,
    pub uid: u16,
}

/// The complete concrete payload initialized by `Unit::add_trade_order`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeRouteOrderPayload {
    pub first: ObjectIdentity,
    pub second: ObjectIdentity,
    pub started: i32,
    pub loaded: i32,
    pub flags: u8,
}

/// Only fields read or written by this action are frozen here.  `group_slot` is the
/// containment identity; unlike `GroupData::id`, it remains stable if scenario pruning
/// clears the selected Group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TradeGroupSnapshot {
    pub group_slot: i32,
    pub id: i32,
    pub owner: u8,
    pub num: i32,
    pub form: i32,
    pub disband: i32,
    pub members: Vec<i16>,
}

impl TradeGroupSnapshot {
    fn well_formed(&self) -> bool {
        self.num >= 0 && self.num <= 128 && self.members.len() == self.num as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawTradeAction {
    pub ox: i32,
    pub whom: i32,
    pub oxx: i32,
    pub whose: i32,
    pub queued: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupActionTradeRequest {
    pub group: TradeGroupSnapshot,
    pub action: RawTradeAction,
}

/// One nonnegative entry visited by the scenario ignore-orders prelude.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScenarioKillCall {
    pub o: i32,
    pub who: i32,
    pub arg2: i32,
    pub arg3: i32,
}

/// Complete external receipt for the scenario-owned `Group::kill` prelude.  The economy
/// host must recompute `group_after`; this frontier only validates its call schedule and
/// uses the receipt as the next coherent snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioPruneFacts {
    pub ignore_orders: bool,
    pub ignored_objects: Vec<i32>,
    pub calls: Vec<ScenarioKillCall>,
    pub group_after: TradeGroupSnapshot,
    pub state_before_digest: u64,
    pub state_after_digest: u64,
    pub complete: bool,
}

/// Entry object reads.  Optional fields encode retail short-circuiting, not uncertainty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeTargetGateFacts {
    pub identity: ObjectIdentity,
    pub live_building: bool,
    pub active: Option<bool>,
    /// Result of `target->vt[0xB0]()->is_trade()` after the compiler's devirtualized arm.
    pub entry_receiver_is_trade: Option<bool>,
    pub entry_receiver_digest: Option<u64>,
}

/// One complete, read-only `WorldData::get_tregion` receipt. `tile_x/tile_y` are loaded
/// through `div_3_table[(coord ^ 0x63637) >> 6]` before the call. Separate receipts are
/// retained when the installer re-reads objects already observed by the Group body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainRegionReceipt {
    pub object: ObjectIdentity,
    pub x: i32,
    pub y: i32,
    pub tile_x: i32,
    pub tile_y: i32,
    pub tregion: i32,
    pub world_before_digest: u64,
    pub world_after_digest: u64,
    pub complete: bool,
}

impl TerrainRegionReceipt {
    fn is_read_only_complete(self) -> bool {
        self.complete && self.world_before_digest == self.world_after_digest
    }
}

/// One accepted member's fresh object lookups inside `Unit::add_trade_order`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeInstallFacts {
    pub actor: ObjectIdentity,
    pub unit_flags_before: u32,
    pub primary_lookup: Option<ObjectIdentity>,
    pub secondary_lookup: Option<ObjectIdentity>,
    /// Retail calls `get_tregion(primary)` first and `get_tregion(actor)` second even
    /// though it computes the actor tile pair first.
    pub primary_region: TerrainRegionReceipt,
    pub actor_region: TerrainRegionReceipt,
    /// PlayerData leader flag word; read only when the two regions differ.
    pub leader_flags: Option<u32>,
    pub transport_type: Option<i32>,
    /// Read only when the derived leader capability is at least `transport_type`.
    pub can_ever_transport: Option<bool>,
    pub orders_before_digest: u64,
    pub partial_path_before_digest: u64,
    pub action_before_digest: u64,
}

/// Per-member calls in the exact short-circuit shape used by the shipped body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeMemberFacts {
    pub identity: ObjectIdentity,
    pub live_unit: bool,
    pub on_map: Option<bool>,
    pub region: Option<TerrainRegionReceipt>,
    /// Destination's own vslot `+0x24`, re-read once for each live on-map member.
    pub target_is_trade: Option<bool>,
    pub is_caravan: Option<bool>,
    /// Destination vslot `+0x28`, read only when `target_is_trade == false`.
    pub target_is_sea_trade: Option<bool>,
    pub is_sea_trade_member: Option<bool>,
    pub regions_touch: Option<bool>,
    pub install: Option<TradeInstallFacts>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectTradeFacts {
    pub target_region: TerrainRegionReceipt,
    pub members: Vec<TradeMemberFacts>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueBoundaryKind {
    SetUpInsert,
    ActionHalt,
    RecursiveTrade,
    FinishInsert,
    ReleaseSavedList,
}

/// A typed full-state boundary owned by the economy containment host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueBoundaryReceipt {
    pub kind: QueueBoundaryKind,
    pub entry_va: u32,
    pub residual_va: u32,
    pub state_before_digest: u64,
    pub state_after_digest: u64,
    pub complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueFirstFacts {
    /// Concrete order indices cloned by `set_up_insert`, in traversal order.  Type 15 may
    /// not appear: retail's `copy_order` default arm returns null for TRADE_ROUTE.
    pub saved_order_indices: Vec<i32>,
    pub set_up_insert: QueueBoundaryReceipt,
    pub action_halt: QueueBoundaryReceipt,
    pub group_after_halt: TradeGroupSnapshot,
    pub recursive_trade: QueueBoundaryReceipt,
    pub recursive: Box<TradeInvocationFacts>,
    pub finish_insert: QueueBoundaryReceipt,
    pub release_saved_list: QueueBoundaryReceipt,
}

/// Facts for one actual invocation, including the recursive Queue-New invocation made by
/// Queue-First.  Counts are `Option` solely so the planner can prove call short-circuiting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TradeInvocationFacts {
    pub scenario: ScenarioPruneFacts,
    pub target: Option<TradeTargetGateFacts>,
    pub count_land: Option<i32>,
    pub count_sea: Option<i32>,
    pub direct: Option<DirectTradeFacts>,
    pub queue_first: Option<QueueFirstFacts>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupActionTradeFacts {
    pub evidence: RetailTradeEvidence,
    pub invocation: TradeInvocationFacts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeRefusal {
    TargetNotBuilding,
    TargetInactive,
    TargetNotTrade,
    GroupHasNoTrader,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeOutcome {
    Refused(TradeRefusal),
    Completed { installed: usize },
    QueueFirst { installed: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupAfterAuthority {
    /// No opaque Queue-First boundary can alter this projection.
    Exact,
    /// `finish_insert` can reissue heterogeneous saved orders; the containment host owns
    /// the final canonical Group image. `last_known` below is not a commit image.
    EconomyHost,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TradeGroupAfter {
    pub authority: GroupAfterAuthority,
    pub last_known: TradeGroupSnapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeMemberGate {
    NotUnit,
    OffMap,
    NotCaravan,
    NotSeaTrader,
    RegionsDoNotTouch,
}

/// Exact installer micro-chronology at `0x005E4DC0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeInstallStep {
    ClearActiveOrderMask,
    ClearPathAnchor,
    CloseOrders { arg: i32 },
    ClearPartialPath,
    UpdateActionBeforeInstall,
    ClearTradeRouteMask,
    AllocateOrder { index: i32, size: usize },
    StoreFirst(ObjectIdentity),
    StoreLoaded(i32),
    StoreSecond(ObjectIdentity),
    StoreStarted(i32),
    SetGroupFlag,
    ResolvePrimaryRegion(TerrainRegionReceipt),
    ResolveActorRegion(TerrainRegionReceipt),
    ReadTransportCapability { leader_flags: u32, capability: i32 },
    ReadTransportType(i32),
    ReadCanEverTransport(bool),
    SetTransportRouteMask,
    AppendOrder,
    UpdateActionAfterInstall,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TradeOrderInstallPlan {
    pub actor: ObjectIdentity,
    pub queue: i32,
    pub payload: TradeRouteOrderPayload,
    pub unit_flags_before: u32,
    pub unit_flags_after_projection: u32,
    pub orders_before_digest: u64,
    pub partial_path_before_digest: u64,
    pub action_before_digest: u64,
    pub steps: Vec<TradeInstallStep>,
}

/// Calls and mutations in one total instruction order.  Observation events are retained
/// because dropping one would permit a host to resolve facts from different world epochs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TradeActionEvent {
    ScenarioPrune(ScenarioPruneFacts),
    ActionBegin {
        disband_before: i32,
        disband_after: i32,
    },
    ResolvePrimary(ObjectIdentity),
    TargetBuildingGate(bool),
    TargetActiveGate(bool),
    TargetEntryTradeGate {
        receiver_digest: u64,
        accepted: bool,
    },
    CountGroup {
        index: i32,
        type_index: i32,
        result: i32,
    },
    QueueBoundary(QueueBoundaryReceipt),
    SetGroupForm {
        before: i32,
        after: i32,
    },
    ResolveTargetRegion(TerrainRegionReceipt),
    ResolveMember(ObjectIdentity),
    MemberUnitGate {
        actor: ObjectIdentity,
        accepted: bool,
    },
    MemberOnMapGate {
        actor: ObjectIdentity,
        accepted: bool,
    },
    ResolveMemberRegion {
        actor: ObjectIdentity,
        receipt: TerrainRegionReceipt,
    },
    TargetTradeGate {
        actor: ObjectIdentity,
        accepted: bool,
    },
    MemberCaravanGate {
        actor: ObjectIdentity,
        accepted: bool,
    },
    TargetSeaTradeGate {
        actor: ObjectIdentity,
        accepted: bool,
    },
    MemberSeaTypeGate {
        actor: ObjectIdentity,
        accepted: bool,
    },
    RegionTouchGate {
        actor: ObjectIdentity,
        accepted: bool,
    },
    SkipMember {
        actor: ObjectIdentity,
        reason: TradeMemberGate,
    },
    AddTradeOrder(TradeOrderInstallPlan),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupActionTradeTransaction {
    pub request: GroupActionTradeRequest,
    pub events: Vec<TradeActionEvent>,
    pub outcome: TradeOutcome,
    pub group_after: TradeGroupAfter,
    /// All normal arms converge here before the shared epilogue/`ret 0x14`.
    pub atomic_residual_va: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradePlanError {
    EvidenceMismatch,
    GroupShape,
    GroupSlotMismatch,
    ScenarioSchedule,
    IncompleteExternalBoundary,
    RetailCrashDomain,
    TargetIdentity,
    ShortCircuitShape,
    QueueBoundaryKind,
    QueueBoundaryAddress,
    QueueBoundaryChronology,
    QueueFirstCopiedTradeRoute,
    RecursiveGroupIdentity,
    MemberCount,
    MemberIdentity { index: usize },
    MemberFactShape { index: usize },
    InstallIdentity { index: usize },
    InstallFactShape { index: usize },
}

fn validate_scenario(
    entry: &TradeGroupSnapshot,
    scenario: &ScenarioPruneFacts,
) -> Result<(), TradePlanError> {
    if !entry.well_formed() || !scenario.group_after.well_formed() {
        return Err(TradePlanError::GroupShape);
    }
    if entry.group_slot != scenario.group_after.group_slot {
        return Err(TradePlanError::GroupSlotMismatch);
    }
    if !scenario.complete {
        return Err(TradePlanError::IncompleteExternalBoundary);
    }
    let expected: Vec<_> = if scenario.ignore_orders && entry.owner < 8 {
        scenario
            .ignored_objects
            .iter()
            .copied()
            .filter(|o| *o >= 0)
            .map(|o| ScenarioKillCall {
                o,
                who: entry.owner as i32,
                arg2: 0,
                arg3: 0,
            })
            .collect()
    } else {
        Vec::new()
    };
    if scenario.calls != expected {
        return Err(TradePlanError::ScenarioSchedule);
    }
    Ok(())
}

fn require_none_after_refusal(facts: &TradeInvocationFacts) -> Result<(), TradePlanError> {
    if facts.direct.is_some() || facts.queue_first.is_some() {
        Err(TradePlanError::ShortCircuitShape)
    } else {
        Ok(())
    }
}

fn capability(flags: u32) -> i32 {
    if flags & 0x100 != 0 {
        3
    } else if flags & 0x200 != 0 {
        2
    } else {
        ((flags >> 10) & 1) as i32
    }
}

fn plan_install(
    action: RawTradeAction,
    queue: i32,
    target_region: i32,
    member_region: i32,
    expected_actor: ObjectIdentity,
    facts: TradeInstallFacts,
    index: usize,
) -> Result<TradeOrderInstallPlan, TradePlanError> {
    if facts.actor != expected_actor
        || facts.actor_region.object.key != expected_actor.key
        || facts.actor_region.tregion != member_region
        || facts.primary_region.object.key
            != (ObjectKey {
                o: action.ox,
                who: action.whom,
            })
        || facts.primary_region.tregion != target_region
    {
        return Err(TradePlanError::InstallIdentity { index });
    }
    if !facts.primary_region.is_read_only_complete() || !facts.actor_region.is_read_only_complete()
    {
        return Err(TradePlanError::IncompleteExternalBoundary);
    }

    let first_key = ObjectKey {
        o: action.ox,
        who: action.whom,
    };
    let second_key = ObjectKey {
        o: action.oxx,
        who: action.whose,
    };
    let first = if action.ox >= 0 && action.whom >= 0 {
        let Some(identity) = facts.primary_lookup else {
            return Err(TradePlanError::InstallFactShape { index });
        };
        if identity.key != first_key {
            return Err(TradePlanError::InstallIdentity { index });
        }
        identity
    } else {
        if facts.primary_lookup.is_some() {
            return Err(TradePlanError::InstallFactShape { index });
        }
        ObjectIdentity {
            key: first_key,
            uid: u16::MAX,
        }
    };

    // Shipped asymmetry: the second lookup tests primary `whom`, not `whose`.
    let second = if action.oxx >= 0 && action.whom >= 0 {
        if action.whose < 0 {
            return Err(TradePlanError::RetailCrashDomain);
        }
        let Some(identity) = facts.secondary_lookup else {
            return Err(TradePlanError::InstallFactShape { index });
        };
        if identity.key != second_key {
            return Err(TradePlanError::InstallIdentity { index });
        }
        identity
    } else {
        if facts.secondary_lookup.is_some() {
            return Err(TradePlanError::InstallFactShape { index });
        }
        ObjectIdentity {
            key: second_key,
            uid: u16::MAX,
        }
    };

    let mut transport_steps = vec![
        TradeInstallStep::ResolvePrimaryRegion(facts.primary_region),
        TradeInstallStep::ResolveActorRegion(facts.actor_region),
    ];
    let mark_transport = if member_region != target_region {
        let (Some(flags), Some(transport_type)) = (facts.leader_flags, facts.transport_type) else {
            return Err(TradePlanError::InstallFactShape { index });
        };
        let derived_capability = capability(flags);
        transport_steps.extend([
            TradeInstallStep::ReadTransportCapability {
                leader_flags: flags,
                capability: derived_capability,
            },
            TradeInstallStep::ReadTransportType(transport_type),
        ]);
        if derived_capability >= transport_type {
            let answer = facts
                .can_ever_transport
                .ok_or(TradePlanError::InstallFactShape { index })?;
            transport_steps.push(TradeInstallStep::ReadCanEverTransport(answer));
            answer
        } else {
            if facts.can_ever_transport.is_some() {
                return Err(TradePlanError::InstallFactShape { index });
            }
            false
        }
    } else {
        if facts.leader_flags.is_some()
            || facts.transport_type.is_some()
            || facts.can_ever_transport.is_some()
        {
            return Err(TradePlanError::InstallFactShape { index });
        }
        false
    };

    let payload = TradeRouteOrderPayload {
        first,
        second,
        started: 0,
        loaded: 0,
        flags: TRADE_GROUP_FLAG,
    };
    let mut flags_after = facts.unit_flags_before;
    let mut steps = Vec::new();
    if queue == QUEUE_NEW {
        flags_after &= !UNIT_ACTIVE_ORDER_MASK;
        steps.extend([
            TradeInstallStep::ClearActiveOrderMask,
            TradeInstallStep::ClearPathAnchor,
            TradeInstallStep::CloseOrders { arg: 0 },
            TradeInstallStep::ClearPartialPath,
            TradeInstallStep::UpdateActionBeforeInstall,
        ]);
    }
    flags_after &= !UNIT_HAS_TRADE_ROUTE_MASK;
    steps.extend([
        TradeInstallStep::ClearTradeRouteMask,
        TradeInstallStep::AllocateOrder {
            index: TRADE_ROUTE_ORDER_INDEX,
            size: TRADE_ROUTE_ORDER_SIZE,
        },
        TradeInstallStep::StoreFirst(first),
        TradeInstallStep::StoreLoaded(0),
        TradeInstallStep::StoreSecond(second),
        TradeInstallStep::StoreStarted(0),
        TradeInstallStep::SetGroupFlag,
    ]);
    steps.append(&mut transport_steps);
    if mark_transport {
        flags_after |= UNIT_TRANSPORT_ROUTE_MASK;
        steps.push(TradeInstallStep::SetTransportRouteMask);
    }
    steps.extend([
        TradeInstallStep::AppendOrder,
        TradeInstallStep::UpdateActionAfterInstall,
    ]);

    Ok(TradeOrderInstallPlan {
        actor: facts.actor,
        queue,
        payload,
        unit_flags_before: facts.unit_flags_before,
        unit_flags_after_projection: flags_after,
        orders_before_digest: facts.orders_before_digest,
        partial_path_before_digest: facts.partial_path_before_digest,
        action_before_digest: facts.action_before_digest,
        steps,
    })
}

fn no_member_tail(member: &TradeMemberFacts) -> bool {
    member.region.is_none()
        && member.target_is_trade.is_none()
        && member.is_caravan.is_none()
        && member.target_is_sea_trade.is_none()
        && member.is_sea_trade_member.is_none()
        && member.regions_touch.is_none()
        && member.install.is_none()
}

fn plan_direct(
    action: RawTradeAction,
    group: &mut TradeGroupSnapshot,
    facts: &DirectTradeFacts,
    events: &mut Vec<TradeActionEvent>,
) -> Result<usize, TradePlanError> {
    let old_form = group.form;
    group.form = -1;
    events.push(TradeActionEvent::SetGroupForm {
        before: old_form,
        after: -1,
    });
    if facts.target_region.object.key
        != (ObjectKey {
            o: action.ox,
            who: action.whom,
        })
    {
        return Err(TradePlanError::TargetIdentity);
    }
    if !facts.target_region.is_read_only_complete() {
        return Err(TradePlanError::IncompleteExternalBoundary);
    }
    events.push(TradeActionEvent::ResolveTargetRegion(facts.target_region));

    if facts.members.len() != group.members.len() {
        return Err(TradePlanError::MemberCount);
    }
    let queue = if action.queued == QUEUE_LAST {
        QUEUE_LAST
    } else {
        QUEUE_NEW
    };
    let mut installed = 0usize;
    for (index, member) in facts.members.iter().enumerate() {
        if member.identity.key.o != group.members[index] as i32
            || member.identity.key.who != group.owner as i32
        {
            return Err(TradePlanError::MemberIdentity { index });
        }
        events.push(TradeActionEvent::ResolveMember(member.identity));
        events.push(TradeActionEvent::MemberUnitGate {
            actor: member.identity,
            accepted: member.live_unit,
        });
        if !member.live_unit {
            if member.on_map.is_some() || !no_member_tail(member) {
                return Err(TradePlanError::MemberFactShape { index });
            }
            events.push(TradeActionEvent::SkipMember {
                actor: member.identity,
                reason: TradeMemberGate::NotUnit,
            });
            continue;
        }
        let on_map = member
            .on_map
            .ok_or(TradePlanError::MemberFactShape { index })?;
        events.push(TradeActionEvent::MemberOnMapGate {
            actor: member.identity,
            accepted: on_map,
        });
        if !on_map {
            if !no_member_tail(member) {
                return Err(TradePlanError::MemberFactShape { index });
            }
            events.push(TradeActionEvent::SkipMember {
                actor: member.identity,
                reason: TradeMemberGate::OffMap,
            });
            continue;
        }

        let region = member
            .region
            .ok_or(TradePlanError::MemberFactShape { index })?;
        if region.object.key != member.identity.key {
            return Err(TradePlanError::MemberIdentity { index });
        }
        if !region.is_read_only_complete() {
            return Err(TradePlanError::IncompleteExternalBoundary);
        }
        events.push(TradeActionEvent::ResolveMemberRegion {
            actor: member.identity,
            receipt: region,
        });
        let target_is_trade = member
            .target_is_trade
            .ok_or(TradePlanError::MemberFactShape { index })?;
        events.push(TradeActionEvent::TargetTradeGate {
            actor: member.identity,
            accepted: target_is_trade,
        });

        let accepted = if target_is_trade {
            if member.target_is_sea_trade.is_some()
                || member.is_sea_trade_member.is_some()
                || member.regions_touch.is_some()
            {
                return Err(TradePlanError::MemberFactShape { index });
            }
            let caravan = member
                .is_caravan
                .ok_or(TradePlanError::MemberFactShape { index })?;
            events.push(TradeActionEvent::MemberCaravanGate {
                actor: member.identity,
                accepted: caravan,
            });
            if !caravan {
                events.push(TradeActionEvent::SkipMember {
                    actor: member.identity,
                    reason: TradeMemberGate::NotCaravan,
                });
            }
            caravan
        } else {
            if member.is_caravan.is_some() {
                return Err(TradePlanError::MemberFactShape { index });
            }
            let sea = member
                .target_is_sea_trade
                .ok_or(TradePlanError::MemberFactShape { index })?;
            events.push(TradeActionEvent::TargetSeaTradeGate {
                actor: member.identity,
                accepted: sea,
            });
            if !sea {
                if member.is_sea_trade_member.is_some() || member.regions_touch.is_some() {
                    return Err(TradePlanError::MemberFactShape { index });
                }
                true
            } else {
                let sea_member = member
                    .is_sea_trade_member
                    .ok_or(TradePlanError::MemberFactShape { index })?;
                events.push(TradeActionEvent::MemberSeaTypeGate {
                    actor: member.identity,
                    accepted: sea_member,
                });
                if !sea_member {
                    if member.regions_touch.is_some() {
                        return Err(TradePlanError::MemberFactShape { index });
                    }
                    events.push(TradeActionEvent::SkipMember {
                        actor: member.identity,
                        reason: TradeMemberGate::NotSeaTrader,
                    });
                    false
                } else {
                    let touch = member
                        .regions_touch
                        .ok_or(TradePlanError::MemberFactShape { index })?;
                    events.push(TradeActionEvent::RegionTouchGate {
                        actor: member.identity,
                        accepted: touch,
                    });
                    if !touch {
                        events.push(TradeActionEvent::SkipMember {
                            actor: member.identity,
                            reason: TradeMemberGate::RegionsDoNotTouch,
                        });
                    }
                    touch
                }
            }
        };

        if accepted {
            let install = member
                .install
                .ok_or(TradePlanError::MemberFactShape { index })?;
            let plan = plan_install(
                action,
                queue,
                facts.target_region.tregion,
                region.tregion,
                member.identity,
                install,
                index,
            )?;
            events.push(TradeActionEvent::AddTradeOrder(plan));
            installed += 1;
        } else if member.install.is_some() {
            return Err(TradePlanError::MemberFactShape { index });
        }
    }
    Ok(installed)
}

fn validate_boundary(
    receipt: QueueBoundaryReceipt,
    kind: QueueBoundaryKind,
    entry: u32,
    residual: u32,
) -> Result<(), TradePlanError> {
    if !receipt.complete {
        return Err(TradePlanError::IncompleteExternalBoundary);
    }
    if receipt.kind != kind {
        return Err(TradePlanError::QueueBoundaryKind);
    }
    if receipt.entry_va != entry || receipt.residual_va != residual {
        return Err(TradePlanError::QueueBoundaryAddress);
    }
    Ok(())
}

fn plan_invocation(
    action: RawTradeAction,
    entry_group: &TradeGroupSnapshot,
    facts: &TradeInvocationFacts,
    events: &mut Vec<TradeActionEvent>,
    allow_queue_first: bool,
) -> Result<(TradeOutcome, TradeGroupAfter), TradePlanError> {
    validate_scenario(entry_group, &facts.scenario)?;
    events.push(TradeActionEvent::ScenarioPrune(facts.scenario.clone()));
    let mut group = facts.scenario.group_after.clone();
    let before_disband = group.disband;
    group.disband = 0;
    events.push(TradeActionEvent::ActionBegin {
        disband_before: before_disband,
        disband_after: 0,
    });

    // The initial registry lookup is unconditional and has no safe null/bounds gate.
    if action.ox < 0 || action.whom < 0 {
        return Err(TradePlanError::RetailCrashDomain);
    }
    let target = facts.target.ok_or(TradePlanError::RetailCrashDomain)?;
    if target.identity.key
        != (ObjectKey {
            o: action.ox,
            who: action.whom,
        })
    {
        return Err(TradePlanError::TargetIdentity);
    }
    events.push(TradeActionEvent::ResolvePrimary(target.identity));
    events.push(TradeActionEvent::TargetBuildingGate(target.live_building));
    if !target.live_building {
        if target.active.is_some()
            || target.entry_receiver_is_trade.is_some()
            || target.entry_receiver_digest.is_some()
            || facts.count_land.is_some()
            || facts.count_sea.is_some()
        {
            return Err(TradePlanError::ShortCircuitShape);
        }
        require_none_after_refusal(facts)?;
        return Ok((
            TradeOutcome::Refused(TradeRefusal::TargetNotBuilding),
            TradeGroupAfter {
                authority: GroupAfterAuthority::Exact,
                last_known: group,
            },
        ));
    }

    let active = target.active.ok_or(TradePlanError::ShortCircuitShape)?;
    events.push(TradeActionEvent::TargetActiveGate(active));
    if !active {
        if target.entry_receiver_is_trade.is_some()
            || target.entry_receiver_digest.is_some()
            || facts.count_land.is_some()
            || facts.count_sea.is_some()
        {
            return Err(TradePlanError::ShortCircuitShape);
        }
        require_none_after_refusal(facts)?;
        return Ok((
            TradeOutcome::Refused(TradeRefusal::TargetInactive),
            TradeGroupAfter {
                authority: GroupAfterAuthority::Exact,
                last_known: group,
            },
        ));
    }

    let entry_trade = target
        .entry_receiver_is_trade
        .ok_or(TradePlanError::ShortCircuitShape)?;
    let receiver_digest = target
        .entry_receiver_digest
        .ok_or(TradePlanError::ShortCircuitShape)?;
    events.push(TradeActionEvent::TargetEntryTradeGate {
        receiver_digest,
        accepted: entry_trade,
    });
    if !entry_trade {
        if facts.count_land.is_some() || facts.count_sea.is_some() {
            return Err(TradePlanError::ShortCircuitShape);
        }
        require_none_after_refusal(facts)?;
        return Ok((
            TradeOutcome::Refused(TradeRefusal::TargetNotTrade),
            TradeGroupAfter {
                authority: GroupAfterAuthority::Exact,
                last_known: group,
            },
        ));
    }

    let count_land = facts.count_land.ok_or(TradePlanError::ShortCircuitShape)?;
    events.push(TradeActionEvent::CountGroup {
        index: TRADE_COUNT_INDEX,
        type_index: TRADE_COUNT_LAND_TYPE,
        result: count_land,
    });
    let has_trader = if count_land != 0 {
        if facts.count_sea.is_some() {
            return Err(TradePlanError::ShortCircuitShape);
        }
        true
    } else {
        let count_sea = facts.count_sea.ok_or(TradePlanError::ShortCircuitShape)?;
        events.push(TradeActionEvent::CountGroup {
            index: TRADE_COUNT_INDEX,
            type_index: TRADE_COUNT_SEA_TYPE,
            result: count_sea,
        });
        count_sea != 0
    };
    if !has_trader {
        require_none_after_refusal(facts)?;
        return Ok((
            TradeOutcome::Refused(TradeRefusal::GroupHasNoTrader),
            TradeGroupAfter {
                authority: GroupAfterAuthority::Exact,
                last_known: group,
            },
        ));
    }

    if action.queued != QUEUE_FIRST {
        if facts.queue_first.is_some() {
            return Err(TradePlanError::ShortCircuitShape);
        }
        let direct = facts
            .direct
            .as_ref()
            .ok_or(TradePlanError::ShortCircuitShape)?;
        let installed = plan_direct(action, &mut group, direct, events)?;
        return Ok((
            TradeOutcome::Completed { installed },
            TradeGroupAfter {
                authority: GroupAfterAuthority::Exact,
                last_known: group,
            },
        ));
    }

    if !allow_queue_first || facts.direct.is_some() {
        return Err(TradePlanError::ShortCircuitShape);
    }
    let queue = facts
        .queue_first
        .as_ref()
        .ok_or(TradePlanError::ShortCircuitShape)?;
    if queue.saved_order_indices.contains(&TRADE_ROUTE_ORDER_INDEX) {
        return Err(TradePlanError::QueueFirstCopiedTradeRoute);
    }
    validate_boundary(
        queue.set_up_insert,
        QueueBoundaryKind::SetUpInsert,
        GROUP_SET_UP_INSERT_VA,
        GROUP_SET_UP_INSERT_VA + 0x100,
    )?;
    validate_boundary(
        queue.action_halt,
        QueueBoundaryKind::ActionHalt,
        GROUP_ACTION_HALT_VA,
        GROUP_ACTION_HALT_VA + 685,
    )?;
    validate_boundary(
        queue.recursive_trade,
        QueueBoundaryKind::RecursiveTrade,
        GROUP_ACTION_TRADE_VA,
        GROUP_ACTION_TRADE_END_VA,
    )?;
    validate_boundary(
        queue.finish_insert,
        QueueBoundaryKind::FinishInsert,
        GROUP_FINISH_INSERT_VA,
        GROUP_FINISH_INSERT_VA + 1_104,
    )?;
    validate_boundary(
        queue.release_saved_list,
        QueueBoundaryKind::ReleaseSavedList,
        ORDER_LIST_CLEAR_VA,
        ORDER_LIST_CLEAR_VA + 236,
    )?;
    let boundaries = [
        queue.set_up_insert,
        queue.action_halt,
        queue.recursive_trade,
        queue.finish_insert,
        queue.release_saved_list,
    ];
    if boundaries
        .windows(2)
        .any(|pair| pair[0].state_after_digest != pair[1].state_before_digest)
    {
        return Err(TradePlanError::QueueBoundaryChronology);
    }
    if queue.group_after_halt.group_slot != group.group_slot
        || !queue.group_after_halt.well_formed()
    {
        return Err(TradePlanError::RecursiveGroupIdentity);
    }

    events.push(TradeActionEvent::QueueBoundary(queue.set_up_insert));
    events.push(TradeActionEvent::QueueBoundary(queue.action_halt));
    events.push(TradeActionEvent::QueueBoundary(queue.recursive_trade));
    let recursive_action = RawTradeAction {
        queued: QUEUE_NEW,
        ..action
    };
    let (recursive_outcome, recursive_group) = plan_invocation(
        recursive_action,
        &queue.group_after_halt,
        &queue.recursive,
        events,
        false,
    )?;
    let installed = match recursive_outcome {
        TradeOutcome::Completed { installed } => installed,
        TradeOutcome::Refused(_) => 0,
        TradeOutcome::QueueFirst { .. } => return Err(TradePlanError::ShortCircuitShape),
    };
    events.push(TradeActionEvent::QueueBoundary(queue.finish_insert));
    events.push(TradeActionEvent::QueueBoundary(queue.release_saved_list));
    Ok((
        TradeOutcome::QueueFirst { installed },
        TradeGroupAfter {
            authority: GroupAfterAuthority::EconomyHost,
            last_known: recursive_group.last_known,
        },
    ))
}

/// Produce the one complete action transaction.  Every error is pre-commit; the input and
/// caller state remain untouched.  The returned event vector must be committed as a unit.
pub fn plan_group_action_trade(
    request: GroupActionTradeRequest,
    facts: GroupActionTradeFacts,
) -> Result<GroupActionTradeTransaction, TradePlanError> {
    if !facts.evidence.is_shipped() {
        return Err(TradePlanError::EvidenceMismatch);
    }
    if !request.group.well_formed() {
        return Err(TradePlanError::GroupShape);
    }
    let mut events = Vec::new();
    let (outcome, group_after) = plan_invocation(
        request.action,
        &request.group,
        &facts.invocation,
        &mut events,
        true,
    )?;
    Ok(GroupActionTradeTransaction {
        request,
        events,
        outcome,
        group_after,
        atomic_residual_va: 0x0070_1e8c,
    })
}
