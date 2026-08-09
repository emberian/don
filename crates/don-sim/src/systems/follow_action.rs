// SPDX-License-Identifier: GPL-3.0-or-later
//! Atomic transaction contract for `Group::action_follow` `0x006FD510`.
//!
//! The planner starts from the receiver after the scenario `ignore_orders` prelude.  That
//! prelude and the `QUEUE_FIRST` helper calls own world state which is not represented by
//! [`GroupData`], so they remain explicit host effects.  Everything else in the 645-byte
//! receiver is represented here in retail order: `action_begin`, the building and signed
//! target gates, `form = -1`, target admission, member admission, the canonical self-target
//! rejection, and the exact raw queue value passed to `Unit::add_follow_order`.

use crate::systems::groups_guys::{GroupData, GROUP_MAX_MEMBERS};

pub const FOLLOW_OPCODE: u8 = 30;
pub const FOLLOW_WIRE_SIZE: usize = 13;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FollowCommand {
    pub target_o: i32,
    pub target_who: i32,
    /// The raw `QueuePos` dword.  Retail treats zero specially at the group layer, two
    /// specially in `add_follow_order`, and passes every other value through unchanged.
    pub queued: i32,
}

pub fn decode_follow(wire: &[u8]) -> Option<FollowCommand> {
    if wire.len() != FOLLOW_WIRE_SIZE || wire.first().copied()? != FOLLOW_OPCODE {
        return None;
    }
    let read = |offset: usize| {
        Some(i32::from_le_bytes([
            *wire.get(offset)?,
            *wire.get(offset + 1)?,
            *wire.get(offset + 2)?,
            *wire.get(offset + 3)?,
        ]))
    };
    Some(FollowCommand {
        target_o: read(1)?,
        target_who: read(5)?,
        queued: read(9)?,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct FollowRequest {
    pub group: GroupData,
    pub command: FollowCommand,
}

/// Results of the target virtuals read at `0x006FD66A..0x006FD6A4` and the later
/// `ObjectData::get_o` call at `0x006FD734`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FollowTargetFacts {
    /// Echo of the owner-band lookup.  These are the raw command identities, not the
    /// canonical identity returned by `get_o`.
    pub queried_o: i32,
    pub queried_who: i32,
    pub valid_unit: bool,
    pub on_map: bool,
    pub is_plane: bool,
    /// `ObjectData::get_o` (virtual `+0xE4`).  Retail uses this only for the self-target
    /// rejection; `add_follow_order` still receives the two raw command identities.
    pub canonical_o: i32,
}

/// Results of the three member virtuals at `0x006FD6DF..0x006FD720`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FollowMemberFacts {
    /// Identity sentinel; must equal the corresponding live `GroupData::list` entry.
    pub o: i16,
    pub valid_unit: bool,
    pub on_map: bool,
    pub is_plane: bool,
}

/// Instruction-ordered effects outside the checksum-owned [`GroupData`] after-image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FollowEffect {
    /// `Group::set_up_insert(OrderList*)` `0x0070E520`.
    SetUpInsert,
    /// `Group::action_halt(0)` `0x0070D0C0`.  A product host must apply the complete
    /// already-recovered halt transaction, not merely clear a vector.
    ActionHalt { flags: i32 },
    /// `Unit::add_follow_order(raw_o, raw_who, queued, ignored_tail)` `0x005E3F60`.
    /// The callee constructs both target identities/UIDs, performs the `QUEUE_NEW`
    /// lifecycle when `queued == 2`, and updates the actor action.
    AddFollowOrder {
        actor_who: u8,
        actor_o: i16,
        target_o: i32,
        target_who: i32,
        queued: i32,
    },
    /// `Group::finish_insert(OrderList*)` `0x0070E620`.
    FinishInsert,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FollowPlan {
    /// Includes the unconditional leading `action_begin` store and, once the signed target
    /// gates are crossed, the `form = -1` store which precedes target admission.
    pub group: GroupData,
    pub effects: Vec<FollowEffect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FollowPlanError {
    TargetIdentity,
    MemberCount,
    MemberIdentity {
        index: usize,
        expected: i16,
        got: i16,
    },
}

/// Recover the complete simulation-side plan of `Group::action_follow`.
///
/// `group_after_ignore_orders` is supplied by the atomic host after it has applied the
/// scenario-owned `Group::kill` prelude to `request.group`.  In ordinary lockstep play the
/// prelude is disabled and the two group values differ only by `action_begin`'s
/// `disband = 0` store.  Missing world capability must make the enclosing receipt
/// unavailable; it must never be represented by deleting one [`FollowEffect`].
pub fn plan_follow(
    request: &FollowRequest,
    group_after_ignore_orders: &GroupData,
    target: Option<FollowTargetFacts>,
    members: &[FollowMemberFacts],
) -> Result<FollowPlan, FollowPlanError> {
    let mut group = group_after_ignore_orders.clone();
    group.disband = 0;

    // The building and signed-target gates precede the scenario prelude, form reset, and
    // all object lookups.  No target/member snapshot is required on either early return.
    if group.buildings != 0 || request.command.target_o < 0 || request.command.target_who < 0 {
        return Ok(FollowPlan {
            group,
            effects: Vec::new(),
        });
    }

    let queue_first = request.command.queued == 0;
    let mut effects = Vec::new();
    if queue_first {
        effects.extend([
            FollowEffect::SetUpInsert,
            FollowEffect::ActionHalt { flags: 0 },
        ]);
    }

    // On the direct arm this store is at 0x006FD64C, before the target virtuals.  The
    // QUEUE_FIRST arm reaches the same after-image through action_halt and its recursive
    // QUEUE_NEW invocation.  It therefore survives an absent or rejected target.
    group.form = -1;

    let Some(target) = target else {
        if queue_first {
            effects.push(FollowEffect::FinishInsert);
        }
        return Ok(FollowPlan { group, effects });
    };
    if target.queried_o != request.command.target_o
        || target.queried_who != request.command.target_who
    {
        return Err(FollowPlanError::TargetIdentity);
    }
    if !target.valid_unit || !target.on_map || target.is_plane {
        if queue_first {
            effects.push(FollowEffect::FinishInsert);
        }
        return Ok(FollowPlan { group, effects });
    }

    let n = group.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
    if members.len() != n {
        return Err(FollowPlanError::MemberCount);
    }
    for (index, facts) in members.iter().enumerate() {
        let expected = group.list[index];
        if facts.o != expected {
            return Err(FollowPlanError::MemberIdentity {
                index,
                expected,
                got: facts.o,
            });
        }
    }

    let queued = if queue_first {
        // The recursive call at 0x006FD606 pushes literal QUEUE_NEW.
        2
    } else {
        request.command.queued
    };
    for member in members {
        if !member.valid_unit
            || !member.on_map
            || member.is_plane
            || (i32::from(member.o) == target.canonical_o
                && i32::from(group.who) == request.command.target_who)
        {
            continue;
        }
        effects.push(FollowEffect::AddFollowOrder {
            actor_who: group.who,
            actor_o: member.o,
            target_o: request.command.target_o,
            target_who: request.command.target_who,
            queued,
        });
    }
    if queue_first {
        effects.push(FollowEffect::FinishInsert);
    }
    Ok(FollowPlan { group, effects })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FollowTransactionStatus {
    Applied,
    Unavailable,
}

/// Atomic host receipt.  An applied receipt means every effect was preflighted and committed
/// as one transaction; unavailable means no group, order, path, mask, or action mutation.
#[derive(Clone, Debug, PartialEq)]
pub struct FollowReceipt {
    pub request: FollowRequest,
    pub status: FollowTransactionStatus,
    pub group_after_ignore_orders: Option<GroupData>,
    pub target: Option<FollowTargetFacts>,
    pub members: Vec<FollowMemberFacts>,
    pub plan: Option<FollowPlan>,
}

impl FollowReceipt {
    pub fn unavailable(request: FollowRequest) -> Self {
        Self {
            request,
            status: FollowTransactionStatus::Unavailable,
            group_after_ignore_orders: None,
            target: None,
            members: Vec::new(),
            plan: None,
        }
    }

    pub fn validates(&self, expected: &FollowRequest) -> bool {
        if &self.request != expected {
            return false;
        }
        match self.status {
            FollowTransactionStatus::Unavailable => {
                self.group_after_ignore_orders.is_none()
                    && self.target.is_none()
                    && self.members.is_empty()
                    && self.plan.is_none()
            }
            FollowTransactionStatus::Applied => {
                let (Some(group), Some(observed)) =
                    (self.group_after_ignore_orders.as_ref(), self.plan.as_ref())
                else {
                    return false;
                };
                plan_follow(expected, group, self.target, &self.members)
                    .is_ok_and(|recomputed| recomputed == *observed)
            }
        }
    }
}
