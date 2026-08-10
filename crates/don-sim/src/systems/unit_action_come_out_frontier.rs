// SPDX-License-Identifier: GPL-3.0-or-later
//! Source-only reconstruction of `Unit::action_come_out` (`0x005e20b0`).
//!
//! The 532-byte wrapper is completely bounded here, including the Scholar/University
//! animation-chain repair.  Its final `Unit::come_out(0)` call is deliberately an authority
//! step: that 9,925-byte callee still owns collision, containment, Guys, Groups, RNG, and world
//! insertion.  Consequently this module is not registered in `systems::mod` and changes no
//! command-closure row.

pub const UNIT_ACTION_COME_OUT_VA: u32 = 0x005e_20b0;
pub const UNIT_ACTION_COME_OUT_BYTES: usize = 532;
pub const UNIT_ACTION_COME_OUT_END_VA: u32 = 0x005e_22c4;
pub const UNIT_COME_OUT_VA: u32 = 0x0061_7c10;
pub const UNIT_COME_OUT_BYTES: usize = 9_925;
pub const SCHOLAR_TYPE: i32 = 0x34;
pub const KOREAN_SCHOLAR_TYPE: i32 = 0x35;
pub const UNIVERSITY_TYPE: i32 = 0x1a4;
pub const LAUNCHING_UNIT_MASK: u32 = 0x0400_0000;
pub const LEADER_SCHOLAR_RELEASE_MASK: u32 = 0x0200_0000;

/// The exact static closure delta of this source-only frontier.
pub const COMPLETE_OPCODE_DELTA: usize = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ObjectIdentity {
    pub owner: u8,
    pub object: i16,
}

impl ObjectIdentity {
    pub const fn new(owner: u8, object: i16) -> Self {
        Self { owner, object }
    }

