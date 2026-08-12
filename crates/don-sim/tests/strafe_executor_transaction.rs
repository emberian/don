// SPDX-License-Identifier: GPL-3.0-or-later
#![allow(dead_code)]

mod systems {
    pub mod air {
        pub use don_sim::systems::air::AirOrderWalk;
    }
    pub mod air_physics_frontier {
        pub use don_sim::systems::air_physics_frontier::*;
    }
    pub mod movement {
        pub use don_sim::systems::movement::*;
    }
    pub mod order_dispatch {
        pub use don_sim::systems::order_dispatch::*;
    }
    pub mod patrol {
        pub use don_sim::systems::patrol::*;
    }
    pub mod strafe_order_frontier {
        pub use crate::frontier::*;
    }
    pub mod strafe_runtime_authority {
        pub use crate::authority::*;
    }
}

#[path = "../src/systems/strafe_runtime_authority.rs"]
mod authority;
#[path = "../src/systems/strafe_order_frontier.rs"]
mod frontier;
#[path = "../src/systems/strafe_executor_transaction.rs"]
mod transaction;

use don_sim::order::OrderIndex;
use frontier::*;
use systems::air_physics_frontier::{
    AirOrderState as PhysicsAirOrderState, AirPhysicsCommitReceipt, AirPhysicsPlan,
    AirPhysicsReturn, AirPhysicsSnapshot, AirPhysicsStep, ObjectIdentity as PhysicsIdentity,
};
use systems::order_dispatch::{OrderQueue, OrderRec, PatrolPayload};
use systems::patrol::StrafeOrder;
use transaction::*;

fn id(o: i32, who: i32, uid: u16) -> ObjectIdentity {
    ObjectIdentity { o, who, uid }
}

fn strafe() -> StrafeOrder {
    StrafeOrder {
        target_o: 4,
        target_who: 2,
        target_uid: 40,
        air: AirOrderState {
            oxx: 8,
            whose: 1,
            cruising_alt: AIR_CRUISING_ALTITUDE,
            ..AirOrderState::default()
        },
        xx: 500,
        yy: 600,
        ..StrafeOrder::default()
    }
}

fn facts() -> StrafeFrameFacts {
    StrafeFrameFacts {
        frame: 8,
        actor: ActorSnapshot {
            identity: id(9, 1, 90),
            x: 100,
            y: 200,
            angle: 0,
            active: true,
            animal: false,
            missile: false,
            helicopter: false,
            bomber: false,
            strafes: false,
            queue_len: 1,
            attack_latch: 0,
            spell_time: 0,
        },
        order: strafe(),
        front: FrontCone::Active {
            target: TargetSnapshot {
                identity: id(4, 2, 40),
                x: 510,
                y: 610,
                active: true,
            },
            repaired_from: None,
            aim: AimCone::Live { x: 510, y: 610 },
            search_kind: AirTargetSearchKind::AirFirst,
            scan: SearchObservation::NotDue,
        },
        physics: Some(AirPhysicsReceipt {
            completed: true,
            mutation_digest: 0xabc,
            rng_epoch_before: 20,
            rng_epoch_after: 20,
            draws: vec![],
        }),
        post: PostPhysicsCone::Combat {
            fire: FireCone::Hold,
            reacquire: ReacquireCone::NotEligible,
        },
    }
}

fn image() -> CanonicalStrafeImage {
    let mut orders = OrderQueue::new();
    orders.push_back(OrderRec::strafe(strafe()));
    CanonicalStrafeImage {
        actor: CanonicalStrafeActor {
            identity: id(9, 1, 90),
            x: 100,
            y: 200,
            angle: 0,
            active: true,
            attack_latch: 0,
            spell_time: 0,
        },
        orders,
        path: Default::default(),
        rng_epoch: 20,
        external_effect_epoch: 6,
    }
}

fn snapshot() -> StrafeHostSnapshot {
    StrafeHostSnapshot {
        actor: id(9, 1, 90),
        actor_version: 1,
        order_version: 2,
        target_pool_epoch: 3,
        queue_digest: 4,
        path_digest: 5,
        external_effect_epoch: 6,
        rng_epoch: 20,
    }
}

fn nested_physics(transaction: u64, order: &StrafeOrder) -> AirPhysicsCommitReceipt {
    let snapshot = AirPhysicsSnapshot {
        transaction,
        actor: PhysicsIdentity {
            who: 1,
            o: 9,
            uid: 90,
        },
        actor_version: 1,
        order_version: 2,
        queue_digest: 4,
        path_digest: 5,
        object_pool_epoch: 3,
        terrain_epoch: 7,
        animation_epoch: 8,
        external_effect_epoch: 7,
        rng_epoch: 20,
    };
    let plan = AirPhysicsPlan {
        snapshot,
        steps: vec![AirPhysicsStep::Return(AirPhysicsReturn::Continue)],
        final_order: PhysicsAirOrderState {
            home_o: order.air.oxx,
            home_who: order.air.whose,
            cruising_alt: order.air.cruising_alt,
            sharp_turn: order.air.sharp_turn,
            old: order.air.old,
            returning: order.air.returning,
        },
        rng_draws: vec![],
        rng_epoch_after: 20,
        returned: AirPhysicsReturn::Continue,
    };
    AirPhysicsCommitReceipt::applied(&plan)
}

