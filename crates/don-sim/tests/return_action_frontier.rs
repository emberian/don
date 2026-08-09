// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation and atomic-receipt pins for `Group::action_return`.

mod systems {
    pub mod groups_guys {
        pub use don_sim::systems::groups_guys::*;
    }
}

#[path = "../src/systems/return_action_frontier.rs"]
mod subject;

use don_sim::systems::groups_guys::GroupData;
use subject::*;

fn group(members: &[i16]) -> GroupData {
    let mut group = GroupData {
        id: 44,
        num: members.len() as i32,
        form: 3,
        disband: 77,
        who: 2,
        ..GroupData::default()
    };
    group.list[..members.len()].copy_from_slice(members);
    group
}

fn request(members: &[i16]) -> ReturnRequest {
    ReturnRequest {
        group: group(members),
        ignore_orders: false,
        scenario_selection: Vec::new(),
    }
}

fn after_action_begin(request: &ReturnRequest) -> GroupData {
    let mut group = request.group.clone();
    group.disband = 0;
    group
}

fn ordinary_predicate() -> ReturnPlanePredicate {
    ReturnPlanePredicate::Concrete {
        domain_218: AIR_DOMAIN,
        unit_flags_2b4: 0,
        object_masks_1e4: Some(0),
    }
}

fn update_resolution() -> ReturnAirResolution {
    ReturnAirResolution::UpdateOrder {
        head_present: false,
        first_order_type: None,
    }
}

fn ordinary_member(o: i16, route: ReturnOrdinaryAircraftFacts) -> ReturnMemberFacts {
    ReturnMemberFacts {
        o,
        valid_unit: true,
        plane: Some(ordinary_predicate()),
        route: Some(ReturnRouteFacts::OrdinaryAircraft(route)),
    }
}

fn facts(request: &ReturnRequest, members: Vec<ReturnMemberFacts>) -> ReturnFacts {
    ReturnFacts {
        group_after_ignore_orders: after_action_begin(request),
        members,
    }
}

fn reset_effects(o: i16) -> Vec<ReturnEffect> {
    vec![
        ReturnEffect::ClearUnitMasks {
            who: 2,
            o,
            mask: UNIT_MASK_RETURN_CLEAR,
        },
        ReturnEffect::ResetPathLength {
            who: 2,
            o,
            value: 0,
        },
        ReturnEffect::CloseOrders { who: 2, o, arg: 0 },
        ReturnEffect::ClearPartialPath { who: 2, o },
        ReturnEffect::UpdateAction { who: 2, o },
    ]
}

fn action_begin() -> ReturnEffect {
    ReturnEffect::ActionBegin { disband: 0 }
}

#[test]
fn retail_extent_and_direct_children_are_pinned() {
    assert_eq!(GROUP_ACTION_RETURN_VA, 0x006f_ad40);
    assert_eq!(GROUP_ACTION_RETURN_BYTES, 1_307);
    assert_eq!(UNIT_DATA_IS_PLANE_VA, 0x0046_ce40);
    assert_eq!(OBJECT_GET_INSIDE_VA, 0x0065_1a80);
    assert_eq!(UNIT_CLOSE_ORDERS_VA, 0x005e_37f0);
    assert_eq!(UNIT_CLEAR_PARTIAL_PATH_VA, 0x005e_3920);
    assert_eq!(UNIT_UPDATE_ACTION_VA, 0x0060_a870);
    assert_eq!(UNIT_ADD_STRAFE_ORDER_VA, 0x005e_48c0);
}

#[test]
fn action_begin_precedes_scenario_kills_and_empty_return() {
    let mut request = request(&[]);
    request.ignore_orders = true;
    request.scenario_selection = vec![-1, 8, 12];
    let facts = ReturnFacts {
        group_after_ignore_orders: after_action_begin(&request),
        members: Vec::new(),
    };
    let plan = plan_return(&request, &facts).unwrap();
    assert_eq!(plan.group.disband, 0);
    assert_eq!(plan.group.form, 3);
    assert_eq!(plan.direct_rng_draws, 0);
    assert_eq!(
        plan.effects,
        vec![
            action_begin(),
            ReturnEffect::ScenarioKill {
                target_o: 8,
                target_who: 2,
                tail_0: 0,
                tail_1: 0,
            },
            ReturnEffect::ScenarioKill {
                target_o: 12,
                target_who: 2,
                tail_0: 0,
                tail_1: 0,
            },
        ]
    );
}

