// SPDX-License-Identifier: GPL-3.0-or-later
//! Source-only contract for `Group::set_up_insert` / `Group::finish_insert`.
//!
//! This module is deliberately not registered in `systems/mod.rs`.  It freezes the
//! instruction-observable list chronology and the exact Gather/Cast/Trade replay boundary
//! without becoming a second production order queue.

pub const GROUP_SET_UP_INSERT_VA: u32 = 0x0070_e520;
pub const GROUP_SET_UP_INSERT_BYTES: usize = 0x100;
pub const GROUP_FINISH_INSERT_VA: u32 = 0x0070_e620;
pub const GROUP_FINISH_INSERT_BYTES: usize = 0x450;
pub const COPY_ORDER_VA: u32 = 0x0072_f900;
pub const COPY_ORDER_BYTES: usize = 0x41c;
pub const LINK_LIST_ADD_VA: u32 = 0x0046_d5a0;
pub const LINK_LIST_REMOVE_VA: u32 = 0x0046_d620;

pub const GROUP_FIND_LEADER_CALL_VA: u32 = 0x0070_e52a;
pub const GROUP_FLAG_TEST_VA: u32 = 0x0070_e5a3;
pub const COPY_ORDER_CALL_VA: u32 = 0x0070_e5a9;
pub const SAVED_LIST_ADD_CALL_VA: u32 = 0x0070_e5b2;
pub const SOURCE_NEXT_LINK_VA: u32 = 0x0070_e5da;
pub const SOURCE_HEAD_TEST_VA: u32 = 0x0070_e607;

pub const FINISH_RESET_TO_HEAD_VA: u32 = 0x0070_e62e;
pub const FINISH_REMOVE_BEFORE_DISPATCH_VA: u32 = 0x0070_e657;
pub const FINISH_KIND_DISPATCH_VA: u32 = 0x0070_e664;
pub const FINISH_RECYCLE_VA: u32 = 0x0070_e9e8;

pub const COPY_GATHER_ARM_VA: u32 = 0x0072_fa56;
pub const COPY_CAST_ARM_VA: u32 = 0x0072_fb64;
pub const COPY_NULL_ARM_VA: u32 = 0x0072_fca2;
pub const FINISH_GATHER_ARM_VA: u32 = 0x0070_e7f2;
pub const FINISH_TRADE_ARM_VA: u32 = 0x0070_e8c3;
pub const FINISH_CAST_ARM_VA: u32 = 0x0070_e8e7;
pub const GROUP_ACTION_GATHER_VA: u32 = 0x0070_0b90;
pub const GROUP_ACTION_TRADE_VA: u32 = 0x0070_1cc0;
pub const GROUP_ACTION_SPELL_VA: u32 = 0x006f_e1a0;

pub const QUEUE_FIRST: i32 = 0;
pub const QUEUE_LAST: i32 = 1;
pub const QUEUE_NEW: i32 = 2;
pub const ORDER_GROUP: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum OrderKind {
    None = 0,
    MoveTo = 1,
    AttackTo = 2,
    ExploreTo = 3,
    FleeTo = 4,
    Patrol = 5,
    BuildAt = 6,
    Gather = 7,
    BoardShip = 8,
    AwaitBoard = 9,
    Attack = 10,
    Follow = 11,
    Guard = 12,
    Repair = 13,
    CastSpell = 14,
    TradeRoute = 15,
    Strafe = 16,
    AirPatrol = 17,
    ChangeForm = 18,
    GroupMove = 19,
    GroupAttack = 20,
    GroupAttackTo = 21,
    GroupPatrol = 22,
    AttackGround = 23,
    AirAttackGround = 24,
    SpecialAnim = 25,
    Garrison = 26,
    Think = 27,
}