    fn safe(self) -> bool {
        self.owner < 8 && self.object >= 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuyAnimationState {
    /// `GuyData::end_time` at `Guy+0x74`.
    pub end_time: u32,
    /// `GuyData::cur_anim` at `Guy+0x9c`.
    pub cur_anim: i8,
    /// `GuyData::anim_index_hints.length` at `Guy+0xd0`.
    pub anim_hint_length: i32,
}

/// One object reached by following `ObjectData::inside_down` / `inside_down_who`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsideChainNode {
    pub identity: ObjectIdentity,
    pub type_index: i32,
    pub next: Option<ObjectIdentity>,
    /// Retail dereferences `UnitData::guys.list[0]` only for Scholar nodes.
    pub first_guy: Option<GuyAnimationState>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsideLookupFacts {
    /// `ObjectData::get_inside(&owner)` result. `None` represents its negative object result.
    pub container: Option<ObjectIdentity>,
    /// Virtual `ObjectData::is_build` at vtable `+0x20`. Reached only for a Scholar whose
    /// returned container owner equals the actor owner.
    pub container_is_build: Option<bool>,
    /// `container->get_build()->is(UNIVERSITY, 0)`. Reached only after `is_build != 0`.
    pub container_is_university: Option<bool>,
}

impl Default for InsideLookupFacts {
    fn default() -> Self {
        Self {
            container: None,
            container_is_build: None,
            container_is_university: None,
        }
    }
}

/// Complete branch-sensitive facts consumed by the 532-byte wrapper.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitActionComeOutFacts {
    pub actor: ObjectIdentity,
    pub actor_type: i32,
    pub unit_masks: u32,
    pub inside: InsideLookupFacts,
    /// First `Guy` of the actor. Required only on the Scholar-in-University branch.
    pub actor_first_guy: Option<GuyAnimationState>,
    /// Actor's initial `inside_down` edge. Required only on the University branch.
    pub actor_inside_down: Option<ObjectIdentity>,
    /// Exact successive children reached through the initial `inside_down` edge.
    pub inside_chain: Vec<InsideChainNode>,
    /// `LeaderData::leader_flags` before OR `0x02000000`; branch-lazy.
    pub leader_flags: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitActionComeOutStep {
    SetUnitMasks {
        before: u32,
        after: u32,
    },
    ClearPathAnchor,
    CloseOrders {
        argument: i32,
    },
    ClearPartialPath,
    UpdateAction,
    SetLeaderFlags {
        owner: u8,
        before: u32,
        after: u32,
    },
    ClearFirstGuyAnimHints {
        unit: ObjectIdentity,
        before: i32,
    },
    ShiftScholarAnimation {
        unit: ObjectIdentity,
        before_end_time: u32,
        before_cur_anim: i8,
        after_end_time: u32,
        after_cur_anim: i8,
    },
    /// The mandatory final call. This frontier observes but cannot apply it.
    AuthorityUnitComeOut {
        unit: ObjectIdentity,
        argument: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitActionComeOutPlan {
    pub actor: ObjectIdentity,
    pub steps: Vec<UnitActionComeOutStep>,
    /// Always true: every return path reaches `Unit::come_out(0)` at `0x005e22ab`.
    pub downstream_required: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingComeOutFact {
    ContainerIsBuild,
    ContainerIsUniversity,
    ActorFirstGuy,
    LeaderFlags,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitActionComeOutPlanError {
    UnsafeActor(ObjectIdentity),
    UnsafeContainer(ObjectIdentity),
    Missing(MissingComeOutFact),
    UnexpectedActorGuy,
    UnexpectedLeaderFlags,
    UnexpectedInsideChain,
    ChainHeadMismatch,
    UnsafeChainIdentity(ObjectIdentity),
    ChainLinkMismatch {
        at: ObjectIdentity,
        expected: Option<ObjectIdentity>,
        observed: Option<ObjectIdentity>,
    },
    MissingScholarGuy(ObjectIdentity),
    UnexpectedNonScholarGuy(ObjectIdentity),
}

fn is_scholar(type_index: i32) -> bool {
    matches!(type_index, SCHOLAR_TYPE | KOREAN_SCHOLAR_TYPE)
}

fn university_branch(facts: &UnitActionComeOutFacts) -> Result<bool, UnitActionComeOutPlanError> {
    let Some(container) = facts.inside.container else {
        return Ok(false);
    };
    if !container.safe() {
        return Err(UnitActionComeOutPlanError::UnsafeContainer(container));
    }
    if !is_scholar(facts.actor_type) || container.owner != facts.actor.owner {
        return Ok(false);
    }
    let is_build = facts
        .inside
        .container_is_build
        .ok_or(UnitActionComeOutPlanError::Missing(
            MissingComeOutFact::ContainerIsBuild,
        ))?;
    if !is_build {
        return Ok(false);
    }
    facts
        .inside
        .container_is_university
        .ok_or(UnitActionComeOutPlanError::Missing(
            MissingComeOutFact::ContainerIsUniversity,
        ))
}

fn validate_chain(facts: &UnitActionComeOutFacts) -> Result<(), UnitActionComeOutPlanError> {
    if facts.actor_inside_down != facts.inside_chain.first().map(|node| node.identity) {
        return Err(UnitActionComeOutPlanError::ChainHeadMismatch);
    }
    for (index, node) in facts.inside_chain.iter().enumerate() {
        if !node.identity.safe() {
            return Err(UnitActionComeOutPlanError::UnsafeChainIdentity(
                node.identity,
            ));
        }
        let expected = facts.inside_chain.get(index + 1).map(|next| next.identity);
        if node.next != expected {
            return Err(UnitActionComeOutPlanError::ChainLinkMismatch {
                at: node.identity,
                expected,
                observed: node.next,
            });
        }
        match (is_scholar(node.type_index), node.first_guy) {
            (true, None) => {
                return Err(UnitActionComeOutPlanError::MissingScholarGuy(node.identity))
            }
            (false, Some(_)) => {
                return Err(UnitActionComeOutPlanError::UnexpectedNonScholarGuy(
                    node.identity,
                ))
            }
            _ => {}
        }
    }
    Ok(())
}

/// Plan the complete state-writing order of `Unit::action_come_out`.
///
/// The caller must commit the returned steps together with the final authority call or none
/// of them. The wrapper clears orders/path state before learning whether the general release
/// succeeds, so publishing only this prefix would be a retail-incompatible partial mutation.
pub fn plan_unit_action_come_out(
    facts: &UnitActionComeOutFacts,
) -> Result<UnitActionComeOutPlan, UnitActionComeOutPlanError> {
    if !facts.actor.safe() {
        return Err(UnitActionComeOutPlanError::UnsafeActor(facts.actor));
    }
    let university = university_branch(facts)?;
    if !university {
        if facts.actor_first_guy.is_some() {
            return Err(UnitActionComeOutPlanError::UnexpectedActorGuy);
        }
        if facts.leader_flags.is_some() {
            return Err(UnitActionComeOutPlanError::UnexpectedLeaderFlags);
        }
        if facts.actor_inside_down.is_some() || !facts.inside_chain.is_empty() {
            return Err(UnitActionComeOutPlanError::UnexpectedInsideChain);
        }
    }

    let mut steps = vec![
        UnitActionComeOutStep::SetUnitMasks {
            before: facts.unit_masks,
            after: facts.unit_masks & !LAUNCHING_UNIT_MASK,
        },
        UnitActionComeOutStep::ClearPathAnchor,
        UnitActionComeOutStep::CloseOrders { argument: 0 },
        UnitActionComeOutStep::ClearPartialPath,
        UnitActionComeOutStep::UpdateAction,
    ];

    if university {
        let actor_guy = facts
            .actor_first_guy
            .ok_or(UnitActionComeOutPlanError::Missing(
                MissingComeOutFact::ActorFirstGuy,
            ))?;
        let leader_flags = facts
            .leader_flags
            .ok_or(UnitActionComeOutPlanError::Missing(
                MissingComeOutFact::LeaderFlags,
            ))?;
        validate_chain(facts)?;

        steps.push(UnitActionComeOutStep::SetLeaderFlags {
            owner: facts.actor.owner,
            before: leader_flags,
            after: leader_flags | LEADER_SCHOLAR_RELEASE_MASK,
        });
        steps.push(UnitActionComeOutStep::ClearFirstGuyAnimHints {
            unit: facts.actor,
            before: actor_guy.anim_hint_length,
        });

        // Retail carries the last Scholar animation through intervening non-Scholar nodes.
        let mut carried_end_time = actor_guy.end_time;
        let mut carried_cur_anim = actor_guy.cur_anim;
        for node in &facts.inside_chain {
            let Some(guy) = node.first_guy else {
                continue;
            };
            steps.push(UnitActionComeOutStep::ClearFirstGuyAnimHints {
                unit: node.identity,
                before: guy.anim_hint_length,
            });
            steps.push(UnitActionComeOutStep::ShiftScholarAnimation {
                unit: node.identity,
                before_end_time: guy.end_time,
                before_cur_anim: guy.cur_anim,
                after_end_time: carried_end_time,
                after_cur_anim: carried_cur_anim,
            });
            carried_end_time = guy.end_time;
            carried_cur_anim = guy.cur_anim;
        }
    }

    steps.push(UnitActionComeOutStep::AuthorityUnitComeOut {
        unit: facts.actor,
        argument: 0,
    });
    Ok(UnitActionComeOutPlan {
        actor: facts.actor,
        steps,
        downstream_required: true,
    })
}

/// Snapshot binding required by a future atomic host adapter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitActionComeOutPreflight {
    pub object_epoch: u64,
    pub order_epoch: u64,
    pub containment_epoch: u64,
    pub guy_epoch: u64,
    pub leader_epoch: u64,
    pub facts: UnitActionComeOutFacts,
    pub plan: UnitActionComeOutPlan,
}

pub fn preflight_unit_action_come_out(
    object_epoch: u64,
    order_epoch: u64,
    containment_epoch: u64,
    guy_epoch: u64,
    leader_epoch: u64,
    facts: UnitActionComeOutFacts,
) -> Result<UnitActionComeOutPreflight, UnitActionComeOutPlanError> {
    let plan = plan_unit_action_come_out(&facts)?;
    Ok(UnitActionComeOutPreflight {
        object_epoch,
        order_epoch,
        containment_epoch,
        guy_epoch,
        leader_epoch,
        facts,
        plan,
    })
}

pub fn preflight_still_valid(
    expected: &UnitActionComeOutPreflight,
    observed: &UnitActionComeOutPreflight,
) -> bool {
    expected == observed
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitActionComeOutOpenTail {
    /// Mandatory `Unit::come_out(0)` at `0x00617c10`, 9,925 bytes.
    GeneralComeOutTransaction,
    CanonicalObjectAndTypeQueries,
    LiveOrderPathGuyLeaderAdapter,
    AtomicOpcode49Commit,
}

pub const UNIT_ACTION_COME_OUT_OPEN_TAILS: [UnitActionComeOutOpenTail; 4] = [
    UnitActionComeOutOpenTail::GeneralComeOutTransaction,
    UnitActionComeOutOpenTail::CanonicalObjectAndTypeQueries,
    UnitActionComeOutOpenTail::LiveOrderPathGuyLeaderAdapter,
    UnitActionComeOutOpenTail::AtomicOpcode49Commit,
];