#[test]
fn contained_aircraft_resets_then_removes_second_inside_launching_entry() {
    let request = request(&[7]);
    let member = ordinary_member(
        7,
        ReturnOrdinaryAircraftFacts {
            first_inside: ReturnInsideLookup { o: 20, who: 2 },
            contained: Some(ReturnContainedFacts {
                second_inside: ReturnInsideLookup { o: 21, who: 3 },
                launching: ReturnLaunchingFacts::Present {
                    contains_actor_o: true,
                },
            }),
            airborne: None,
        },
    );
    let plan = plan_return(&request, &facts(&request, vec![member])).unwrap();
    let mut expected = vec![action_begin()];
    expected.extend(reset_effects(7));
    expected.push(ReturnEffect::RemoveLaunchingObject {
        host_o: 21,
        host_who: 3,
        actor_o: 7,
    });
    assert_eq!(plan.effects, expected);
}

#[test]
fn null_launching_pointer_still_keeps_the_contained_reset_sequence() {
    let request = request(&[7]);
    let member = ordinary_member(
        7,
        ReturnOrdinaryAircraftFacts {
            first_inside: ReturnInsideLookup { o: 20, who: 2 },
            contained: Some(ReturnContainedFacts {
                second_inside: ReturnInsideLookup { o: 20, who: 2 },
                launching: ReturnLaunchingFacts::Null,
            }),
            airborne: None,
        },
    );
    let plan = plan_return(&request, &facts(&request, vec![member])).unwrap();
    let mut expected = vec![action_begin()];
    expected.extend(reset_effects(7));
    assert_eq!(plan.effects, expected);
}

#[test]
fn airborne_aircraft_rebuilds_strafe_and_restores_altitude_and_turn() {
    let request = request(&[7]);
    let member = ordinary_member(
        7,
        ReturnOrdinaryAircraftFacts {
            first_inside: ReturnInsideLookup { o: -1, who: -1 },
            contained: None,
            airborne: Some(ReturnAirLookup::Found {
                resolution: ReturnAirResolution::AfterSpecialAnimation {
                    first_order_type: ORDER_SPECIAL_ANIM,
                },
                order: ReturnAirOrderFacts {
                    home_o: 40,
                    home_who: 4,
                    cruising_alt: 0x550,
                    sharp_turn: -3,
                },
            }),
        },
    );
    let plan = plan_return(&request, &facts(&request, vec![member])).unwrap();
    let mut expected = vec![action_begin()];
    expected.extend(reset_effects(7));
    expected.extend([
        ReturnEffect::AddStrafeOrder {
            who: 2,
            o: 7,
            x: -1,
            y: -1,
            home_o: 40,
            home_who: 4,
            arg5: 0,
            queue_pos: QUEUE_NEW,
            arg7: 0,
        },
        ReturnEffect::UpdateOrder { who: 2, o: 7 },
        ReturnEffect::RestoreNewAirCruisingAltitude {
            who: 2,
            o: 7,
            value: 0x550,
        },
        ReturnEffect::RestoreNewAirSharpTurn {
            who: 2,
            o: 7,
            value: -3,
        },
    ]);
    assert_eq!(plan.effects, expected);
}

#[test]
fn missing_update_order_is_a_measured_no_effect_arm() {
    let request = request(&[7]);
    let member = ordinary_member(
        7,
        ReturnOrdinaryAircraftFacts {
            first_inside: ReturnInsideLookup { o: -1, who: 6 },
            contained: None,
            airborne: Some(ReturnAirLookup::UpdateOrderMissing {
                resolution: update_resolution(),
            }),
        },
    );
    let plan = plan_return(&request, &facts(&request, vec![member])).unwrap();
    assert_eq!(plan.effects, vec![action_begin()]);
}

#[test]
fn special_animation_may_resolve_to_no_air_order_without_mutation() {
    let request = request(&[7]);
    let member = ordinary_member(
        7,
        ReturnOrdinaryAircraftFacts {
            first_inside: ReturnInsideLookup { o: -9, who: 2 },
            contained: None,
            airborne: Some(ReturnAirLookup::AirOrderMissing {
                resolution: ReturnAirResolution::AfterSpecialAnimation {
                    first_order_type: ORDER_SPECIAL_ANIM,
                },
            }),
        },
    );
    let plan = plan_return(&request, &facts(&request, vec![member])).unwrap();
    assert_eq!(plan.effects, vec![action_begin()]);
}

