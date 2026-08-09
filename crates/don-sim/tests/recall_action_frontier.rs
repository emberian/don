// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation and receipt pins for the isolated `Group::action_recall` frontier.

mod systems {
    pub mod groups_guys {
        pub use don_sim::systems::groups_guys::*;
    }
}

#[path = "../src/systems/recall_action_frontier.rs"]
mod subject;

use don_sim::systems::groups_guys::GroupData;
use subject::*;

fn group(members: &[i16], buildings: u8) -> GroupData {
    let mut group = GroupData {
        id: 31,
        num: members.len() as i32,
        form: 6,
        disband: 77,
        buildings,
        who: 2,
        ..GroupData::default()
    };
    group.list[..members.len()].copy_from_slice(members);
    group
}

fn request(members: &[i16], buildings: u8) -> RecallRequest {
    RecallRequest {
        group: group(members, buildings),
        ignore_orders: false,
        scenario_selection: Vec::new(),
    }
}

fn leader(group: &GroupData, domain: i32) -> RecallLeaderFacts {
    RecallLeaderFacts {
        source: if group.buildings != 0 {
            RecallLeaderSource::FirstMember
        } else {
            RecallLeaderSource::FindLeaderZero
        },
        o: i32::from(group.list[0]),
        domain: Some(domain),
    }
}

fn member(o: i16) -> RecallGroupMemberFacts {
    RecallGroupMemberFacts {
        o,
        valid_wall: false,
        is_build: None,
    }
}

fn base_facts(request: &RecallRequest) -> RecallFacts {
    RecallFacts {
        group_after_ignore_orders: request.group.clone(),
        leader: Some(leader(&request.group, 0)),
        group_members: request.group.list[..request.group.num as usize]
            .iter()
            .copied()
            .map(member)
            .collect(),
        owner_units: Vec::new(),
    }
}

fn target(o: i32, active: bool) -> RecallObjectRef {
    RecallObjectRef { o, who: 2, active }
}

fn current_air(home: RecallObjectRef) -> RecallAirOrderFacts {
    RecallAirOrderFacts {
        resolution: RecallAirResolution::UpdateOrder {
            head_present: false,
            first_order_type: None,
        },
        home,
        cruising_alt: 0x1234,
        sharp_turn: -9,
    }
}

fn candidate(
    inside: Option<RecallInsideFacts>,
    current_air: Option<RecallAirOrderFacts>,
) -> RecallOwnerUnitState {
    RecallOwnerUnitState::Candidate(RecallPlaneCandidateFacts {
        unit_flags_2b4: 0,
        object_masks_1e4: 0,
        inside,
        current_air,
    })
}

#[test]
fn retail_extent_and_single_byte_wire_are_pinned() {
    assert_eq!(GROUP_ACTION_RECALL_VA, 0x006f_a7e0);
    assert_eq!(GROUP_ACTION_RECALL_BYTES, 1_373);
    assert_eq!(GROUP_ACTION_RETURN_VA, 0x006f_ad40);
    assert_eq!(GROUP_ACTION_RETURN_BYTES, 1_307);
    assert_eq!(GROUP_FIND_LEADER_VA, 0x0070_ccb0);
    assert_eq!(GROUP_MEMBER_VA, 0x0070_f8f0);
    assert_eq!(UNIT_ADD_STRAFE_ORDER_VA, 0x005e_48c0);
    assert!(decode_recall(&[35]));
    assert!(!decode_recall(&[]));
    assert!(!decode_recall(&[35, 0]));
}

