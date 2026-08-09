// SPDX-License-Identifier: GPL-3.0-or-later
//! Source-only transaction frontier for `Group::action_recall` `0x006FA7E0`.
//!
//! The ordinary airbase/carrier arm is recovered through every branch and emitted host
//! effect.  A selected air-domain leader delegates to `Group::action_return`; that separate
//! 1,307-byte receiver remains an explicit, non-applicable tail.  Consequently this module
//! is planner evidence, not a `Port::Complete` claim.

use crate::systems::groups_guys::{GroupData, GROUP_MAX_MEMBERS};

pub const RECALL_OPCODE: u8 = 35;
pub const RECALL_WIRE_SIZE: usize = 1;
pub const GROUP_ACTION_RECALL_VA: u32 = 0x006f_a7e0;
pub const GROUP_ACTION_RECALL_BYTES: usize = 1_373;
pub const GROUP_ACTION_RETURN_VA: u32 = 0x006f_ad40;
pub const GROUP_ACTION_RETURN_BYTES: usize = 1_307;
pub const GROUP_FIND_LEADER_VA: u32 = 0x0070_ccb0;
pub const GROUP_MEMBER_VA: u32 = 0x0070_f8f0;
pub const UNIT_ADD_STRAFE_ORDER_VA: u32 = 0x005e_48c0;
pub const ORDER_SPECIAL_ANIM: i32 = 25;
pub const QUEUE_NEW: i32 = 2;
pub const AIR_DOMAIN: i32 = 2;
pub const HELICOPTER_TYPE_FLAG: u32 = 0x20;
pub const MISSILE_OBJECT_MASK: u32 = 0x0800_0000;
pub const UNIT_MASK_RECALL_CLEAR: u32 = 0x0400_0000;