#[test]
fn helicopter_resets_before_on_map_gate_and_installs_targetless_strafe() {
    let request = request(&[7, 8]);
    let helicopter = |o, on_map| ReturnMemberFacts {
        o,
        valid_unit: true,
        plane: Some(ReturnPlanePredicate::Concrete {
            domain_218: 0,
            unit_flags_2b4: HELICOPTER_TYPE_FLAG,
            object_masks_1e4: None,
        }),
        route: Some(ReturnRouteFacts::Helicopter { on_map }),
    };
    let plan = plan_return(
        &request,
        &facts(&request, vec![helicopter(7, false), helicopter(8, true)]),
    )
    .unwrap();
    let mut expected = vec![action_begin()];
    expected.extend(reset_effects(7));
    expected.extend(reset_effects(8));
    expected.push(ReturnEffect::AddStrafeOrder {
        who: 2,
        o: 8,
        x: -1,
        y: -1,
        home_o: -1,
        home_who: -1,
        arg5: 1,
        queue_pos: QUEUE_NEW,
        arg7: 0,
    });
    assert_eq!(plan.effects, expected);
}

#[test]
fn concrete_missile_and_dynamic_virtual_cones_remain_distinct() {
    let request = request(&[7, 8]);
    let missile = ReturnMemberFacts {
        o: 7,
        valid_unit: true,
        plane: Some(ReturnPlanePredicate::Concrete {
            domain_218: AIR_DOMAIN,
            unit_flags_2b4: 0,
            object_masks_1e4: Some(MISSILE_OBJECT_MASK),
        }),
        route: None,
    };
    let dynamic = ReturnMemberFacts {
        o: 8,
        valid_unit: true,
        plane: Some(ReturnPlanePredicate::Dynamic {
            is_plane: true,
            unit_flags_2b4: None,
        }),
        route: Some(ReturnRouteFacts::OrdinaryAircraft(
            ReturnOrdinaryAircraftFacts {
                first_inside: ReturnInsideLookup { o: -1, who: -1 },
                contained: None,
                airborne: Some(ReturnAirLookup::AirOrderMissing {
                    resolution: update_resolution(),
                }),
            },
        )),
    };
    let plan = plan_return(&request, &facts(&request, vec![missile, dynamic])).unwrap();
    assert_eq!(plan.effects, vec![action_begin()]);
}

#[test]
fn branch_sensitive_facts_and_member_identity_fail_closed() {
    let request = request(&[7]);
    let mut member = ReturnMemberFacts {
        o: 7,
        valid_unit: false,
        plane: Some(ordinary_predicate()),
        route: None,
    };
    assert_eq!(
        plan_return(&request, &facts(&request, vec![member])),
        Err(ReturnPlanError::UnexpectedPlanePredicate { index: 0 })
    );

    member.plane = None;
    member.o = 9;
    assert_eq!(
        plan_return(&request, &facts(&request, vec![member])),
        Err(ReturnPlanError::MemberIdentity {
            index: 0,
            expected: 7,
            got: 9,
        })
    );
}

#[test]
fn receipt_recomputes_full_effect_order_and_unavailable_is_empty() {
    let request = request(&[7]);
    let member = ordinary_member(
        7,
        ReturnOrdinaryAircraftFacts {
            first_inside: ReturnInsideLookup { o: -1, who: -1 },
            contained: None,
            airborne: Some(ReturnAirLookup::Found {
                resolution: update_resolution(),
                order: ReturnAirOrderFacts {
                    home_o: 20,
                    home_who: 2,
                    cruising_alt: 4,
                    sharp_turn: 5,
                },
            }),
        },
    );
    let facts = facts(&request, vec![member]);
    let plan = plan_return(&request, &facts).unwrap();
    let mut receipt = ReturnReceipt {
        request: request.clone(),
        status: ReturnTransactionStatus::Applied,
        facts: Some(facts),
        plan: Some(plan),
    };
    assert!(receipt.validates(&request));
    receipt.plan.as_mut().unwrap().effects.swap(0, 1);
    assert!(!receipt.validates(&request));

    let unavailable = ReturnReceipt::unavailable(request.clone());
    assert!(unavailable.validates(&request));
}