#[test]
fn scenario_kills_precede_the_empty_group_return() {
    let mut request = request(&[], 0);
    request.ignore_orders = true;
    request.scenario_selection = vec![7, -1, 9];
    let facts = RecallFacts {
        group_after_ignore_orders: request.group.clone(),
        leader: None,
        group_members: Vec::new(),
        owner_units: Vec::new(),
    };
    let plan = plan_recall(&request, &facts).unwrap();
    assert_eq!(plan.boundary, RecallBoundary::EmptyGroup);
    assert_eq!(plan.group, request.group);
    assert_eq!(plan.direct_rng_draws, 0);
    assert_eq!(
        plan.effects,
        vec![
            RecallEffect::ScenarioKill {
                target_o: 7,
                target_who: 2,
                tail_0: 0,
                tail_1: 0,
            },
            RecallEffect::ScenarioKill {
                target_o: 9,
                target_who: 2,
                tail_0: 0,
                tail_1: 0,
            },
        ]
    );
}

#[test]
fn selected_air_leader_is_an_unapplicable_return_tail() {
    let request = request(&[4], 0);
    let facts = RecallFacts {
        group_after_ignore_orders: request.group.clone(),
        leader: Some(leader(&request.group, AIR_DOMAIN)),
        group_members: Vec::new(),
        owner_units: Vec::new(),
    };
    let plan = plan_recall(&request, &facts).unwrap();
    assert_eq!(plan.group.disband, 77);
    assert_eq!(plan.boundary, RecallBoundary::OpenActionReturn);
    assert_eq!(
        plan.effects,
        vec![RecallEffect::OpenActionReturnTail { leader_o: 4 }]
    );

    let receipt = RecallReceipt {
        request: request.clone(),
        status: RecallTransactionStatus::Applied,
        facts: Some(facts),
        plan: Some(plan),
    };
    assert!(!receipt.validates(&request));
}

#[test]
fn leader_source_and_first_member_identity_fail_closed() {
    let request = request(&[4, 5], 1);
    let mut facts = base_facts(&request);
    facts.leader.as_mut().unwrap().source = RecallLeaderSource::FindLeaderZero;
    assert_eq!(
        plan_recall(&request, &facts),
        Err(RecallPlanError::LeaderSource)
    );

    facts.leader.as_mut().unwrap().source = RecallLeaderSource::FirstMember;
    facts.leader.as_mut().unwrap().o = 5;
    assert_eq!(
        plan_recall(&request, &facts),
        Err(RecallPlanError::LeaderIdentity)
    );
}

#[test]
fn main_body_orders_action_begin_gather_clears_then_form_reset() {
    let request = request(&[4, 5], 1);
    let mut facts = base_facts(&request);
    facts.group_members = vec![
        RecallGroupMemberFacts {
            o: 4,
            valid_wall: true,
            is_build: Some(true),
        },
        RecallGroupMemberFacts {
            o: 5,
            valid_wall: true,
            is_build: Some(false),
        },
    ];
    let plan = plan_recall(&request, &facts).unwrap();
    assert_eq!(plan.boundary, RecallBoundary::MainBody);
    assert_eq!(plan.group.disband, 0);
    assert_eq!(plan.group.form, -1);
    assert_eq!(
        plan.effects,
        vec![RecallEffect::ClearBuildGather { who: 2, o: 4 }]
    );
}

#[test]
fn aircraft_inside_selected_host_clears_orders_and_launching_membership() {
    let request = request(&[10], 1);
    let mut facts = base_facts(&request);
    facts.owner_units = vec![RecallOwnerUnitFacts {
        slot: 0,
        state: candidate(
            Some(RecallInsideFacts {
                target: target(10, true),
                launching: Some(RecallLaunchingFacts::Present {
                    contains_actor_slot: true,
                }),
            }),
            None,
        ),
    }];
    let plan = plan_recall(&request, &facts).unwrap();
    assert_eq!(
        plan.effects,
        vec![
            RecallEffect::ClearAircraftOrders { who: 2, o: 0 },
            RecallEffect::RemoveLaunchingSlot {
                host: target(10, true),
                actor_slot: 0,
            },
        ]
    );
}

