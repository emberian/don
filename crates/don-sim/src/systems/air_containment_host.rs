// SPDX-License-Identifier: GPL-3.0-or-later
//! The reference-host implementation of `Group::action_recall` `0x006FA7E0` and its
//! delegated `Group::action_return` `0x006FAD40`.
//!
//! [`crate::command::recall_action_frontier`] and [`crate::command::return_action_frontier`]
//! already carry the two recovered receiver bodies as recomputable plans; what was missing
//! was a host that actually **owns** the state those plans read and writes it back. This
//! module is that host: it adds the aircraft/containment columns retail reads
//! (`ObjectData::get_inside`, `ObjectData::launching`, the current `AirOrder`, the path
//! length, the `Build` gather list) to [`crate::command::ObjectTable`], captures the facts
//! from them, recomputes the plan, and commits every effect atomically.
//!
//! # What "atomic" means here
//!
//! [`apply_recall_action`] snapshots the whole object table plus this side table before it
//! writes anything, and restores the snapshot if any step cannot be applied. So a caller
//! either sees the complete recall (and, on the air-leader branch, the complete RETURN)
//! or sees nothing at all. That is the property `RecallActionReceipt::validates` is written
//! against: it refuses to publish recall's scenario prefix when the RETURN tail is absent.
//!
//! # `Build::clear_gather` `0x00623180`, 390 bytes [measured]
//!
//! `action_recall`'s group-member pass ends in this child, so the row cannot be complete
//! without it. The whole body is:
//!
//! ```text
//! while (this->gather_head (+0xCC) != 0) {          ; 0x006231A9..0x006231E5
//!     node = list.current; list.remove_current(); free(node);
//! }
//! if (this->build_masks (+0x60) & 8) {              ; 0x006231E7
//!     if (this->is(0x1BF, 0)) {                     ; 0x006231F3, devirtualized at 0x006231FA
//!         for (i = 0; i < players[who].num_units; i++) {          ; 0x00623215
//!             u = objects[who][i];
//!             if (!(u->flags(+8) & 1)) continue;                  ; 0x0062324C
//!             if (u->type->domain(+0x218) != 2) continue;         ; 0x00623259
//!             if (UnitData::home_base(u, &hw) != this->o) continue;; 0x0062326D
//!             if (hw != this->who) continue;                      ; 0x00623279
//!             if (u->is_on_map()) {                               ; 0x00623297, word +0x82 >> 15
//!                 add_strafe_order(u, -1, -1, this->o, this->who, 0, QUEUE_NEW, 0);
//!             } else {
//!                 Unit::clear_orders(u);                          ; 0x006232CA
//!                 if (this->launching (+0x44)) launching.remove(i);; 0x006232D7
//!             }
//!         }
//!     }
//! }
//! ```
//!
//! The argument order of the `add_strafe_order` call at `0x006232C3` is the push sequence
//! `-1, -1, o, who, 0, 2, 0` read bottom-up, which is the same seven-argument shape
//! `action_recall` and `action_return` install. `0x1BF` is the type-class selector passed
//! to `ObjectData::is` `0x00653790`, itself a two-instruction forwarder to the type
//! object's virtual `+0x60`.
//!
//! # Deliberate no-ops, and why they are not a gap in this row
//!
//! `RecallEffect::ClearPartialPath` / `UpdateAction` (and the RETURN equivalents) name
//! `Unit::clear_partial_path` `0x005E3920` and `Unit::update_action` `0x0060A870`. Those are
//! separate 674- and 485-byte receivers with their own owners; the reference object table
//! holds no partial-path or action column, so the effects are recorded in the receipt and
//! applied as no-ops. This is exactly the treatment the already-`Port::Complete`
//! `Group::action_stop_spell` row gives the same two children.
//!
//! Tier C. Nothing here has been executed against retail.

use std::collections::BTreeMap;

use crate::command::recall_action_frontier::{
    plan_recall, RecallAirOrderFacts, RecallAirResolution, RecallBoundary, RecallEffect,
    RecallFacts, RecallGroupMemberFacts, RecallInsideFacts, RecallLaunchingFacts,
    RecallLeaderFacts, RecallLeaderSource, RecallObjectRef, RecallOwnerUnitFacts,
    RecallOwnerUnitState, RecallPlaneCandidateFacts, RecallRequest, AIR_DOMAIN,
    HELICOPTER_TYPE_FLAG, MISSILE_OBJECT_MASK, ORDER_SPECIAL_ANIM,
};
use crate::command::return_action_frontier::{
    plan_return, ReturnAirLookup, ReturnAirOrderFacts, ReturnAirResolution, ReturnContainedFacts,
    ReturnEffect, ReturnFacts, ReturnInsideLookup, ReturnLaunchingFacts, ReturnMemberFacts,
    ReturnOrdinaryAircraftFacts, ReturnPlan, ReturnPlanePredicate, ReturnRequest, ReturnRouteFacts,
};
use crate::command::{
    ObjectTable, QueuePos, RecallActionReceipt, RecallActionRequest, RecallActionTransactionStatus,
};
use crate::systems::air::AirOrderWalk;
use crate::systems::groups_guys::{GroupData, GROUP_MAX_MEMBERS};
use crate::systems::order_dispatch::OrderRec;
use crate::systems::patrol::StrafeOrder;

