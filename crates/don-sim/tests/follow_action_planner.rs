// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation and receipt pins for the standalone `Group::action_follow` transaction.

mod systems {
    pub mod groups_guys {
        pub use don_sim::systems::groups_guys::*;
    }
}

#[path = "../src/systems/follow_action.rs"]
mod subject;

use don_sim::systems::groups_guys::GroupData;
use subject::*;

fn group(members: &[i16]) -> GroupData {
    let mut group = GroupData {
        id: 17,
        num: members.len() as i32,
        form: 6,
        disband: 91,
        who: 2,
        ..GroupData::default()
    };
    group.list[..members.len()].copy_from_slice(members);
    group
}

fn request(members: &[i16], target_o: i32, target_who: i32, queued: i32) -> FollowRequest {
    FollowRequest {
        group: group(members),
        command: FollowCommand {
            target_o,
            target_who,
            queued,
        },
    }
}

fn target(raw_o: i32, raw_who: i32, canonical_o: i32) -> FollowTargetFacts {
    FollowTargetFacts {
        queried_o: raw_o,
        queried_who: raw_who,
        valid_unit: true,
        on_map: true,
        is_plane: false,
        canonical_o,
    }
}

fn member(o: i16) -> FollowMemberFacts {
    FollowMemberFacts {
        o,
        valid_unit: true,
        on_map: true,
        is_plane: false,
    }
}

#[test]
fn decoder_preserves_the_raw_queue_dword() {
    let mut wire = vec![FOLLOW_OPCODE];
    wire.extend_from_slice(&19_i32.to_le_bytes());
    wire.extend_from_slice(&3_i32.to_le_bytes());
    wire.extend_from_slice(&99_i32.to_le_bytes());
    assert_eq!(
        decode_follow(&wire),
        Some(FollowCommand {
            target_o: 19,
            target_who: 3,
            queued: 99,
        })
    );
    assert!(decode_follow(&wire[..12]).is_none());
}

#[test]
fn building_and_negative_target_returns_need_no_world_facts() {
    let mut building = request(&[1], 7, 2, 1);
    building.group.buildings = 1;
    let plan = plan_follow(&building, &building.group, None, &[]).unwrap();
    assert_eq!(plan.group.disband, 0);
    assert_eq!(plan.group.form, 6);
    assert!(plan.effects.is_empty());

    let negative = request(&[1], -1, 2, 1);
    let plan = plan_follow(&negative, &negative.group, None, &[]).unwrap();
    assert_eq!(plan.group.disband, 0);
    assert_eq!(plan.group.form, 6);
    assert!(plan.effects.is_empty());
}

#[test]
fn rejected_target_still_resets_form_and_closes_the_insert_dance() {
    let request = request(&[1], 7, 2, 0);
    let mut rejected = target(7, 2, 7);
    rejected.on_map = false;
    let plan = plan_follow(&request, &request.group, Some(rejected), &[]).unwrap();
    assert_eq!(plan.group.form, -1);
    assert_eq!(plan.group.disband, 0);
    assert_eq!(
        plan.effects,
        vec![
            FollowEffect::SetUpInsert,
            FollowEffect::ActionHalt { flags: 0 },
            FollowEffect::FinishInsert,
        ]
    );
}

#[test]
fn canonical_self_target_is_not_confused_with_the_raw_lookup_identity() {
    let request = request(&[7, 8, 9, 10], 20, 2, 2);
    let mut members = [member(7), member(8), member(9), member(10)];
    members[2].is_plane = true;
    members[3].on_map = false;
    let plan = plan_follow(&request, &request.group, Some(target(20, 2, 8)), &members).unwrap();
    assert_eq!(plan.group.form, -1);
    assert_eq!(
        plan.effects,
        vec![FollowEffect::AddFollowOrder {
            actor_who: 2,
            actor_o: 7,
            target_o: 20,
            target_who: 2,
            queued: 2,
        }]
    );
}

#[test]
fn queue_first_wraps_new_follow_installs_in_retail_order() {
    let request = request(&[4, 5], 12, 3, 0);
    let plan = plan_follow(
        &request,
        &request.group,
        Some(target(12, 3, 12)),
        &[member(4), member(5)],
    )
    .unwrap();
    assert_eq!(
        plan.effects,
        vec![
            FollowEffect::SetUpInsert,
            FollowEffect::ActionHalt { flags: 0 },
            FollowEffect::AddFollowOrder {
                actor_who: 2,
                actor_o: 4,
                target_o: 12,
                target_who: 3,
                queued: 2,
            },
            FollowEffect::AddFollowOrder {
                actor_who: 2,
                actor_o: 5,
                target_o: 12,
                target_who: 3,
                queued: 2,
            },
            FollowEffect::FinishInsert,
        ]
    );
}

#[test]
fn nonstandard_nonzero_queue_value_is_passed_through() {
    let request = request(&[4], 12, 3, 77);
    let plan = plan_follow(
        &request,
        &request.group,
        Some(target(12, 3, 12)),
        &[member(4)],
    )
    .unwrap();
    assert_eq!(
        plan.effects,
        vec![FollowEffect::AddFollowOrder {
            actor_who: 2,
            actor_o: 4,
            target_o: 12,
            target_who: 3,
            queued: 77,
        }]
    );
}

#[test]
fn identity_failures_do_not_mutate_the_supplied_group() {
    let request = request(&[4], 12, 3, 2);
    let before = request.group.clone();
    assert_eq!(
        plan_follow(
            &request,
            &request.group,
            Some(target(99, 3, 12)),
            &[member(4)],
        ),
        Err(FollowPlanError::TargetIdentity)
    );
    assert_eq!(request.group, before);

    assert_eq!(
        plan_follow(
            &request,
            &request.group,
            Some(target(12, 3, 12)),
            &[member(5)],
        ),
        Err(FollowPlanError::MemberIdentity {
            index: 0,
            expected: 4,
            got: 5,
        })
    );
    assert_eq!(request.group, before);
}

#[test]
fn receipt_recomputes_the_whole_plan_and_rejects_spliced_effects() {
    let request = request(&[4], 12, 3, 2);
    let target = target(12, 3, 12);
    let members = vec![member(4)];
    let plan = plan_follow(&request, &request.group, Some(target), &members).unwrap();
    let mut receipt = FollowReceipt {
        request: request.clone(),
        status: FollowTransactionStatus::Applied,
        group_after_ignore_orders: Some(request.group.clone()),
        target: Some(target),
        members,
        plan: Some(plan),
    };
    assert!(receipt.validates(&request));
    receipt.plan.as_mut().unwrap().effects.clear();
    assert!(!receipt.validates(&request));

    let unavailable = FollowReceipt::unavailable(request.clone());
    assert!(unavailable.validates(&request));
}