#[test]
fn inactive_inside_target_falls_through_to_current_air_home() {
    let request = request(&[10], 1);
    let mut facts = base_facts(&request);
    facts.owner_units = vec![RecallOwnerUnitFacts {
        slot: 0,
        state: candidate(
            Some(RecallInsideFacts {
                target: target(10, false),
                launching: None,
            }),
            Some(current_air(target(99, true))),
        ),
    }];
    let plan = plan_recall(&request, &facts).unwrap();
    assert!(plan.effects.is_empty());
}

#[test]
fn selected_air_home_replaces_strafe_and_restores_two_air_fields() {
    let request = request(&[10], 1);
    let mut facts = base_facts(&request);
    let mut air = current_air(target(10, true));
    air.resolution = RecallAirResolution::AfterSpecialAnimation {
        first_order_type: ORDER_SPECIAL_ANIM,
    };
    facts.owner_units = vec![RecallOwnerUnitFacts {
        slot: 0,
        state: candidate(None, Some(air)),
    }];
    let plan = plan_recall(&request, &facts).unwrap();
    assert_eq!(
        plan.effects,
        vec![
            RecallEffect::SetExistingAirReturning {
                who: 2,
                o: 0,
                value: 1,
            },
            RecallEffect::ClearUnitMasks {
                who: 2,
                o: 0,
                mask: 0x0400_0000,
            },
            RecallEffect::ResetPathLength {
                who: 2,
                o: 0,
                value: 0,
            },
            RecallEffect::CloseOrders {
                who: 2,
                o: 0,
                arg: 0,
            },
            RecallEffect::ClearPartialPath { who: 2, o: 0 },
            RecallEffect::UpdateAction { who: 2, o: 0 },
            RecallEffect::AddStrafeOrder {
                who: 2,
                o: 0,
                x: -1,
                y: -1,
                home_o: 10,
                home_who: 2,
                arg5: 0,
                queue_pos: 2,
                arg7: 0,
            },
            RecallEffect::UpdateOrder { who: 2, o: 0 },
            RecallEffect::RestoreNewAirCruisingAltitude {
                who: 2,
                o: 0,
                value: 0x1234,
            },
            RecallEffect::RestoreNewAirSharpTurn {
                who: 2,
                o: 0,
                value: -9,
            },
        ]
    );
}

#[test]
fn post_is_plane_type_gates_and_owner_slot_are_strict() {
    let request = request(&[10], 1);
    let mut facts = base_facts(&request);
    facts.owner_units = vec![RecallOwnerUnitFacts {
        slot: 0,
        state: RecallOwnerUnitState::ExcludedPlane {
            unit_flags_2b4: 0,
            object_masks_1e4: 0,
        },
    }];
    assert_eq!(
        plan_recall(&request, &facts),
        Err(RecallPlanError::PlaneGate)
    );

    facts.owner_units[0] = RecallOwnerUnitFacts {
        slot: 4,
        state: RecallOwnerUnitState::Invalid,
    };
    assert_eq!(
        plan_recall(&request, &facts),
        Err(RecallPlanError::OwnerUnitSlot {
            expected: 0,
            got: 4,
        })
    );
}

#[test]
fn receipt_recomputes_every_effect_and_unavailable_is_mutation_free() {
    let request = request(&[10], 1);
    let mut facts = base_facts(&request);
    facts.owner_units = vec![RecallOwnerUnitFacts {
        slot: 0,
        state: candidate(None, Some(current_air(target(10, true)))),
    }];
    let plan = plan_recall(&request, &facts).unwrap();
    let mut receipt = RecallReceipt {
        request: request.clone(),
        status: RecallTransactionStatus::Applied,
        facts: Some(facts),
        plan: Some(plan),
    };
    assert!(receipt.validates(&request));
    receipt.plan.as_mut().unwrap().effects.pop();
    assert!(!receipt.validates(&request));

    let unavailable = RecallReceipt::unavailable(request.clone());
    assert!(unavailable.validates(&request));
}