fn receipts(transaction: u64) -> Vec<StrafeExternalReceipt> {
    let before = image();
    let mut after_think = before.clone();
    after_think.external_effect_epoch = 7;

    let mut before_physics = after_think.clone();
    let PatrolPayload::Strafe(order) =
        &mut before_physics.orders.front_mut().unwrap().patrol_payload
    else {
        unreachable!()
    };
    order.xx = 510;
    order.yy = 610;
    let mut after_physics = before_physics.clone();
    after_physics.external_effect_epoch = 8;
    let PatrolPayload::Strafe(order) = &after_physics.orders.front().unwrap().patrol_payload else {
        unreachable!()
    };
    let order = order.clone();

    vec![
        StrafeExternalReceipt {
            transaction,
            step: StrafeStep::ThinkBird { mode: 0 },
            before,
            after: after_think,
            air_physics: None,
        },
        StrafeExternalReceipt {
            transaction,
            step: StrafeStep::AirPhysics {
                x: 510,
                y: 610,
                digest: 0xabc,
            },
            before: before_physics,
            after: after_physics,
            air_physics: Some(nested_physics(transaction, &order)),
        },
    ]
}

#[test]
fn complete_frame_prepares_all_local_and_nested_mutations_then_commits_once() {
    let transaction = 77;
    let before = image();
    let prepared = prepare_strafe_frame(
        transaction,
        snapshot(),
        facts(),
        before.clone(),
        receipts(transaction),
    )
    .unwrap();
    let PatrolPayload::Strafe(after_order) = &prepared.after.orders.front().unwrap().patrol_payload
    else {
        panic!("STRAFE disappeared")
    };
    assert_eq!((after_order.xx, after_order.yy), (510, 610));
    assert_eq!(prepared.after.external_effect_epoch, 8);

    let mut canonical = before;
    let receipt = commit_strafe_frame(snapshot(), &mut canonical, prepared).unwrap();
    assert_eq!(receipt.transaction, transaction);
    assert_eq!(receipt.external_steps, 2);
    assert_eq!(canonical.external_effect_epoch, 8);
}

#[test]
fn missing_nested_effect_fails_before_canonical_state_is_borrowed_mutably() {
    let canonical = image();
    let before = canonical.clone();
    let error = prepare_strafe_frame(77, snapshot(), facts(), canonical, vec![]).unwrap_err();
    assert_eq!(
        error,
        StrafeTransactionError::MissingExternalReceipt {
            step: StrafeStep::ThinkBird { mode: 0 }
        }
    );
    assert_eq!(before, image());
}

#[test]
fn forged_air_physics_transaction_or_return_is_rejected() {
    let mut wrong_transaction = receipts(77);
    let nested = wrong_transaction[1].air_physics.as_mut().unwrap();
    nested.snapshot.transaction = 78;
    nested.plan.snapshot.transaction = 78;
    assert_eq!(
        prepare_strafe_frame(77, snapshot(), facts(), image(), wrong_transaction).unwrap_err(),
        StrafeTransactionError::AirPhysicsTransactionMismatch
    );

    let mut wrong_return = receipts(77);
    let nested = wrong_return[1].air_physics.as_mut().unwrap();
    nested.plan.returned = AirPhysicsReturn::Stop;
    nested.plan.steps = vec![AirPhysicsStep::Return(AirPhysicsReturn::Stop)];
    nested.committed_steps = 1;
    assert_eq!(
        prepare_strafe_frame(77, snapshot(), facts(), image(), wrong_return).unwrap_err(),
        StrafeTransactionError::AirPhysicsReturnMismatch
    );
}

#[test]
fn malformed_duplicate_header_and_stale_commit_are_fail_closed() {
    let mut malformed = image();
    malformed.orders.front_mut().unwrap().target_uid ^= 1;
    assert_eq!(
        prepare_strafe_frame(77, snapshot(), facts(), malformed, receipts(77)).unwrap_err(),
        StrafeTransactionError::StrafeTargetHeaderMismatch
    );

    let prepared = prepare_strafe_frame(77, snapshot(), facts(), image(), receipts(77)).unwrap();
    let mut current = image();
    current.actor.spell_time = 1;
    let unchanged = current.clone();
    assert_eq!(
        commit_strafe_frame(snapshot(), &mut current, prepared).unwrap_err(),
        StrafeTransactionError::StaleImage
    );
    assert_eq!(current, unchanged);
}

#[test]
fn frontier_and_payload_authority_are_registered_for_the_canonical_host() {
    assert_eq!(OrderIndex::Strafe as i32, STRAFE_ORDER_INDEX);
    authority::validate_strafe_order(&strafe()).unwrap();
    assert!(std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/systems/mod.rs")
    )
    .unwrap()
    .contains("pub mod strafe_executor_transaction"));
}
