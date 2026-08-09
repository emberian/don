// SPDX-License-Identifier: GPL-3.0-or-later
//! Source-only atomic plan for `Group::action_return` `0x006FAD40`.
//!
//! This receiver is the explicit tail reached by `Group::action_recall` when the selected
//! leader is air-domain.  It has no further group-action delegate.  The planner preserves
//! the optimized concrete `UnitData::is_plane` cone, the dynamic virtual cone, contained
//! aircraft launching-list removal, airborne STRAFE replacement, and the helicopter arm.

use crate::systems::groups_guys::{GroupData, GROUP_MAX_MEMBERS};

pub const GROUP_ACTION_RETURN_VA: u32 = 0x006f_ad40;
pub const GROUP_ACTION_RETURN_BYTES: usize = 1_307;
pub const UNIT_DATA_IS_PLANE_VA: u32 = 0x0046_ce40;
pub const OBJECT_GET_INSIDE_VA: u32 = 0x0065_1a80;
pub const UNIT_CLOSE_ORDERS_VA: u32 = 0x005e_37f0;
pub const UNIT_CLEAR_PARTIAL_PATH_VA: u32 = 0x005e_3920;
pub const UNIT_UPDATE_ACTION_VA: u32 = 0x0060_a870;
pub const UNIT_ADD_STRAFE_ORDER_VA: u32 = 0x005e_48c0;
pub const ORDER_SPECIAL_ANIM: i32 = 25;
pub const QUEUE_NEW: i32 = 2;
pub const AIR_DOMAIN: i32 = 2;
pub const HELICOPTER_TYPE_FLAG: u32 = 0x20;
pub const MISSILE_OBJECT_MASK: u32 = 0x0800_0000;
pub const UNIT_MASK_RETURN_CLEAR: u32 = 0x0400_0000;

