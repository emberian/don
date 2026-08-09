// SPDX-License-Identifier: GPL-3.0-or-later
//! Path-import mutation pins for `Unit::do_follow` without changing the shared dispatcher.

mod systems {
    pub mod groups_guys {
        pub use don_sim::systems::groups_guys::*;
    }
    pub mod movement {
        pub use don_sim::systems::movement::*;
    }
}

#[path = "../src/systems/follow_executor.rs"]
mod subject;

use subject::*;

fn identity(o: i32, who: i32, uid: u16) -> FollowIdentity {
    FollowIdentity { o, who, uid }
}

fn actor() -> FollowActorFacts {
    FollowActorFacts {
        o: 9,
        who: 2,
        x: 0,
        y: 0,
        speed: 8,
        los: 4,
    }
}

fn object(id: FollowIdentity, x: i32, y: i32) -> FollowObjectFacts {
    FollowObjectFacts {
        identity: id,
        valid_unit: true,
        on_map: true,
        active: true,
        inside_up: -1,
        seen_by_actor: true,
        x,
        y,
        angle: 0x2000_0000,
        speed: 4,
        is_moving: false,
        captain_o: id.o,
    }
}

fn request(primary: FollowIdentity, fallback: FollowIdentity) -> FollowExecutorRequest {
    FollowExecutorRequest {
        actor: actor(),
        order: FollowOrderState { primary, fallback },
    }
}

#[test]
fn invalid_primary_promotes_fallback_then_reenters_full_work() {
    let primary = identity(7, 3, 70);
    let fallback = identity(8, 3, 80);
    let request = request(primary, fallback);
    let mut bad = object(primary, 100, 100);
    bad.valid_unit = false;
    bad.active = false;
    let facts = FollowExecutorFacts {
        primary: Some(bad),
        ..FollowExecutorFacts::default()
    };
    let plan = plan_follow_executor(&request, &facts).unwrap();
    assert_eq!(plan.order.primary, fallback);
    assert_eq!(
        plan.branch,
        FollowExecutorBranch::PromoteFallbackAndReenterWork
    );
    assert_eq!(plan.effects, vec![FollowExecutorEffect::ReenterWork]);
}

#[test]
fn invalid_primary_equal_to_fallback_is_bare_kill() {
    let primary = identity(-1, 3, 70);
    let request = request(primary, primary);
    let plan = plan_follow_executor(&request, &FollowExecutorFacts::default()).unwrap();
    assert_eq!(plan.branch, FollowExecutorBranch::KillInvalidTarget);
    assert_eq!(
        plan.effects,
        vec![FollowExecutorEffect::KillCurrentOrder { flags: 0 }]
    );
}

#[test]
fn contained_primary_promotes_outer_root_and_saves_old_primary_as_fallback() {
    let primary = identity(7, 3, 70);
    let fallback = primary;
    let request = request(primary, fallback);
    let mut contained = object(primary, 100, 100);
    contained.on_map = false;
    contained.inside_up = 4;
    let root_id = identity(11, 4, 111);
    let root = object(root_id, 100, 100);
    let facts = FollowExecutorFacts {
        primary: Some(contained),
        containment_root: Some(root),
        fallback: Some(contained),
        fallback_captain: Some(contained),
        ..FollowExecutorFacts::default()
    };
    let plan = plan_follow_executor(&request, &facts).unwrap();
    assert_eq!(plan.order.primary, root_id);
    assert_eq!(plan.order.fallback, primary);
    assert_eq!(plan.branch, FollowExecutorBranch::HoldNearTarget);
}

#[test]
fn fallback_uid_match_tracks_its_captain_but_mismatch_collapses_to_primary() {
    let primary = identity(7, 3, 70);
    let fallback = identity(8, 4, 80);
    let request = request(primary, fallback);
    let primary_facts = object(primary, 100, 100);
    let mut fallback_facts = object(fallback, 120, 100);
    fallback_facts.captain_o = 9;
    let captain = object(identity(9, 4, 90), 120, 100);
    let matching = FollowExecutorFacts {
        primary: Some(primary_facts),
        fallback: Some(fallback_facts),
        fallback_captain: Some(captain),
        ..FollowExecutorFacts::default()
    };
    let plan = plan_follow_executor(&request, &matching).unwrap();
    assert_eq!(plan.order.fallback, identity(9, 4, 90));

    fallback_facts.identity.uid = 81;
    let mismatching = FollowExecutorFacts {
        primary: Some(primary_facts),
        fallback: Some(fallback_facts),
        ..FollowExecutorFacts::default()
    };
    let plan = plan_follow_executor(&request, &mismatching).unwrap();
    assert_eq!(plan.order.fallback, primary);
}