/// `ObjectData::is` selector for the airbase class tested by `Build::clear_gather`
/// [measured, `push 0x1bf` at `0x006231F5`].
pub const BUILD_CLEAR_GATHER_AIRBASE_CLASS: i32 = 0x1bf;

/// `BuildData::build_masks & 8`, the gate on `Build::clear_gather`'s second arm
/// [measured, `test byte ptr [edi + 0x60], 8` at `0x006231E7`].
pub const BUILD_MASK_AIRBASE_GATHER: u16 = 0x08;

pub const BUILD_CLEAR_GATHER_VA: u32 = 0x0062_3180;
pub const BUILD_CLEAR_GATHER_BYTES: usize = 390;
pub const UNIT_HOME_BASE_VA: u32 = 0x0060_9dc0;
pub const UNIT_CLEAR_ORDERS_VA: u32 = 0x005e_3860;
pub const OBJECT_DATA_IS_VA: u32 = 0x0065_3790;

/// The aircraft/containment columns `Group::action_recall`, `Group::action_return` and
/// `Build::clear_gather` read, for one object.
///
/// Every field is a state column of the shipped object, not a derived convenience: a host
/// that leaves one at its default is asserting that column's zero value, not "unknown".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AirObject {
    /// Object virtual `+0x0C` (`is_valid_wall`), the first predicate of recall's
    /// group-member pass.
    pub valid_wall: bool,
    /// Object virtual `+0x20` (`is_build`), read only after `valid_wall` succeeds.
    pub is_build: bool,
    /// `ObjectTypeData::obj_masks` at `+0x1E4`. Bit `0x08000000` excludes missiles.
    pub obj_masks: u32,
    /// `ObjectData::get_inside(&who)` `0x00651A80` — the containing object, or `None` for
    /// retail's negative return.
    pub inside: Option<(i32, i32)>,
    /// `ObjectData::launching` at `+0x44`. `None` is retail's null pointer; an empty vector
    /// is a present but empty array, and the two are not interchangeable.
    pub launching: Option<Vec<i32>>,
    /// The `AirOrder` subobject reached through `Unit::update_order()` /
    /// `UnitOrder::get_air_order()`.
    pub air: Option<AirOrderWalk>,
    /// `UnitData::path.length` at `Unit+0xC0`.
    pub path_length: i32,
    /// `Build`'s `GatherPoint` linked list at `+0xCC`, drained by `Build::clear_gather`.
    pub gather_points: Vec<i32>,
    /// `UnitData::home_base` `0x00609DC0` result `(o, who)`, a pure query over the order
    /// list. `None` is its negative return.
    pub home_base: Option<(i32, i32)>,
    /// `ObjectData::is(0x1BF, 0)` — the airbase class gate in `Build::clear_gather`.
    pub airbase_class: bool,
}

/// Scenario and per-object columns owned by the reference host.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AirWorld {
    objects: BTreeMap<(u8, i16), AirObject>,
    /// Ordered evidence of the presentation/child calls the reference table records but
    /// does not model as state.
    unmodelled: Vec<UnmodelledChild>,
}

/// A child receiver whose body belongs to another owner and whose state column this
/// reference table does not hold. Recorded so a test can prove the effect was reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnmodelledChild {
    /// `Unit::clear_partial_path` `0x005E3920`.
    ClearPartialPath { who: u8, o: i32 },
    /// `Unit::update_action` `0x0060A870`.
    UpdateAction { who: u8, o: i32 },
}

impl AirWorld {
    pub fn get(&self, who: u8, o: i16) -> Option<&AirObject> {
        self.objects.get(&(who, o))
    }

    pub fn get_mut(&mut self, who: u8, o: i16) -> Option<&mut AirObject> {
        self.objects.get_mut(&(who, o))
    }

    pub fn entry(&mut self, who: u8, o: i16) -> &mut AirObject {
        self.objects.entry((who, o)).or_default()
    }

    pub fn put(&mut self, who: u8, o: i16, object: AirObject) {
        self.objects.insert((who, o), object);
    }

    pub fn unmodelled(&self) -> &[UnmodelledChild] {
        &self.unmodelled
    }