#[derive(Clone, Debug, PartialEq)]
pub struct ReturnRequest {
    pub group: GroupData,
    /// Snapshot of scenario global `ignore_orders` (`0x00CC02F8`).
    pub ignore_orders: bool,
    /// Owner scenario selection array walked after `action_begin` and before member dispatch.
    pub scenario_selection: Vec<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReturnInsideLookup {
    /// `ObjectData::get_inside` return value.  Retail branches only on this signed value.
    pub o: i32,
    /// The out-parameter written by `get_inside`.
    pub who: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnLaunchingFacts {
    Null,
    Present { contains_actor_o: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReturnContainedFacts {
    /// Retail calls `get_inside` a second time after clearing orders/path/action state.
    pub second_inside: ReturnInsideLookup,
    pub launching: ReturnLaunchingFacts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnAirResolution {
    /// `Unit::update_order()` followed by virtual `get_air_order()`.
    UpdateOrder {
        head_present: bool,
        /// Read only when the linked-list head is present; must not be SPECIAL_ANIM.
        first_order_type: Option<i32>,
    },
    /// The first linked order is SPECIAL_ANIM; retail advances one node and calls
    /// `get_air_order()` without calling `Unit::update_order()`.
    AfterSpecialAnimation { first_order_type: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReturnAirOrderFacts {
    /// `AirOrder+0x04/+0x08`, relative to the adjusted secondary-base pointer.
    pub home_o: i32,
    pub home_who: i32,
    /// `AirOrder+0x0C/+0x10`, restored on the replacement order.
    pub cruising_alt: i32,
    pub sharp_turn: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnAirLookup {
    /// The null result is checked before the `get_air_order` virtual.
    UpdateOrderMissing { resolution: ReturnAirResolution },
    /// `get_air_order()` returned null.
    AirOrderMissing { resolution: ReturnAirResolution },
    Found {
        resolution: ReturnAirResolution,
        order: ReturnAirOrderFacts,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReturnOrdinaryAircraftFacts {
    pub first_inside: ReturnInsideLookup,
    /// Required only when `first_inside.o >= 0`.
    pub contained: Option<ReturnContainedFacts>,
    /// Required only when `first_inside.o < 0`.
    pub airborne: Option<ReturnAirLookup>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnRouteFacts {
    OrdinaryAircraft(ReturnOrdinaryAircraftFacts),
    Helicopter { on_map: bool },
}

/// Facts controlling the pointer-equality optimization at `0x006FAE32`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnPlanePredicate {
    /// The virtual slot equals the concrete `UnitData::is_plane` body at `0x0046CE40`.
    Concrete {
        domain_218: i32,
        unit_flags_2b4: u32,
        /// Read only when the concrete predicate succeeds.
        object_masks_1e4: Option<u32>,
    },
    /// An overridden virtual is called.  A false result falls through to a separate
    /// `unit_flags & 0x20` helicopter test; a true result reads neither type word here.
    Dynamic {
        is_plane: bool,
        unit_flags_2b4: Option<u32>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReturnMemberFacts {
    /// Identity sentinel; must equal the corresponding post-scenario `GroupData::list` word.
    pub o: i16,
    pub valid_unit: bool,
    /// Read only after `is_valid_unit` succeeds.
    pub plane: Option<ReturnPlanePredicate>,
    /// Present exactly when the measured predicate cone enters an actionable route.
    pub route: Option<ReturnRouteFacts>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReturnFacts {
    /// Exact group after `action_begin` and every enabled scenario kill.
    pub group_after_ignore_orders: GroupData,
    pub members: Vec<ReturnMemberFacts>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnEffect {
    ActionBegin {
        disband: i32,
    },
    ScenarioKill {
        target_o: i32,
        target_who: i32,
        tail_0: i32,
        tail_1: i32,
    },
    ClearUnitMasks {
        who: u8,
        o: i16,
        mask: u32,
    },
    ResetPathLength {
        who: u8,
        o: i16,
        value: i32,
    },
    CloseOrders {
        who: u8,
        o: i16,
        arg: i32,
    },
    ClearPartialPath {
        who: u8,
        o: i16,
    },
    UpdateAction {
        who: u8,
        o: i16,
    },
    RemoveLaunchingObject {
        host_o: i32,
        host_who: i32,
        actor_o: i16,
    },
    AddStrafeOrder {
        who: u8,
        o: i16,
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
        o: i16,
    },
    RestoreNewAirCruisingAltitude {
        who: u8,
        o: i16,
        value: i32,
    },
    RestoreNewAirSharpTurn {
        who: u8,
        o: i16,
        value: i32,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReturnPlan {
    pub group: GroupData,
    pub effects: Vec<ReturnEffect>,
    pub direct_rng_draws: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnPlanError {
    UnexpectedScenarioSelection,
    ScenarioAfterImage,
    OwnerOutOfRange(u8),
    MemberCount(i32),
    MemberFactsCount,
    MemberIdentity {
        index: usize,
        expected: i16,
        got: i16,
    },
    MissingPlanePredicate {
        index: usize,
    },
    UnexpectedPlanePredicate {
        index: usize,
    },
    MissingObjectMasks {
        index: usize,
    },
    UnexpectedObjectMasks {
        index: usize,
    },
    MissingDynamicUnitFlags {
        index: usize,
    },
    UnexpectedDynamicUnitFlags {
        index: usize,
    },
    MissingRoute {
        index: usize,
    },
    UnexpectedRoute {
        index: usize,
    },
    RouteKind {
        index: usize,
    },
    MissingContainedFacts {
        index: usize,
    },
    UnexpectedContainedFacts {
        index: usize,
    },
    MissingAirborneFacts {
        index: usize,
    },
    UnexpectedAirborneFacts {
        index: usize,
    },
    UnsafeSecondInsideAddress {
        index: usize,
    },
    AirResolution {
        index: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MeasuredRoute {
    Skip,
    Ordinary,
    Helicopter,
}

fn classify_plane(
    index: usize,
    predicate: ReturnPlanePredicate,
) -> Result<MeasuredRoute, ReturnPlanError> {
    match predicate {
        ReturnPlanePredicate::Concrete {
            domain_218,
            unit_flags_2b4,
            object_masks_1e4,
        } => {
            let concrete_plane =
                domain_218 == AIR_DOMAIN && unit_flags_2b4 & HELICOPTER_TYPE_FLAG == 0;
            if concrete_plane {
                let masks =
                    object_masks_1e4.ok_or(ReturnPlanError::MissingObjectMasks { index })?;
                if masks & MISSILE_OBJECT_MASK != 0 {
                    Ok(MeasuredRoute::Skip)
                } else {
                    Ok(MeasuredRoute::Ordinary)
                }
            } else {
                if object_masks_1e4.is_some() {
                    return Err(ReturnPlanError::UnexpectedObjectMasks { index });
                }
                if unit_flags_2b4 & HELICOPTER_TYPE_FLAG != 0 {
                    Ok(MeasuredRoute::Helicopter)
                } else {
                    Ok(MeasuredRoute::Skip)
                }
            }
        }
        ReturnPlanePredicate::Dynamic {
            is_plane: true,
            unit_flags_2b4,
        } => {
            if unit_flags_2b4.is_some() {
                Err(ReturnPlanError::UnexpectedDynamicUnitFlags { index })
            } else {
                Ok(MeasuredRoute::Ordinary)
            }
        }
        ReturnPlanePredicate::Dynamic {
            is_plane: false,
            unit_flags_2b4,
        } => {
            let flags = unit_flags_2b4.ok_or(ReturnPlanError::MissingDynamicUnitFlags { index })?;
            if flags & HELICOPTER_TYPE_FLAG != 0 {
                Ok(MeasuredRoute::Helicopter)
            } else {
                Ok(MeasuredRoute::Skip)
            }
        }
    }
}

fn valid_air_resolution(resolution: ReturnAirResolution) -> bool {
    match resolution {
        ReturnAirResolution::UpdateOrder {
            head_present: false,
            first_order_type,
        } => first_order_type.is_none(),
        ReturnAirResolution::UpdateOrder {
            head_present: true,
            first_order_type: Some(ty),
        } => ty != ORDER_SPECIAL_ANIM,
        ReturnAirResolution::UpdateOrder {
            head_present: true,
            first_order_type: None,
        } => false,
        ReturnAirResolution::AfterSpecialAnimation { first_order_type } => {
            first_order_type == ORDER_SPECIAL_ANIM
        }
    }
}

fn reset_steps(who: u8, o: i16) -> [ReturnEffect; 5] {
    [
        ReturnEffect::ClearUnitMasks {
            who,
            o,
            mask: UNIT_MASK_RETURN_CLEAR,
        },
        ReturnEffect::ResetPathLength { who, o, value: 0 },
        ReturnEffect::CloseOrders { who, o, arg: 0 },
        ReturnEffect::ClearPartialPath { who, o },
        ReturnEffect::UpdateAction { who, o },
    ]
}

/// Recover the full simulation-side effect plan of `Group::action_return`.
///
/// Stateful child calls are emitted as ordered host effects and must be preflighted and
/// published atomically by the eventual product adapter.  Malformed facts fail before any
/// mutation is authorized.
pub fn plan_return(
    request: &ReturnRequest,
    facts: &ReturnFacts,
) -> Result<ReturnPlan, ReturnPlanError> {
    // action_begin precedes the scenario prelude in this receiver.
    let mut after_action_begin = request.group.clone();
    after_action_begin.disband = 0;

    let mut effects = vec![ReturnEffect::ActionBegin { disband: 0 }];
    let mut scenario_calls = 0usize;
    let prelude_reached = request.ignore_orders && after_action_begin.who < 8;
    if !prelude_reached && !request.scenario_selection.is_empty() {
        return Err(ReturnPlanError::UnexpectedScenarioSelection);
    }
    if prelude_reached {
        for &o in &request.scenario_selection {
            if o >= 0 {
                scenario_calls += 1;
                effects.push(ReturnEffect::ScenarioKill {
                    target_o: o,
                    target_who: i32::from(after_action_begin.who),
                    tail_0: 0,
                    tail_1: 0,
                });
            }
        }
    }
    if scenario_calls == 0 && facts.group_after_ignore_orders != after_action_begin {
        return Err(ReturnPlanError::ScenarioAfterImage);
    }
    if facts.group_after_ignore_orders.who != after_action_begin.who {
        return Err(ReturnPlanError::ScenarioAfterImage);
    }

    let group = facts.group_after_ignore_orders.clone();
    if group.num <= 0 {
        if !facts.members.is_empty() {
            return Err(ReturnPlanError::MemberFactsCount);
        }
        return Ok(ReturnPlan {
            group,
            effects,
            direct_rng_draws: 0,
        });
    }
    if group.who >= 8 {
        return Err(ReturnPlanError::OwnerOutOfRange(group.who));
    }
    if group.num as usize > GROUP_MAX_MEMBERS {
        return Err(ReturnPlanError::MemberCount(group.num));
    }
    let n = group.num as usize;
    if facts.members.len() != n {
        return Err(ReturnPlanError::MemberFactsCount);
    }

    for (index, member) in facts.members.iter().enumerate() {
        let expected = group.list[index];
        if member.o != expected {
            return Err(ReturnPlanError::MemberIdentity {
                index,
                expected,
                got: member.o,
            });
        }
        if !member.valid_unit {
            if member.plane.is_some() {
                return Err(ReturnPlanError::UnexpectedPlanePredicate { index });
            }
            if member.route.is_some() {
                return Err(ReturnPlanError::UnexpectedRoute { index });
            }
            continue;
        }

        let predicate = member
            .plane
            .ok_or(ReturnPlanError::MissingPlanePredicate { index })?;
        match classify_plane(index, predicate)? {
            MeasuredRoute::Skip => {
                if member.route.is_some() {
                    return Err(ReturnPlanError::UnexpectedRoute { index });
                }
            }
            MeasuredRoute::Helicopter => {
                let Some(ReturnRouteFacts::Helicopter { on_map }) = member.route else {
                    return Err(match member.route {
                        None => ReturnPlanError::MissingRoute { index },
                        Some(_) => ReturnPlanError::RouteKind { index },
                    });
                };
                effects.extend(reset_steps(group.who, member.o));
                if on_map {
                    effects.push(ReturnEffect::AddStrafeOrder {
                        who: group.who,
                        o: member.o,
                        x: -1,
                        y: -1,
                        home_o: -1,
                        home_who: -1,
                        arg5: 1,
                        queue_pos: QUEUE_NEW,
                        arg7: 0,
                    });
                }
            }
            MeasuredRoute::Ordinary => {
                let Some(ReturnRouteFacts::OrdinaryAircraft(ordinary)) = member.route else {
                    return Err(match member.route {
                        None => ReturnPlanError::MissingRoute { index },
                        Some(_) => ReturnPlanError::RouteKind { index },
                    });
                };
                if ordinary.first_inside.o >= 0 {
                    if ordinary.airborne.is_some() {
                        return Err(ReturnPlanError::UnexpectedAirborneFacts { index });
                    }
                    let contained = ordinary
                        .contained
                        .ok_or(ReturnPlanError::MissingContainedFacts { index })?;
                    // Retail directly dereferences the second address.  The safe planner
                    // rejects a malformed address instead of reproducing an access violation.
                    if contained.second_inside.o < 0
                        || !(0..8).contains(&contained.second_inside.who)
                    {
                        return Err(ReturnPlanError::UnsafeSecondInsideAddress { index });
                    }
                    effects.extend(reset_steps(group.who, member.o));
                    if contained.launching
                        == (ReturnLaunchingFacts::Present {
                            contains_actor_o: true,
                        })
                    {
                        effects.push(ReturnEffect::RemoveLaunchingObject {
                            host_o: contained.second_inside.o,
                            host_who: contained.second_inside.who,
                            actor_o: member.o,
                        });
                    }
                } else {
                    if ordinary.contained.is_some() {
                        return Err(ReturnPlanError::UnexpectedContainedFacts { index });
                    }
                    let lookup = ordinary
                        .airborne
                        .ok_or(ReturnPlanError::MissingAirborneFacts { index })?;
                    let (resolution, order) = match lookup {
                        ReturnAirLookup::UpdateOrderMissing { resolution } => {
                            if !matches!(resolution, ReturnAirResolution::UpdateOrder { .. })
                                || !valid_air_resolution(resolution)
                            {
                                return Err(ReturnPlanError::AirResolution { index });
                            }
                            continue;
                        }
                        ReturnAirLookup::AirOrderMissing { resolution } => (resolution, None),
                        ReturnAirLookup::Found { resolution, order } => (resolution, Some(order)),
                    };
                    if !valid_air_resolution(resolution) {
                        return Err(ReturnPlanError::AirResolution { index });
                    }
                    let Some(order) = order else {
                        continue;
                    };
                    effects.extend(reset_steps(group.who, member.o));
                    effects.extend([
                        ReturnEffect::AddStrafeOrder {
                            who: group.who,
                            o: member.o,
                            x: -1,
                            y: -1,
                            home_o: order.home_o,
                            home_who: order.home_who,
                            arg5: 0,
                            queue_pos: QUEUE_NEW,
                            arg7: 0,
                        },
                        ReturnEffect::UpdateOrder {
                            who: group.who,
                            o: member.o,
                        },
                        ReturnEffect::RestoreNewAirCruisingAltitude {
                            who: group.who,
                            o: member.o,
                            value: order.cruising_alt,
                        },
                        ReturnEffect::RestoreNewAirSharpTurn {
                            who: group.who,
                            o: member.o,
                            value: order.sharp_turn,
                        },
                    ]);
                }
            }
        }
    }

    Ok(ReturnPlan {
        group,
        effects,
        direct_rng_draws: 0,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnTransactionStatus {
    Applied,
    Unavailable,
}

/// Atomic receipt for the entire receiver.  Applied means the group after-image and every
/// ordered child effect were committed together; unavailable authorizes no state change.
#[derive(Clone, Debug, PartialEq)]
pub struct ReturnReceipt {
    pub request: ReturnRequest,
    pub status: ReturnTransactionStatus,
    pub facts: Option<ReturnFacts>,
    pub plan: Option<ReturnPlan>,
}

impl ReturnReceipt {
    pub fn unavailable(request: ReturnRequest) -> Self {
        Self {
            request,
            status: ReturnTransactionStatus::Unavailable,
            facts: None,
            plan: None,
        }
    }

    pub fn validates(&self, expected: &ReturnRequest) -> bool {
        if &self.request != expected {
            return false;
        }
        match self.status {
            ReturnTransactionStatus::Unavailable => self.facts.is_none() && self.plan.is_none(),
            ReturnTransactionStatus::Applied => {
                let (Some(facts), Some(observed)) = (self.facts.as_ref(), self.plan.as_ref())
                else {
                    return false;
                };
                plan_return(expected, facts).is_ok_and(|recomputed| recomputed == *observed)
            }
        }
    }
}