impl OrderKind {
    pub const ALL: [Self; 28] = [
        Self::None,
        Self::MoveTo,
        Self::AttackTo,
        Self::ExploreTo,
        Self::FleeTo,
        Self::Patrol,
        Self::BuildAt,
        Self::Gather,
        Self::BoardShip,
        Self::AwaitBoard,
        Self::Attack,
        Self::Follow,
        Self::Guard,
        Self::Repair,
        Self::CastSpell,
        Self::TradeRoute,
        Self::Strafe,
        Self::AirPatrol,
        Self::ChangeForm,
        Self::GroupMove,
        Self::GroupAttack,
        Self::GroupAttackTo,
        Self::GroupPatrol,
        Self::AttackGround,
        Self::AirAttackGround,
        Self::SpecialAnim,
        Self::Garrison,
        Self::Think,
    ];

    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// Jump-table destinations for kinds 1..27 in `copy_order`.
///
/// Index zero is unused. Kinds 5, 15, and 17 share the null/default arm.  The table is
/// kept explicit because `finish_insert` having a Trade arm does not make Trade copyable.
pub const COPY_ORDER_CASE_VA: [u32; 28] = [
    0,
    0x0072_f92e,
    0x0072_f94d,
    0x0072_f96c,
    0x0072_f98b,
    COPY_NULL_ARM_VA,
    0x0072_fa37,
    COPY_GATHER_ARM_VA,
    0x0072_fa75,
    0x0072_fa94,
    0x0072_faaa,
    0x0072_fb07,
    0x0072_fb26,
    0x0072_fb45,
    COPY_CAST_ARM_VA,
    COPY_NULL_ARM_VA,
    0x0072_fb83,
    COPY_NULL_ARM_VA,
    0x0072_fba8,
    0x0072_fbcd,
    0x0072_fbf2,
    0x0072_fc17,
    0x0072_f9aa,
    0x0072_fac9,
    0x0072_fae8,
    0x0072_fc3c,
    0x0072_fc61,
    0x0072_fc86,
];

/// Jump-table destinations for kinds 1..26 in `finish_insert`.
///
/// `FINISH_RECYCLE_VA` means the cloned node is recycled without a Group action. Kind 27
/// is outside the dispatch range and reaches the same recycle path.
pub const FINISH_INSERT_CASE_VA: [u32; 28] = [
    FINISH_RECYCLE_VA,
    0x0070_e67c,
    0x0070_e6d7,
    0x0070_e719,
    0x0070_e75e,
    FINISH_RECYCLE_VA,
    0x0070_e7bb,
    FINISH_GATHER_ARM_VA,
    0x0070_e80a,
    FINISH_RECYCLE_VA,
    0x0070_e822,
    0x0070_e85f,
    0x0070_e87a,
    0x0070_e897,
    FINISH_CAST_ARM_VA,
    FINISH_TRADE_ARM_VA,
    FINISH_RECYCLE_VA,
    FINISH_RECYCLE_VA,
    0x0070_e909,
    0x0070_e929,
    FINISH_RECYCLE_VA,
    0x0070_e988,
    0x0070_e7a3,
    0x0070_e844,
    FINISH_RECYCLE_VA,
    FINISH_RECYCLE_VA,
    0x0070_e9cd,
    FINISH_RECYCLE_VA,
];

#[inline]
pub const fn copy_order_returns_clone(kind: OrderKind) -> bool {
    kind.index() != 0 && COPY_ORDER_CASE_VA[kind.index()] != COPY_NULL_ARM_VA
}

#[inline]
pub const fn finish_insert_has_action(kind: OrderKind) -> bool {
    FINISH_INSERT_CASE_VA[kind.index()] != FINISH_RECYCLE_VA
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GatherHistory {
    pub tx: i32,
    pub ty: i32,
    pub build_type: i32,
    pub wait: i32,
    pub goto_build: u8,
    pub non_flat_gather: u8,
    pub dist_mod: u8,
    pub been_there: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinishPayload {
    Gather {
        target_o: i32,
        target_who: i32,
        target_uid: u16,
        history: GatherHistory,
    },
    Cast {
        spell: i32,
        target_o: i32,
        target_who: i32,
        x: i32,
        y: i32,
        paid: i32,
    },
    /// The contract intentionally does not model the other action argument layouts. A
    /// caller that encounters one must hand it to the matching source-owned action cohort.
    OtherSourceOwned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderOrderSnapshot {
    /// Stable test/receipt identity; it is not a retail field.
    pub token: u32,
    pub kind: OrderKind,
    pub flags: u8,
    /// `copy_order` calls the concrete assignment operator. This bit attests that the
    /// canonical queue retained every field that operator reads.
    pub clone_image_complete: bool,
    pub finish_payload: Option<FinishPayload>,
}

impl LeaderOrderSnapshot {
    #[inline]
    pub const fn is_group(self) -> bool {
        self.flags & ORDER_GROUP != 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueFirstRequest {
    /// Exact `Group::find_leader(0)` result is addressable.
    pub leader_found: bool,
    /// The slice contains the complete canonical leader list in execution order
    /// (`head_node->prev`, then `prev`, front to tail).
    pub leader_queue_complete: bool,
    /// The circular list head/next/prev relationship was validated. Without it the
    /// reverse physical traversal is an assumption, not a source fact.
    pub physical_links_complete: bool,
    /// The action recursively issued this order through `QUEUE_NEW` before finish replay.
    pub new_order_token: u32,
    pub leader_execution_order: Vec<LeaderOrderSnapshot>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReissuedAction {
    /// `0x0070E7F2..0x0070E805`: only `ox` and literal queue 1 are forwarded. Target owner,
    /// UID, and the complete Gather suffix are re-derived/reset by `action_gather`.
    Gather {
        source_token: u32,
        target_o: i32,
        queue_raw: i32,
        discarded_target_who: i32,
        discarded_target_uid: u16,
        discarded_history: GatherHistory,
    },
    /// `0x0070E8E7..0x0070E904`: `action_spell(spell,ox,whom,x,y)` has no queue argument.
    /// `paid` is not forwarded; the reissued Cast order starts a new payment history.
    Cast {
        source_token: u32,
        spell: i32,
        target_o: i32,
        target_who: i32,
        x: i32,
        y: i32,
        discarded_paid: i32,
    },
}

impl ReissuedAction {
    #[inline]
    pub const fn source_token(self) -> u32 {
        match self {
            Self::Gather { source_token, .. } | Self::Cast { source_token, .. } => source_token,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContractStage {
    FindLeader,
    VisitSourceHeadThenNext,
    HaltAndIssueNew,
    RemoveSavedHeadBeforeDispatch,
    ReissueAction,
    RecycleClone,
}

pub const CONTRACT_CHRONOLOGY: [ContractStage; 6] = [
    ContractStage::FindLeader,
    ContractStage::VisitSourceHeadThenNext,
    ContractStage::HaltAndIssueNew,
    ContractStage::RemoveSavedHeadBeforeDispatch,
    ContractStage::ReissueAction,
    ContractStage::RecycleClone,
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueFirstPlan {
    /// Every source node in physical head/next order, including non-group nodes.
    pub set_up_visit_tokens: Vec<u32>,
    /// Saved nodes removed head-first by `finish_insert`. `None` is a known null clone,
    /// notably a grouped TRADE_ROUTE.
    pub finish_slots: Vec<Option<u32>>,
    /// Action calls in native finish order (old tail to old front).
    pub reissued_actions: Vec<ReissuedAction>,
    /// Observable execution order after each reissued order is installed at raw queue 1.
    pub final_execution_tokens: Vec<u32>,
    pub dropped_non_group_tokens: Vec<u32>,
    pub dropped_noncopyable_tokens: Vec<u32>,
    pub recycled_without_action_tokens: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueFirstContractError {
    MissingLeader,
    IncompleteLeaderQueue,
    IncompletePhysicalLinks,
    IncompleteCloneImage { token: u32, kind: OrderKind },
    MissingFinishPayload { token: u32, kind: OrderKind },
    FinishPayloadKindMismatch { token: u32, kind: OrderKind },
    SourceOwnedFinishPayloadRequired { token: u32, kind: OrderKind },
}

fn exact_reissue(order: LeaderOrderSnapshot) -> Result<ReissuedAction, QueueFirstContractError> {
    match (order.kind, order.finish_payload) {
        (
            OrderKind::Gather,
            Some(FinishPayload::Gather {
                target_o,
                target_who,
                target_uid,
                history,
            }),
        ) => Ok(ReissuedAction::Gather {
            source_token: order.token,
            target_o,
            queue_raw: QUEUE_LAST,
            discarded_target_who: target_who,
            discarded_target_uid: target_uid,
            discarded_history: history,
        }),
        (
            OrderKind::CastSpell,
            Some(FinishPayload::Cast {
                spell,
                target_o,
                target_who,
                x,
                y,
                paid,
            }),
        ) => Ok(ReissuedAction::Cast {
            source_token: order.token,
            spell,
            target_o,
            target_who,
            x,
            y,
            discarded_paid: paid,
        }),
        (OrderKind::Gather | OrderKind::CastSpell, None) => {
            Err(QueueFirstContractError::MissingFinishPayload {
                token: order.token,
                kind: order.kind,
            })
        }
        (OrderKind::Gather | OrderKind::CastSpell, Some(_)) => {
            Err(QueueFirstContractError::FinishPayloadKindMismatch {
                token: order.token,
                kind: order.kind,
            })
        }
        (_, Some(FinishPayload::OtherSourceOwned)) => {
            Err(QueueFirstContractError::SourceOwnedFinishPayloadRequired {
                token: order.token,
                kind: order.kind,
            })
        }
        (_, _) => Err(QueueFirstContractError::MissingFinishPayload {
            token: order.token,
            kind: order.kind,
        }),
    }
}

/// Derive the exact Gather/Cast/Trade `QUEUE_FIRST` transaction without mutating a queue.
///
/// The function preflights the whole leader list before returning a plan. An unsupported
/// copyable/action-bearing kind or missing concrete history therefore refuses atomically.
pub fn plan_queue_first(
    request: &QueueFirstRequest,
) -> Result<QueueFirstPlan, QueueFirstContractError> {
    if !request.leader_found {
        return Err(QueueFirstContractError::MissingLeader);
    }
    if !request.leader_queue_complete {
        return Err(QueueFirstContractError::IncompleteLeaderQueue);
    }
    if !request.physical_links_complete {
        return Err(QueueFirstContractError::IncompletePhysicalLinks);
    }

    let set_up_visit_tokens = request
        .leader_execution_order
        .iter()
        .rev()
        .map(|order| order.token)
        .collect();
    let dropped_non_group_tokens = request
        .leader_execution_order
        .iter()
        .filter(|order| !order.is_group())
        .map(|order| order.token)
        .collect();

    let mut finish_slots = Vec::new();
    let mut reissued_actions = Vec::new();
    let mut dropped_noncopyable_tokens = Vec::new();
    let mut recycled_without_action_tokens = Vec::new();

    // Source execution order is head.prev -> prev (front to tail). set_up_insert starts at
    // head and follows next, hence this reverse walk. finish_insert resets to the saved
    // head and removes before dispatch, preserving the same tail-to-front visit order.
    for order in request
        .leader_execution_order
        .iter()
        .rev()
        .copied()
        .filter(|order| order.is_group())
    {
        if !copy_order_returns_clone(order.kind) {
            finish_slots.push(None);
            dropped_noncopyable_tokens.push(order.token);
            continue;
        }
        if !order.clone_image_complete {
            return Err(QueueFirstContractError::IncompleteCloneImage {
                token: order.token,
                kind: order.kind,
            });
        }
        finish_slots.push(Some(order.token));
        if !finish_insert_has_action(order.kind) {
            recycled_without_action_tokens.push(order.token);
            continue;
        }
        reissued_actions.push(exact_reissue(order)?);
    }

    // The recursive action first installs N through QUEUE_NEW. Each finish action then
    // reaches the ordinary list add shape used by raw queue 1, which becomes the execution
    // front. Replaying old tail-to-front therefore reconstructs old front-to-tail before N.
    let mut final_execution_tokens = vec![request.new_order_token];
    for action in &reissued_actions {
        final_execution_tokens.insert(0, action.source_token());
    }

    Ok(QueueFirstPlan {
        set_up_visit_tokens,
        finish_slots,
        reissued_actions,
        final_execution_tokens,
        dropped_non_group_tokens,
        dropped_noncopyable_tokens,
        recycled_without_action_tokens,
    })
}