#[inline]
pub fn decode_recall(wire: &[u8]) -> bool {
    wire == [RECALL_OPCODE]
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecallRequest {
    pub group: GroupData,
    /// Snapshot of scenario global `ignore_orders` (`0x00CC02F8`).
    pub ignore_orders: bool,
    /// The owner's scenario selection array at `0x00ED6580`, in retail iteration order.
    /// Negative entries are walked but do not call the group `kill` virtual.
    pub scenario_selection: Vec<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecallLeaderSource {
    /// `GroupData::buildings != 0`: sign-extended `list[0]`.
    FirstMember,
    /// `GroupData::find_leader(nullptr)`.
    FindLeaderZero,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecallLeaderFacts {
    pub source: RecallLeaderSource,
    pub o: i32,
    /// Read only when `o >= 0`; `2` is the shipped AIR domain.
    pub domain: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecallGroupMemberFacts {
    /// Identity sentinel; must equal the corresponding live `GroupData::list` word.
    pub o: i16,
    /// Object virtual `+0x0C` (`is_valid_wall`).
    pub valid_wall: bool,
    /// Object virtual `+0x20` (`is_build`), read only after `valid_wall` succeeds.
    pub is_build: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecallObjectRef {
    pub o: i32,
    pub who: i32,
    /// `GroupData::member(..., 1)` tests Object byte `+0x08 & 1` before scanning the list.
    pub active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecallLaunchingFacts {
    Null,
    Present { contains_actor_slot: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecallInsideFacts {
    /// The signed pair produced by `ObjectData::get_inside(int*)`.
    pub target: RecallObjectRef,
    /// Read only when the target passes `GroupData::member(..., 1)`.
    pub launching: Option<RecallLaunchingFacts>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecallAirResolution {
    /// `Unit::update_order()` followed by `UnitOrder::get_air_order()`.
    UpdateOrder {
        head_present: bool,
        /// Read only when `head_present`; this arm requires a value other than SPECIAL_ANIM.
        first_order_type: Option<i32>,
    },
    /// The first linked order is SPECIAL_ANIM, so retail advances once more before calling
    /// `get_air_order()`.
    AfterSpecialAnimation { first_order_type: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecallAirOrderFacts {
    pub resolution: RecallAirResolution,
    /// `AirOrder::oxx/+0x04` and `whose/+0x08`, relative to the adjusted AirOrder pointer.
    pub home: RecallObjectRef,
    /// Preserved across `close_orders` and the replacement STRAFE install.
    pub cruising_alt: i32,
    /// Preserved across `close_orders` and the replacement STRAFE install.
    pub sharp_turn: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecallPlaneCandidateFacts {
    pub unit_flags_2b4: u32,
    pub object_masks_1e4: u32,
    /// `None` represents a negative `get_inside` result.
    pub inside: Option<RecallInsideFacts>,
    /// Required unless `inside` names an active member of the selected group.
    pub current_air: Option<RecallAirOrderFacts>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecallOwnerUnitState {
    Invalid,
    NotPlane,
    /// `is_plane()` succeeded, but one of the two explicit post-predicate type gates skips it.
    ExcludedPlane {
        unit_flags_2b4: u32,
        object_masks_1e4: u32,
    },
    Candidate(RecallPlaneCandidateFacts),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecallOwnerUnitFacts {
    /// Index in the owner's `Players.units` pointer array; also the value stored in an
    /// airbase/carrier `ObjectData::launching` array.
    pub slot: i32,
    pub state: RecallOwnerUnitState,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecallFacts {
    /// Exact group after every scenario `Group::kill(o,who,0,0)` call has completed.
    pub group_after_ignore_orders: GroupData,
    /// Required only when the post-prelude group has `num > 0`.
    pub leader: Option<RecallLeaderFacts>,
    /// Read only on the non-return arm, after `action_begin`.
    pub group_members: Vec<RecallGroupMemberFacts>,
    /// The complete owner `Players.units` pointer array, in index order.
    pub owner_units: Vec<RecallOwnerUnitFacts>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecallEffect {
    ScenarioKill {
        target_o: i32,
        target_who: i32,
        tail_0: i32,
        tail_1: i32,
    },
    ClearBuildGather {
        who: u8,
        o: i16,
    },
    ClearAircraftOrders {
        who: u8,
        o: i32,
    },
    RemoveLaunchingSlot {
        host: RecallObjectRef,
        actor_slot: i32,
    },
    SetExistingAirReturning {
        who: u8,
        o: i32,
        value: i32,
    },
    ClearUnitMasks {
        who: u8,
        o: i32,
        mask: u32,
    },
    ResetPathLength {
        who: u8,
        o: i32,
        value: i32,
    },
    CloseOrders {
        who: u8,
        o: i32,
        arg: i32,
    },
    ClearPartialPath {
        who: u8,
        o: i32,
    },
    UpdateAction {
        who: u8,
        o: i32,
    },
    AddStrafeOrder {
        who: u8,
        o: i32,
        x: i32,
        y: i32,
        home_o: i32,
        home_who: i32,
        arg5: i32,
        queue_pos: i32,
        arg7: i32,
    },
    UpdateOrder {
        who: u8,
        o: i32,
    },
    RestoreNewAirCruisingAltitude {
        who: u8,
        o: i32,
        value: i32,
    },
    RestoreNewAirSharpTurn {
        who: u8,
        o: i32,
        value: i32,
    },
    /// A real downstream receiver, not permission to publish the preceding scenario kills.
    OpenActionReturnTail {
        leader_o: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecallBoundary {
    EmptyGroup,
    MainBody,
    OpenActionReturn,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecallPlan {
    pub group: GroupData,
    pub effects: Vec<RecallEffect>,
    pub boundary: RecallBoundary,
    /// Neither `action_recall` nor the modeled direct children draw from the canonical RNG.
    pub direct_rng_draws: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecallPlanError {
    UnexpectedScenarioSelection,
    ScenarioAfterImage,
    OwnerOutOfRange(u8),
    MemberCount(i32),
    MissingLeader,
    UnexpectedLeader,
    LeaderSource,
    LeaderIdentity,
    LeaderDomain,
    GroupMemberCount,
    GroupMemberIdentity {
        index: usize,
        expected: i16,
        got: i16,
    },
    MissingBuildPredicate {
        index: usize,
    },
    UnexpectedBuildPredicate {
        index: usize,
    },
    OwnerUnitSlot {
        expected: i32,
        got: i32,
    },
    PlaneGate,
    UnexpectedLaunchingFacts,
    MissingLaunchingFacts,
    UnexpectedAirOrder,
    MissingAirOrder,
    AirResolution,
}

fn selected_member(group: &GroupData, target: RecallObjectRef, n: usize) -> bool {
    target.o >= 0
        && target.who == i32::from(group.who)
        && target.active
        && group.list[..n]
            .iter()
            .any(|&member| i32::from(member) == target.o)
}

fn validate_air_resolution(resolution: RecallAirResolution) -> bool {
    match resolution {
        RecallAirResolution::UpdateOrder {
            head_present: false,
            first_order_type,
        } => first_order_type.is_none(),
        RecallAirResolution::UpdateOrder {
            head_present: true,
            first_order_type: Some(ty),
        } => ty != ORDER_SPECIAL_ANIM,
        RecallAirResolution::UpdateOrder {
            head_present: true,
            first_order_type: None,
        } => false,
        RecallAirResolution::AfterSpecialAnimation { first_order_type } => {
            first_order_type == ORDER_SPECIAL_ANIM
        }
    }
}

/// Recover the maximal atomic plan for `Group::action_recall`.
///
/// `OpenActionReturn` plans are observation-only.  [`RecallReceipt::validates`] rejects an
/// applied receipt for that boundary so a host cannot publish the scenario prefix while
/// silently omitting the delegated action.
pub fn plan_recall(
    request: &RecallRequest,
    facts: &RecallFacts,
) -> Result<RecallPlan, RecallPlanError> {
    let mut effects = Vec::new();
    let prelude_reached = request.ignore_orders && request.group.who < 8;
    if !prelude_reached && !request.scenario_selection.is_empty() {
        return Err(RecallPlanError::UnexpectedScenarioSelection);
    }
    if prelude_reached {
        for &o in &request.scenario_selection {
            if o >= 0 {
                effects.push(RecallEffect::ScenarioKill {
                    target_o: o,
                    target_who: i32::from(request.group.who),
                    tail_0: 0,
                    tail_1: 0,
                });
            }
        }
    }

    if effects.is_empty() && facts.group_after_ignore_orders != request.group {
        return Err(RecallPlanError::ScenarioAfterImage);
    }
    if facts.group_after_ignore_orders.who != request.group.who {
        return Err(RecallPlanError::ScenarioAfterImage);
    }

    let mut group = facts.group_after_ignore_orders.clone();
    if group.num <= 0 {
        if facts.leader.is_some()
            || !facts.group_members.is_empty()
            || !facts.owner_units.is_empty()
        {
            return Err(RecallPlanError::UnexpectedLeader);
        }
        return Ok(RecallPlan {
            group,
            effects,
            boundary: RecallBoundary::EmptyGroup,
            direct_rng_draws: 0,
        });
    }
    if group.who >= 8 {
        return Err(RecallPlanError::OwnerOutOfRange(group.who));
    }
    if group.num as usize > GROUP_MAX_MEMBERS {
        return Err(RecallPlanError::MemberCount(group.num));
    }
    let n = group.num as usize;

    let leader = facts.leader.ok_or(RecallPlanError::MissingLeader)?;
    let expected_source = if group.buildings != 0 {
        RecallLeaderSource::FirstMember
    } else {
        RecallLeaderSource::FindLeaderZero
    };
    if leader.source != expected_source {
        return Err(RecallPlanError::LeaderSource);
    }
    if leader.source == RecallLeaderSource::FirstMember && leader.o != i32::from(group.list[0]) {
        return Err(RecallPlanError::LeaderIdentity);
    }
    if leader.source == RecallLeaderSource::FindLeaderZero
        && leader.o >= 0
        && !group.list[..n]
            .iter()
            .any(|&member| i32::from(member) == leader.o)
    {
        return Err(RecallPlanError::LeaderIdentity);
    }
    if (leader.o >= 0) != leader.domain.is_some() {
        return Err(RecallPlanError::LeaderDomain);
    }
    if leader.domain == Some(AIR_DOMAIN) {
        if !facts.group_members.is_empty() || !facts.owner_units.is_empty() {
            return Err(RecallPlanError::UnexpectedLeader);
        }
        effects.push(RecallEffect::OpenActionReturnTail { leader_o: leader.o });
        return Ok(RecallPlan {
            group,
            effects,
            boundary: RecallBoundary::OpenActionReturn,
            direct_rng_draws: 0,
        });
    }

    // Group::action_begin: `disband = 0`.
    group.disband = 0;
    if facts.group_members.len() != n {
        return Err(RecallPlanError::GroupMemberCount);
    }
    for (index, member) in facts.group_members.iter().enumerate() {
        let expected = group.list[index];
        if member.o != expected {
            return Err(RecallPlanError::GroupMemberIdentity {
                index,
                expected,
                got: member.o,
            });
        }
        if member.valid_wall {
            let is_build = member
                .is_build
                .ok_or(RecallPlanError::MissingBuildPredicate { index })?;
            if is_build {
                effects.push(RecallEffect::ClearBuildGather {
                    who: group.who,
                    o: member.o,
                });
            }
        } else if member.is_build.is_some() {
            return Err(RecallPlanError::UnexpectedBuildPredicate { index });
        }
    }

    // Store occurs after the group-member clear_gather loop and before owner-unit scanning.
    group.form = -1;
    for (index, actor) in facts.owner_units.iter().enumerate() {
        let expected_slot = index as i32;
        if actor.slot != expected_slot {
            return Err(RecallPlanError::OwnerUnitSlot {
                expected: expected_slot,
                got: actor.slot,
            });
        }
        let candidate = match actor.state {
            RecallOwnerUnitState::Invalid | RecallOwnerUnitState::NotPlane => continue,
            RecallOwnerUnitState::ExcludedPlane {
                unit_flags_2b4,
                object_masks_1e4,
            } => {
                if unit_flags_2b4 & HELICOPTER_TYPE_FLAG == 0
                    && object_masks_1e4 & MISSILE_OBJECT_MASK == 0
                {
                    return Err(RecallPlanError::PlaneGate);
                }
                continue;
            }
            RecallOwnerUnitState::Candidate(candidate) => {
                if candidate.unit_flags_2b4 & HELICOPTER_TYPE_FLAG != 0
                    || candidate.object_masks_1e4 & MISSILE_OBJECT_MASK != 0
                {
                    return Err(RecallPlanError::PlaneGate);
                }
                candidate
            }
        };

        let selected_inside = candidate
            .inside
            .is_some_and(|inside| selected_member(&group, inside.target, n));
        if selected_inside {
            if candidate.current_air.is_some() {
                return Err(RecallPlanError::UnexpectedAirOrder);
            }
            let inside = candidate
                .inside
                .ok_or(RecallPlanError::MissingLaunchingFacts)?;
            let launching = inside
                .launching
                .ok_or(RecallPlanError::MissingLaunchingFacts)?;
            effects.push(RecallEffect::ClearAircraftOrders {
                who: group.who,
                o: actor.slot,
            });
            if launching
                == (RecallLaunchingFacts::Present {
                    contains_actor_slot: true,
                })
            {
                effects.push(RecallEffect::RemoveLaunchingSlot {
                    host: inside.target,
                    actor_slot: actor.slot,
                });
            }
            continue;
        }
        if candidate
            .inside
            .is_some_and(|inside| inside.launching.is_some())
        {
            return Err(RecallPlanError::UnexpectedLaunchingFacts);
        }

        let air = candidate
            .current_air
            .ok_or(RecallPlanError::MissingAirOrder)?;
        if !validate_air_resolution(air.resolution) {
            return Err(RecallPlanError::AirResolution);
        }
        if !selected_member(&group, air.home, n) {
            continue;
        }

        effects.extend([
            RecallEffect::SetExistingAirReturning {
                who: group.who,
                o: actor.slot,
                value: 1,
            },
            RecallEffect::ClearUnitMasks {
                who: group.who,
                o: actor.slot,
                mask: UNIT_MASK_RECALL_CLEAR,
            },
            RecallEffect::ResetPathLength {
                who: group.who,
                o: actor.slot,
                value: 0,
            },
            RecallEffect::CloseOrders {
                who: group.who,
                o: actor.slot,
                arg: 0,
            },
            RecallEffect::ClearPartialPath {
                who: group.who,
                o: actor.slot,
            },
            RecallEffect::UpdateAction {
                who: group.who,
                o: actor.slot,
            },
            RecallEffect::AddStrafeOrder {
                who: group.who,
                o: actor.slot,
                x: -1,
                y: -1,
                home_o: air.home.o,
                home_who: air.home.who,
                arg5: 0,
                queue_pos: QUEUE_NEW,
                arg7: 0,
            },
            RecallEffect::UpdateOrder {
                who: group.who,
                o: actor.slot,
            },
            RecallEffect::RestoreNewAirCruisingAltitude {
                who: group.who,
                o: actor.slot,
                value: air.cruising_alt,
            },
            RecallEffect::RestoreNewAirSharpTurn {
                who: group.who,
                o: actor.slot,
                value: air.sharp_turn,
            },
        ]);
    }

    Ok(RecallPlan {
        group,
        effects,
        boundary: RecallBoundary::MainBody,
        direct_rng_draws: 0,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecallTransactionStatus {
    Applied,
    Unavailable,
}

/// Atomic host receipt. `Applied` means the entire closed main-body plan was preflighted and
/// committed as one unit. `Unavailable` means no group, build, launching-list, order, path,
/// mask, or action state changed.
#[derive(Clone, Debug, PartialEq)]
pub struct RecallReceipt {
    pub request: RecallRequest,
    pub status: RecallTransactionStatus,
    pub facts: Option<RecallFacts>,
    pub plan: Option<RecallPlan>,
}

impl RecallReceipt {
    pub fn unavailable(request: RecallRequest) -> Self {
        Self {
            request,
            status: RecallTransactionStatus::Unavailable,
            facts: None,
            plan: None,
        }
    }

    pub fn validates(&self, expected: &RecallRequest) -> bool {
        if &self.request != expected {
            return false;
        }
        match self.status {
            RecallTransactionStatus::Unavailable => self.facts.is_none() && self.plan.is_none(),
            RecallTransactionStatus::Applied => {
                let (Some(facts), Some(observed)) = (self.facts.as_ref(), self.plan.as_ref())
                else {
                    return false;
                };
                observed.boundary != RecallBoundary::OpenActionReturn
                    && plan_recall(expected, facts).is_ok_and(|recomputed| recomputed == *observed)
            }
        }
    }
}