#[test]
fn unseen_target_kills_after_fallback_refresh_without_searches() {
    let primary = identity(7, 3, 70);
    let fallback = identity(8, 4, 80);
    let request = request(primary, fallback);
    let mut primary_facts = object(primary, 10_000, 0);
    primary_facts.seen_by_actor = false;
    let mut fallback_facts = object(fallback, 20, 20);
    fallback_facts.identity.uid = 81;
    let facts = FollowExecutorFacts {
        primary: Some(primary_facts),
        fallback: Some(fallback_facts),
        ..FollowExecutorFacts::default()
    };
    let plan = plan_follow_executor(&request, &facts).unwrap();
    assert_eq!(plan.order.fallback, primary);
    assert_eq!(plan.branch, FollowExecutorBranch::KillUnseenTarget);
    assert!(plan.searches.is_empty());
}

#[test]
fn near_target_uses_exact_radius_arithmetic_and_idles_in_place() {
    let primary = identity(7, 3, 70);
    let request = request(primary, primary);
    let facts = FollowExecutorFacts {
        primary: Some(object(primary, 1_000, 0)),
        ..FollowExecutorFacts::default()
    };
    let plan = plan_follow_executor(&request, &facts).unwrap();
    // Faster actor: margin=4*0x60, raw radius=4*0x180-margin=0x480, cutoff=0x540.
    assert_eq!(plan.radius, Some(0x480));
    assert_eq!(plan.cutoff, Some(0x540));
    assert_eq!(plan.branch, FollowExecutorBranch::HoldNearTarget);
    assert_eq!(
        plan.effects,
        vec![FollowExecutorEffect::SetAnim {
            anim: 0,
            mode: 0,
            choose: 1,
        }]
    );
}

#[test]
fn far_target_uses_ordered_searches_and_same_tick_move() {
    let primary = identity(7, 3, 70);
    let request = request(primary, primary);
    let mut target = object(primary, 10_000, 8_000);
    target.angle = 0x3000_0000;
    let facts = FollowExecutorFacts {
        primary: Some(target),
        nearby_results: vec![
            NearbySpotResult::Failed,
            NearbySpotResult::Found { x: -49, y: 97 },
        ],
        ..FollowExecutorFacts::default()
    };
    let plan = plan_follow_executor(&request, &facts).unwrap();
    assert_eq!(plan.searches.len(), 2);
    assert_eq!(plan.searches[0].request.angle, 0x5555_5555);
    assert_eq!(plan.searches[1].request.angle, 0x5555_5555);
    assert_eq!(plan.searches[0].request.filter, 3);
    assert_eq!(plan.searches[0].request.actor_o, 9);
    assert_eq!(plan.searches[0].request.actor_who, 2);
    assert_eq!(
        plan.effects,
        vec![
            FollowExecutorEffect::AddMoveFacingOrder {
                // floor((-49 >> 4) / 3) = floor(-4 / 3) = -2; 97 -> 6 -> 2.
                x: -2,
                y: 2,
                angle: 0x3000_0000,
                tail: FOLLOW_MOVE_FACING_TAIL,
            },
            FollowExecutorEffect::UpdateOrderThenDoMove,
        ]
    );
}

#[test]
fn third_search_has_radial_tuple_and_all_failures_fall_back_to_target() {
    let primary = identity(7, 3, 70);
    let request = request(primary, primary);
    let mut target = object(primary, 10_000, 8_000);
    target.angle = 0x3000_0000;
    let facts = FollowExecutorFacts {
        primary: Some(target),
        nearby_results: vec![
            NearbySpotResult::Failed,
            NearbySpotResult::Failed,
            NearbySpotResult::Failed,
        ],
        ..FollowExecutorFacts::default()
    };
    let plan = plan_follow_executor(&request, &facts).unwrap();
    let third = plan.searches[2].request;
    assert_eq!((third.centre_x, third.centre_y), (10_000, 8_000));
    assert_eq!(third.min_radius, plan.radius.unwrap());
    assert_eq!(third.max_radius, plan.cutoff.unwrap());
    assert_eq!(third.step, 0x30);
    assert_eq!(third.angle, target.angle);
    assert_eq!(
        plan.effects[0],
        FollowExecutorEffect::AddMoveFacingOrder {
            x: 208,
            y: 166,
            angle: target.angle,
            tail: FOLLOW_MOVE_FACING_TAIL,
        }
    );
}

#[test]
fn search_cardinality_and_receipt_recomputation_reject_splicing() {
    let primary = identity(7, 3, 70);
    let request = request(primary, primary);
    let facts = FollowExecutorFacts {
        primary: Some(object(primary, 10_000, 8_000)),
        nearby_results: vec![NearbySpotResult::Found { x: 500, y: 600 }],
        ..FollowExecutorFacts::default()
    };
    let plan = plan_follow_executor(&request, &facts).unwrap();
    let mut receipt = FollowExecutorReceipt {
        request,
        status: FollowExecutorTransactionStatus::Applied,
        facts: Some(facts),
        plan: Some(plan),
    };
    assert!(receipt.validates(&request));
    receipt.plan.as_mut().unwrap().effects.pop();
    assert!(!receipt.validates(&request));

    let mut extra = receipt.facts.unwrap();
    extra.nearby_results.push(NearbySpotResult::Failed);
    assert_eq!(
        plan_follow_executor(&request, &extra),
        Err(FollowExecutorPlanError::UnexpectedNearbyResults {
            expected: 1,
            got: 2,
        })
    );
}