    pub fn take_unmodelled(&mut self) -> Vec<UnmodelledChild> {
        std::mem::take(&mut self.unmodelled)
    }

    fn column(&self, who: u8, o: i16) -> AirObject {
        self.objects.get(&(who, o)).cloned().unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Fact capture
// ---------------------------------------------------------------------------

fn member_slice(group: &GroupData) -> &[i16] {
    let n = group.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
    &group.list[..n]
}

/// `GroupData::member(o, who, 1)` `0x0070F8F0`: matching owner, the object's active bit
/// `+0x08 & 1`, and an exact hit in the selected list.
fn group_member(table: &ObjectTable, group: &GroupData, o: i32, who: i32) -> bool {
    o >= 0
        && who == i32::from(group.who)
        && i16::try_from(o).is_ok_and(|o| {
            table
                .get(group.who, o)
                .is_some_and(|slot| slot.object_flags & 1 != 0)
                && member_slice(group).contains(&o)
        })
}

/// `GroupData::find_leader(0)` `0x0070CCB0`: the lowest `FormCatIndex` over on-map captains,
/// retried without the on-map predicate. Same derivation as `Action::form_leader`, over an
/// arbitrary receiver group rather than the bridge's addressed slot.
fn find_leader(table: &ObjectTable, group: &GroupData) -> i32 {
    for require_on_map in [true, false] {
        let mut leader: Option<i16> = None;
        let mut best_category = 18;
        for &o in member_slice(group) {
            let Some(slot) = table.get(group.who, o) else {
                continue;
            };
            if !slot.alive || !slot.is_captain || (require_on_map && !slot.is_on_map) {
                continue;
            }
            if leader.is_none() || slot.form_category < best_category {
                leader = Some(o);
                best_category = slot.form_category;
            }
        }
        if let Some(o) = leader {
            return i32::from(o);
        }
    }
    -1
}

/// The linked-list head order type used by both receivers to decide whether retail
/// advances one node past a `SPECIAL_ANIM` before calling `get_air_order()`.
fn head_order_type(table: &ObjectTable, who: u8, o: i16) -> Option<i32> {
    let slot = table.get(who, o)?;
    slot.orders.front().map(|order| order.kind as i32)
}

fn recall_air_resolution(head: Option<i32>) -> RecallAirResolution {
    match head {
        Some(ORDER_SPECIAL_ANIM) => RecallAirResolution::AfterSpecialAnimation {
            first_order_type: ORDER_SPECIAL_ANIM,
        },
        Some(ty) => RecallAirResolution::UpdateOrder {
            head_present: true,
            first_order_type: Some(ty),
        },
        None => RecallAirResolution::UpdateOrder {
            head_present: false,
            first_order_type: None,
        },
    }
}

fn return_air_resolution(head: Option<i32>) -> ReturnAirResolution {
    match head {
        Some(ORDER_SPECIAL_ANIM) => ReturnAirResolution::AfterSpecialAnimation {
            first_order_type: ORDER_SPECIAL_ANIM,
        },
        Some(ty) => ReturnAirResolution::UpdateOrder {
            head_present: true,
            first_order_type: Some(ty),
        },
        None => ReturnAirResolution::UpdateOrder {
            head_present: false,
            first_order_type: None,
        },
    }
}

/// Snapshot `RecallFacts` from live table state.
///
/// `ignore_orders` is `false` and the scenario selection is empty because the reference
/// table holds no `ScenarioData`; that is also the value in ordinary multiplayer and
/// product execution, and the planner rejects any fact set that disagrees.
pub fn recall_facts(table: &ObjectTable, group: &GroupData, per_owner: usize) -> RecallFacts {
    let leader = (group.num > 0).then(|| {
        let (source, o) = if group.buildings != 0 {
            (RecallLeaderSource::FirstMember, i32::from(group.list[0]))
        } else {
            (
                RecallLeaderSource::FindLeaderZero,
                find_leader(table, group),
            )
        };
        RecallLeaderFacts {
            source,
            o,
            domain: (o >= 0).then(|| {
                i16::try_from(o)
                    .ok()
                    .and_then(|o| table.get(group.who, o))
                    .map_or(0, |slot| slot.domain)
            }),
        }
    });

    // Retail reads the group-member and owner-unit passes only when the leader is not an
    // air-domain object; the return delegate happens first and returns.
    let air_leader = leader.is_some_and(|leader| leader.domain == Some(AIR_DOMAIN));
    if group.num <= 0 || air_leader {
        return RecallFacts {
            group_after_ignore_orders: group.clone(),
            leader,
            group_members: Vec::new(),
            owner_units: Vec::new(),
        };
    }

    let group_members = member_slice(group)
        .iter()
        .map(|&o| {
            let column = table.air.column(group.who, o);
            RecallGroupMemberFacts {
                o,
                valid_wall: column.valid_wall,
                is_build: column.valid_wall.then_some(column.is_build),
            }
        })
        .collect();

    // `action_begin` runs before the owner-unit scan, so membership is tested against the
    // group the scan actually sees.
    let mut scanned = group.clone();
    scanned.disband = 0;
    let owner_units = (0..per_owner)
        .map(|index| RecallOwnerUnitFacts {
            slot: index as i32,
            state: recall_owner_unit_state(table, &scanned, index as i16),
        })
        .collect();

    RecallFacts {
        group_after_ignore_orders: group.clone(),
        leader,
        group_members,
        owner_units,
    }
}

fn recall_owner_unit_state(table: &ObjectTable, group: &GroupData, o: i16) -> RecallOwnerUnitState {
    let Some(slot) = table.get(group.who, o) else {
        return RecallOwnerUnitState::Invalid;
    };
    if !slot.alive || !slot.is_unit {
        return RecallOwnerUnitState::Invalid;
    }
    if !slot.is_plane {
        return RecallOwnerUnitState::NotPlane;
    }
    let column = table.air.column(group.who, o);
    if slot.unit_flags & HELICOPTER_TYPE_FLAG != 0 || column.obj_masks & MISSILE_OBJECT_MASK != 0 {
        return RecallOwnerUnitState::ExcludedPlane {
            unit_flags_2b4: slot.unit_flags,
            object_masks_1e4: column.obj_masks,
        };
    }

    let inside = column.inside.map(|(inside_o, inside_who)| {
        let selected = group_member(table, group, inside_o, inside_who);
        RecallInsideFacts {
            target: RecallObjectRef {
                o: inside_o,
                who: inside_who,
                active: i16::try_from(inside_o).ok().is_some_and(|inside_o| {
                    u8::try_from(inside_who).ok().is_some_and(|inside_who| {
                        table
                            .get(inside_who, inside_o)
                            .is_some_and(|slot| slot.object_flags & 1 != 0)
                    })
                }),
            },
            launching: selected.then(|| match column.launching.as_ref() {
                None => RecallLaunchingFacts::Null,
                Some(list) => RecallLaunchingFacts::Present {
                    contains_actor_slot: list.contains(&i32::from(o)),
                },
            }),
        }
    });
    let selected_inside = inside.is_some_and(|inside| {
        group_member(table, group, inside.target.o, inside.target.who) && inside.target.active
    });

    let current_air = (!selected_inside).then(|| {
        let resolution = recall_air_resolution(head_order_type(table, group.who, o));
        match column.air.as_ref() {
            Some(air) => RecallAirOrderFacts {
                resolution,
                home: RecallObjectRef {
                    o: air.oxx,
                    who: air.whose,
                    active: group_member(table, group, air.oxx, air.whose),
                },
                cruising_alt: air.cruising_alt,
                sharp_turn: air.sharp_turn,
            },
            // No AirOrder subobject exists, so retail resolves no home pair and the actor
            // is skipped by the `GroupData::member` gate rather than mutated.
            None => RecallAirOrderFacts {
                resolution,
                home: RecallObjectRef {
                    o: -1,
                    who: -1,
                    active: false,
                },
                cruising_alt: 0,
                sharp_turn: 0,
            },
        }
    });

    RecallOwnerUnitState::Candidate(RecallPlaneCandidateFacts {
        unit_flags_2b4: slot.unit_flags,
        object_masks_1e4: column.obj_masks,
        inside,
        current_air,
    })
}

/// Snapshot `ReturnFacts` from live table state, for the group RECALL hands to
/// `Group::action_return`.
pub fn return_facts(table: &ObjectTable, group: &GroupData) -> ReturnFacts {
    let members = member_slice(group)
        .iter()
        .map(|&o| return_member_facts(table, group, o))
        .collect();
    ReturnFacts {
        group_after_ignore_orders: group.clone(),
        members,
    }
}

fn return_member_facts(table: &ObjectTable, group: &GroupData, o: i16) -> ReturnMemberFacts {
    let Some(slot) = table.get(group.who, o) else {
        return ReturnMemberFacts {
            o,
            valid_unit: false,
            plane: None,
            route: None,
        };
    };
    if !slot.alive || !slot.is_unit {
        return ReturnMemberFacts {
            o,
            valid_unit: false,
            plane: None,
            route: None,
        };
    }
    let column = table.air.column(group.who, o);

    // The reference table exposes no overridden `is_plane` slot: every object resolves to
    // the concrete `UnitData::is_plane` `0x0046CE40`, which is the pointer-equality arm at
    // `0x006FAE32`.
    let concrete_plane = slot.domain == AIR_DOMAIN && slot.unit_flags & HELICOPTER_TYPE_FLAG == 0;
    let plane = ReturnPlanePredicate::Concrete {
        domain_218: slot.domain,
        unit_flags_2b4: slot.unit_flags,
        object_masks_1e4: concrete_plane.then_some(column.obj_masks),
    };

    let route = if concrete_plane {
        (column.obj_masks & MISSILE_OBJECT_MASK == 0).then(|| {
            ReturnRouteFacts::OrdinaryAircraft(return_ordinary_facts(table, group, o, &column))
        })
    } else if slot.unit_flags & HELICOPTER_TYPE_FLAG != 0 {
        Some(ReturnRouteFacts::Helicopter {
            on_map: slot.is_on_map,
        })
    } else {
        None
    };

    ReturnMemberFacts {
        o,
        valid_unit: true,
        plane: Some(plane),
        route,
    }
}

fn return_ordinary_facts(
    table: &ObjectTable,
    group: &GroupData,
    o: i16,
    column: &AirObject,
) -> ReturnOrdinaryAircraftFacts {
    match column.inside {
        Some((inside_o, inside_who)) => {
            // Retail re-reads `get_inside` after the destructive reset; nothing between the
            // two calls edits containment, so the reference table observes the same pair.
            let launching = match table
                .air
                .get(
                    u8::try_from(inside_who).unwrap_or(u8::MAX),
                    i16::try_from(inside_o).unwrap_or(-1),
                )
                .and_then(|host| host.launching.as_ref())
            {
                None => ReturnLaunchingFacts::Null,
                Some(list) => ReturnLaunchingFacts::Present {
                    contains_actor_o: list.contains(&i32::from(o)),
                },
            };
            ReturnOrdinaryAircraftFacts {
                first_inside: ReturnInsideLookup {
                    o: inside_o,
                    who: inside_who,
                },
                contained: Some(ReturnContainedFacts {
                    second_inside: ReturnInsideLookup {
                        o: inside_o,
                        who: inside_who,
                    },
                    launching,
                }),
                airborne: None,
            }
        }
        None => {
            let resolution = return_air_resolution(head_order_type(table, group.who, o));
            let airborne = match column.air.as_ref() {
                Some(air) => ReturnAirLookup::Found {
                    resolution,
                    order: ReturnAirOrderFacts {
                        home_o: air.oxx,
                        home_who: air.whose,
                        cruising_alt: air.cruising_alt,
                        sharp_turn: air.sharp_turn,
                    },
                },
                None => ReturnAirLookup::AirOrderMissing { resolution },
            };
            ReturnOrdinaryAircraftFacts {
                first_inside: ReturnInsideLookup { o: -1, who: -1 },
                contained: None,
                airborne: Some(airborne),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Effect application
// ---------------------------------------------------------------------------

fn strafe_order_rec(home_o: i32, home_who: i32, mandatory: i32) -> OrderRec {
    OrderRec::strafe(StrafeOrder {
        target_o: home_o,
        target_who: home_who,
        mandatory: u8::from(mandatory != 0),
        ..StrafeOrder::default()
    })
}

fn install_strafe(table: &mut ObjectTable, who: u8, o: i16, order: OrderRec) -> bool {
    let Some(slot) = table.get_mut(who, o) else {
        return false;
    };
    slot.orders.clear();
    slot.orders.push_back(order);
    true
}

/// `Build::clear_gather` `0x00623180`, complete.
///
/// Returns `false` when a modelled step cannot be applied, which fails the whole
/// transaction closed rather than committing a partial child.
fn apply_build_clear_gather(table: &mut ObjectTable, who: u8, o: i16, per_owner: usize) -> bool {
    let Some(column) = table.air.get_mut(who, o) else {
        // No gather list and no airbase columns: the drain loop makes zero passes and the
        // `build_masks & 8` gate cannot be reached without a Build column.
        return table.get(who, o).is_some();
    };
    column.gather_points.clear();
    let airbase = column.airbase_class;
    let Some(build) = table.get(who, o) else {
        return false;
    };
    let base_o = i32::from(o);
    if build.build_masks & BUILD_MASK_AIRBASE_GATHER == 0 || !airbase {
        return true;
    }

    for index in 0..per_owner {
        let actor = index as i16;
        let Some(slot) = table.get(who, actor) else {
            continue;
        };
        if slot.object_flags & 1 == 0 || slot.domain != AIR_DOMAIN {
            continue;
        }
        let on_map = slot.is_on_map;
        let Some((home_o, home_who)) = table.air.column(who, actor).home_base else {
            continue;
        };
        if home_o != base_o || home_who != i32::from(who) {
            continue;
        }
        if on_map {
            if !install_strafe(
                table,
                who,
                actor,
                strafe_order_rec(base_o, i32::from(who), 0),
            ) {
                return false;
            }
        } else {
            let Some(slot) = table.get_mut(who, actor) else {
                return false;
            };
            slot.orders.clear();
            if let Some(list) = table
                .air
                .get_mut(who, o)
                .and_then(|host| host.launching.as_mut())
            {
                list.retain(|&slot| slot != index as i32);
            }
        }
    }
    true
}

fn apply_recall_effect(table: &mut ObjectTable, effect: &RecallEffect, per_owner: usize) -> bool {
    match *effect {
        // Not reachable on this host: `ScenarioData::ignore_orders` is zero, so the planner
        // emits no kills and rejects any fact set claiming otherwise.
        RecallEffect::ScenarioKill { .. } | RecallEffect::OpenActionReturnTail { .. } => false,
        RecallEffect::ClearBuildGather { who, o } => {
            apply_build_clear_gather(table, who, o, per_owner)
        }
        RecallEffect::ClearAircraftOrders { who, o } => {
            let Ok(o) = i16::try_from(o) else {
                return false;
            };
            let Some(slot) = table.get_mut(who, o) else {
                return false;
            };
            slot.orders.clear();
            true
        }
        RecallEffect::RemoveLaunchingSlot { host, actor_slot } => {
            let (Ok(host_who), Ok(host_o)) = (u8::try_from(host.who), i16::try_from(host.o)) else {
                return false;
            };
            let Some(list) = table
                .air
                .get_mut(host_who, host_o)
                .and_then(|host| host.launching.as_mut())
            else {
                return false;
            };
            list.retain(|&slot| slot != actor_slot);
            true
        }
        RecallEffect::SetExistingAirReturning { who, o, value } => {
            let Ok(o) = i16::try_from(o) else {
                return false;
            };
            let Some(air) = table.air.get_mut(who, o).and_then(|c| c.air.as_mut()) else {
                return false;
            };
            air.returning = value;
            true
        }
        RecallEffect::ClearUnitMasks { who, o, mask } => {
            let Ok(o) = i16::try_from(o) else {
                return false;
            };
            let Some(slot) = table.get_mut(who, o) else {
                return false;
            };
            slot.unit_masks &= !mask;
            true
        }
        RecallEffect::ResetPathLength { who, o, value } => {
            let Ok(o) = i16::try_from(o) else {
                return false;
            };
            table.air.entry(who, o).path_length = value;
            true
        }
        RecallEffect::CloseOrders { who, o, .. } => {
            let Ok(o) = i16::try_from(o) else {
                return false;
            };
            let Some(slot) = table.get_mut(who, o) else {
                return false;
            };
            slot.orders.clear();
            true
        }
        RecallEffect::ClearPartialPath { who, o } => {
            table
                .air
                .unmodelled
                .push(UnmodelledChild::ClearPartialPath { who, o });
            true
        }
        RecallEffect::UpdateAction { who, o } => {
            table
                .air
                .unmodelled
                .push(UnmodelledChild::UpdateAction { who, o });
            true
        }
        RecallEffect::AddStrafeOrder {
            who,
            o,
            home_o,
            home_who,
            arg5,
            queue_pos,
            ..
        } => {
            let Ok(o) = i16::try_from(o) else {
                return false;
            };
            if QueuePos::from_i64(i64::from(queue_pos)) != QueuePos::New {
                return false;
            }
            if !install_strafe(table, who, o, strafe_order_rec(home_o, home_who, arg5)) {
                return false;
            }
            let column = table.air.entry(who, o);
            let previous = column.air.unwrap_or_default();
            column.air = Some(AirOrderWalk {
                oxx: home_o,
                whose: home_who,
                returning: previous.returning,
                ..AirOrderWalk::default()
            });
            true
        }
        // `Unit::update_order()` only re-resolves the current node; the replacement order
        // installed above is already the head.
        RecallEffect::UpdateOrder { .. } => true,
        RecallEffect::RestoreNewAirCruisingAltitude { who, o, value } => {
            let Ok(o) = i16::try_from(o) else {
                return false;
            };
            let Some(air) = table.air.get_mut(who, o).and_then(|c| c.air.as_mut()) else {
                return false;
            };
            air.cruising_alt = value;
            true
        }
        RecallEffect::RestoreNewAirSharpTurn { who, o, value } => {
            let Ok(o) = i16::try_from(o) else {
                return false;
            };
            let Some(air) = table.air.get_mut(who, o).and_then(|c| c.air.as_mut()) else {
                return false;
            };
            air.sharp_turn = value;
            true
        }
    }
}

fn apply_return_effect(table: &mut ObjectTable, effect: &ReturnEffect) -> bool {
    match *effect {
        // `Group::action_begin`'s `disband = 0` and the scenario prelude are published
        // through the plan's group after-image, not per-object.
        ReturnEffect::ActionBegin { .. } => true,
        ReturnEffect::ScenarioKill { .. } => false,
        ReturnEffect::ClearUnitMasks { who, o, mask } => {
            let Some(slot) = table.get_mut(who, o) else {
                return false;
            };
            slot.unit_masks &= !mask;
            true
        }
        ReturnEffect::ResetPathLength { who, o, value } => {
            table.air.entry(who, o).path_length = value;
            true
        }
        ReturnEffect::CloseOrders { who, o, .. } => {
            let Some(slot) = table.get_mut(who, o) else {
                return false;
            };
            slot.orders.clear();
            true
        }
        ReturnEffect::ClearPartialPath { who, o } => {
            table
                .air
                .unmodelled
                .push(UnmodelledChild::ClearPartialPath {
                    who,
                    o: i32::from(o),
                });
            true
        }
        ReturnEffect::UpdateAction { who, o } => {
            table.air.unmodelled.push(UnmodelledChild::UpdateAction {
                who,
                o: i32::from(o),
            });
            true
        }
        ReturnEffect::RemoveLaunchingObject {
            host_o,
            host_who,
            actor_o,
        } => {
            let (Ok(host_who), Ok(host_o)) = (u8::try_from(host_who), i16::try_from(host_o)) else {
                return false;
            };
            let Some(list) = table
                .air
                .get_mut(host_who, host_o)
                .and_then(|host| host.launching.as_mut())
            else {
                return false;
            };
            list.retain(|&slot| slot != i32::from(actor_o));
            true
        }
        ReturnEffect::AddStrafeOrder {
            who,
            o,
            home_o,
            home_who,
            arg5,
            queue_pos,
            ..
        } => {
            if QueuePos::from_i64(i64::from(queue_pos)) != QueuePos::New {
                return false;
            }
            if !install_strafe(table, who, o, strafe_order_rec(home_o, home_who, arg5)) {
                return false;
            }
            let column = table.air.entry(who, o);
            let previous = column.air.unwrap_or_default();
            column.air = Some(AirOrderWalk {
                oxx: home_o,
                whose: home_who,
                returning: previous.returning,
                ..AirOrderWalk::default()
            });
            true
        }
        ReturnEffect::UpdateOrder { .. } => true,
        ReturnEffect::RestoreNewAirCruisingAltitude { who, o, value } => {
            let Some(air) = table.air.get_mut(who, o).and_then(|c| c.air.as_mut()) else {
                return false;
            };
            air.cruising_alt = value;
            true
        }
        ReturnEffect::RestoreNewAirSharpTurn { who, o, value } => {
            let Some(air) = table.air.get_mut(who, o).and_then(|c| c.air.as_mut()) else {
                return false;
            };
            air.sharp_turn = value;
            true
        }
    }
}

// ---------------------------------------------------------------------------
// The transaction
// ---------------------------------------------------------------------------

/// Execute `Group::action_recall` and, on the air-leader branch, `Group::action_return`,
/// as one atomic transaction over the reference object table.
///
/// The complete plan is recomputed and every effect preflighted against a snapshot; any
/// step this host cannot apply restores the snapshot and returns `Unavailable`, so no
/// partial receiver is ever published.
pub fn apply_recall_action(
    table: &mut ObjectTable,
    request: RecallActionRequest,
    per_owner: usize,
) -> RecallActionReceipt {
    let group = request.group.clone();
    let recall_request = RecallRequest {
        group: group.clone(),
        ignore_orders: false,
        scenario_selection: Vec::new(),
    };
    let facts = recall_facts(table, &group, per_owner);
    let Ok(plan) = plan_recall(&recall_request, &facts) else {
        return RecallActionReceipt::unavailable(request);
    };

    let checkpoint = table.clone();
    let mut committed: Option<(ReturnFacts, ReturnPlan)> = None;
    let mut ok = true;
    for effect in &plan.effects {
        if matches!(effect, RecallEffect::OpenActionReturnTail { .. }) {
            continue;
        }
        if !apply_recall_effect(table, effect, per_owner) {
            ok = false;
            break;
        }
    }

    if ok && plan.boundary == RecallBoundary::OpenActionReturn {
        let return_request = ReturnRequest {
            group: plan.group.clone(),
            ignore_orders: recall_request.ignore_orders,
            scenario_selection: recall_request.scenario_selection.clone(),
        };
        // `action_return` calls `action_begin` before it reads the scenario global, so the
        // group its member pass sees already has `disband` cleared.
        let mut begun = return_request.group.clone();
        begun.disband = 0;
        let return_fact_set = return_facts(table, &begun);
        match plan_return(&return_request, &return_fact_set) {
            Ok(return_plan) => {
                for effect in &return_plan.effects {
                    if !apply_return_effect(table, effect) {
                        ok = false;
                        break;
                    }
                }
                committed = Some((return_fact_set, return_plan));
            }
            Err(_) => ok = false,
        }
    }

    if !ok {
        *table = checkpoint;
        return RecallActionReceipt::unavailable(request);
    }

    let (return_facts_out, return_plan) = match committed {
        Some((facts, plan)) => (Some(facts), Some(plan)),
        None => (None, None),
    };
    RecallActionReceipt {
        request,
        status: RecallActionTransactionStatus::Applied,
        recall_request: Some(recall_request),
        recall_facts: Some(facts),
        recall_plan: Some(plan),
        return_facts: return_facts_out,
        return_plan,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Slot;

    /// The rollback in [`apply_recall_action`] is a safety net, not a live branch: on this
    /// host every effect the planner can emit is applicable, which is precisely why the row
    /// can be `Port::Complete`. Pin both halves of that claim — otherwise "atomic" would be
    /// an untested word.
    #[test]
    fn every_emitted_effect_applies_and_the_two_unreachable_variants_fail_closed() {
        let mut table = ObjectTable::new(2);
        let mut base = Slot::building(1, 48, 48);
        base.build_masks = BUILD_MASK_AIRBASE_GATHER;
        table.put(2, 0, base);
        let mut aircraft = Slot::plane(2, 400, 400);
        aircraft.domain = AIR_DOMAIN;
        table.put(2, 1, aircraft);
        table.set_air_object(
            2,
            0,
            AirObject {
                valid_wall: true,
                is_build: true,
                airbase_class: true,
                launching: Some(vec![1]),
                gather_points: vec![5],
                ..AirObject::default()
            },
        );
        table.set_air_object(
            2,
            1,
            AirObject {
                air: Some(AirOrderWalk {
                    oxx: 0,
                    whose: 2,
                    cruising_alt: 0x640,
                    sharp_turn: 3,
                    ..AirOrderWalk::default()
                }),
                home_base: Some((0, 2)),
                ..AirObject::default()
            },
        );

        let mut group = GroupData {
            who: 2,
            num: 1,
            buildings: 1,
            ..GroupData::default()
        };
        group.list[0] = 0;
        let request = RecallRequest {
            group: group.clone(),
            ignore_orders: false,
            scenario_selection: Vec::new(),
        };
        let facts = recall_facts(&table, &group, 2);
        let plan = plan_recall(&request, &facts).expect("main-body plan");
        assert_eq!(plan.boundary, RecallBoundary::MainBody);
        assert!(
            plan.effects.len() > 1,
            "the fixture must reach the aircraft scan, not just clear_gather"
        );

        for effect in &plan.effects {
            assert!(
                apply_recall_effect(&mut table, effect, 2),
                "unapplicable effect on the reference host: {effect:?}"
            );
        }

        // The two variants the planner cannot emit here must still refuse rather than
        // silently succeed, so the checkpoint restore stays reachable if that ever changes.
        assert!(!apply_recall_effect(
            &mut table,
            &RecallEffect::ScenarioKill {
                target_o: 0,
                target_who: 2,
                tail_0: 0,
                tail_1: 0,
            },
            2
        ));
        assert!(!apply_recall_effect(
            &mut table,
            &RecallEffect::OpenActionReturnTail { leader_o: 0 },
            2
        ));
    }

    /// `Build::clear_gather`'s drain loop runs unconditionally; its second arm does not.
    #[test]
    fn clear_gather_always_drains_but_only_rebases_behind_both_gates() {
        for (mask, class, expect_orders) in [
            (BUILD_MASK_AIRBASE_GATHER, true, 1usize),
            (0, true, 0),
            (BUILD_MASK_AIRBASE_GATHER, false, 0),
        ] {
            let mut table = ObjectTable::new(2);
            let mut base = Slot::building(1, 48, 48);
            base.build_masks = mask;
            table.put(2, 0, base);
            let mut aircraft = Slot::plane(2, 400, 400);
            aircraft.domain = AIR_DOMAIN;
            table.put(2, 1, aircraft);
            table.set_air_object(
                2,
                0,
                AirObject {
                    airbase_class: class,
                    gather_points: vec![5, 6, 7],
                    ..AirObject::default()
                },
            );
            table.set_air_object(
                2,
                1,
                AirObject {
                    home_base: Some((0, 2)),
                    ..AirObject::default()
                },
            );

            assert!(apply_build_clear_gather(&mut table, 2, 0, 2));
            assert!(table.air_object(2, 0).unwrap().gather_points.is_empty());
            assert_eq!(
                table.get(2, 1).unwrap().orders.len(),
                expect_orders,
                "mask={mask:#x} class={class}"
            );
        }
    }
}
