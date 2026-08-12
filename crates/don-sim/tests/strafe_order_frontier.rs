// SPDX-License-Identifier: GPL-3.0-or-later
mod systems {
    pub mod air {
        pub use don_sim::systems::air::AirOrderWalk;
    }
    pub mod patrol {
        pub use don_sim::systems::patrol::StrafeOrder;
    }
}

#[path = "../src/systems/strafe_order_frontier.rs"]
mod strafe;

use strafe::*;

fn id(o: i32, who: i32, uid: u16) -> ObjectIdentity {
    ObjectIdentity { o, who, uid }
}

fn base() -> StrafeFrameFacts {
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
        order: StrafeOrderState {
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
            ..StrafeOrderState::default()
        },
        front: FrontCone::Active {
            target: TargetSnapshot {
                identity: id(4, 2, 40),
                x: 500,
                y: 600,
                active: true,
            },
            repaired_from: None,
            aim: AimCone::Live { x: 500, y: 600 },
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

#[test]
fn pdb_layout_and_cfg_extent_are_frozen() {
    assert_eq!(STRAFE_ORDER_INDEX, 16);
    assert_eq!(UNIT_DO_STRAFE_VA, 0x005e_ab00);
    assert_eq!(UNIT_DO_STRAFE_BYTES, 3_676);
    assert_eq!(STRAFE_ORDER_SIZE, 84);
    assert_eq!(STRAFE_WALKED_BYTES, 57);
    assert_eq!(offsets::TARGET_O, 8);
    assert_eq!(offsets::AIR_RETURNING, 0x3c);
    assert_eq!(offsets::XX, 0x40);
    assert_eq!(offsets::FLAGS, 0x50);
    assert_eq!(
        STRAFE_REACHABLE_CFG.first().unwrap().start,
        UNIT_DO_STRAFE_VA
    );
    assert_eq!(STRAFE_REACHABLE_CFG.last().unwrap().start, 0x005e_b8c1);
}

#[test]
fn cadence_uses_the_two_distinct_retail_phase_formulas() {
    assert!(scan_due(9, 7));
    assert!(!scan_due(9, 8));
    assert!(reacquire_due(9, 14));
    assert!(!reacquire_due(9, 7));
}

#[test]
fn active_target_refreshes_position_before_physics() {
    let plan = plan_strafe_frame(&base()).unwrap();
    assert_eq!(
        plan.steps,
        vec![
            StrafeStep::ThinkBird { mode: 0 },
            StrafeStep::StoreTargetPosition { x: 500, y: 600 },
            StrafeStep::AirPhysics {
                x: 500,
                y: 600,
                digest: 0xabc,
            },
            StrafeStep::Hold,
        ]
    );
    assert!(plan.physics_draws.is_empty());
}

#[test]
fn mod16_hit_inserts_before_physics_and_returns() {
    let mut f = base();
    f.frame = 7;
    f.front = FrontCone::Active {
        target: TargetSnapshot {
            identity: id(f.order.target_o, f.order.target_who, f.order.target_uid),
            x: 500,
            y: 600,
            active: true,
        },
        repaired_from: None,
        aim: AimCone::Live { x: 500, y: 600 },
        search_kind: AirTargetSearchKind::AirFirst,
        scan: SearchObservation::Hit(TargetSnapshot {
            identity: id(12, 3, 120),
            x: 700,
            y: 800,
            active: true,
        }),
    };
    f.physics = None;
    let p = plan_strafe_frame(&f).unwrap();
    assert_eq!(
        p.steps[p.steps.len() - 2],
        StrafeStep::InsertStrafeFirst {
            target: id(12, 3, 120),
            target_x: 700,
            target_y: 800,
            home_o: 8,
            home_who: 1,
        }
    );
    assert_eq!(p.steps.last(), Some(&StrafeStep::UpdateWork));
}

#[test]
fn mod16_observation_cannot_be_spliced_onto_another_frame() {
    let mut f = base();
    f.frame = 8;
    if let FrontCone::Active { ref mut scan, .. } = f.front {
        *scan = SearchObservation::Miss;
    }
    assert_eq!(
        plan_strafe_frame(&f),
        Err(StrafePlanError::ScanCadenceMismatch)
    );
}

#[test]
fn captain_repair_rewrites_identity_before_position() {
    let mut f = base();
    f.order.target_o = 4;
    f.order.target_who = 2;
    f.order.target_uid = 40;
    f.front = FrontCone::Active {
        target: TargetSnapshot {
            identity: id(6, 2, 60),
            x: 510,
            y: 610,
            active: true,
        },
        repaired_from: Some(id(4, 2, 40)),
        aim: AimCone::Live { x: 510, y: 610 },
        search_kind: AirTargetSearchKind::AirFirst,
        scan: SearchObservation::NotDue,
    };
    let p = plan_strafe_frame(&f).unwrap();
    assert_eq!(p.steps[1], StrafeStep::StoreTargetPair { o: 6, who: 2 });
    assert_eq!(
        p.steps[2],
        StrafeStep::StoreTargetPosition { x: 510, y: 610 }
    );
}

#[test]
fn no_target_saved_position_kills_then_installs_patrol_then_works() {
    let mut f = base();
    f.front = FrontCone::Missing(MissingTargetCone::SavedPatrol { group_flag: 4 });
    f.physics = None;
    let p = plan_strafe_frame(&f).unwrap();
    assert_eq!(
        &p.steps[1..],
        &[
            StrafeStep::KillCurrent,
            StrafeStep::AddAirPatrol {
                x: 500,
                y: 600,
                home_o: 8,
                home_who: 1,
                group_flag: 4,
            },
            StrafeStep::UpdateWork,
        ]
    );
}

#[test]
fn no_saved_target_latches_returning_without_fabricating_a_uid_write() {
    let mut f = base();
    f.front = FrontCone::Missing(MissingTargetCone::LatchReturning);
    f.post = PostPhysicsCone::PhysicsStopped;
    f.physics.as_mut().unwrap().completed = false;
    let p = plan_strafe_frame(&f).unwrap();
    assert_eq!(
        &p.steps[1..4],
        &[
            StrafeStep::StoreReturning(1),
            StrafeStep::StoreTargetPair { o: -1, who: -1 },
            StrafeStep::AirPhysics {
                x: -1,
                y: -1,
                digest: 0xabc,
            },
        ]
    );
}

#[test]
fn animal_and_missile_invalid_target_tails_are_distinct() {
    let mut animal = base();
    animal.actor.animal = true;
    animal.front = FrontCone::Missing(MissingTargetCone::Animal {
        active_after_think: true,
    });
    animal.physics = None;
    assert_eq!(
        plan_strafe_frame(&animal).unwrap().steps,
        vec![
            StrafeStep::ThinkBird { mode: 0 },
            StrafeStep::ThinkBird { mode: 1 },
            StrafeStep::UpdateWork,
        ]
    );

    let mut missile = base();
    missile.actor.missile = true;
    missile.front = FrontCone::Missing(MissingTargetCone::MissileDie);
    missile.physics = None;
    assert_eq!(
        plan_strafe_frame(&missile).unwrap().steps,
        vec![StrafeStep::ThinkBird { mode: 0 }, StrafeStep::Die]
    );
}

#[test]
fn physics_rng_trace_is_exact_and_bounded() {
    let mut f = base();
    f.physics = Some(AirPhysicsReceipt {
        completed: false,
        mutation_digest: 99,
        rng_epoch_before: 50,
        rng_epoch_after: 52,
        draws: vec![
            RngDraw {
                lo: 0,
                hi_exclusive: 0xffff,
                value: 4,
            },
            RngDraw {
                lo: 0,
                hi_exclusive: 0xffff,
                value: 65_534,
            },
        ],
    });
    f.post = PostPhysicsCone::PhysicsStopped;
    assert_eq!(plan_strafe_frame(&f).unwrap().physics_draws.len(), 2);
    f.physics.as_mut().unwrap().rng_epoch_after = 51;
    assert_eq!(
        plan_strafe_frame(&f),
        Err(StrafePlanError::InvalidPhysicsRngReceipt)
    );
}

#[test]
fn ammo_or_animation_precedes_the_shared_attack_latch_tail() {
    let mut ammo = base();
    ammo.post = PostPhysicsCone::Combat {
        fire: FireCone::FireAmmo(AttackTail {
            attack_call_al: 7,
            bomber_spell_delta: None,
        }),
        reacquire: ReacquireCone::NotEligible,
    };
    let p = plan_strafe_frame(&ammo).unwrap();
    assert_eq!(
        &p.steps[p.steps.len() - 2..],
        &[
            StrafeStep::FireAmmo(id(
                ammo.order.target_o,
                ammo.order.target_who,
                ammo.order.target_uid,
            )),
            StrafeStep::StoreAttackLatch(8),
        ]
    );

    let mut bomber = base();
    bomber.actor.bomber = true;
    if let FrontCone::Active {
        ref mut search_kind,
        ..
    } = bomber.front
    {
        *search_kind = AirTargetSearchKind::BomberFirst;
    }
    bomber.post = PostPhysicsCone::Combat {
        fire: FireCone::AttackAnimation(AttackTail {
            attack_call_al: u8::MAX,
            bomber_spell_delta: Some(13),
        }),
        reacquire: ReacquireCone::NotEligible,
    };
    let p = plan_strafe_frame(&bomber).unwrap();
    assert_eq!(
        &p.steps[p.steps.len() - 3..],
        &[
            StrafeStep::SetAnimation {
                animation: ATTACK_ANIM,
            },
            StrafeStep::StoreAttackLatch(0),
            StrafeStep::AddSpellTime(13),
        ]
    );
}

#[test]
fn mod32_retarget_writes_identity_and_clears_returning_as_one_plan() {
    let mut f = base();
    f.frame = 14;
    if let FrontCone::Active { ref mut scan, .. } = f.front {
        *scan = SearchObservation::NotDue;
    }
    f.post = PostPhysicsCone::Combat {
        fire: FireCone::Hold,
        reacquire: ReacquireCone::Search {
            origin: QueuedSearchOrigin::AirPatrol { x: 900, y: 901 },
            kind: AirTargetSearchKind::AirFirst,
            result: SearchObservation::Hit(TargetSnapshot {
                identity: id(15, 4, 150),
                x: 910,
                y: 911,
                active: true,
            }),
        },
    };
    let p = plan_strafe_frame(&f).unwrap();
    assert_eq!(
        &p.steps[p.steps.len() - 2..],
        &[
            StrafeStep::StoreTargetIdentity(id(15, 4, 150)),
            StrafeStep::StoreReturning(0),
        ]
    );
}

#[test]
fn receipt_binds_every_mutation_sensitive_epoch_and_recomputes_plan() {
    let f = base();
    let snapshot = StrafeHostSnapshot {
        actor: f.actor.identity,
        actor_version: 1,
        order_version: 2,
        target_pool_epoch: 3,
        queue_digest: 4,
        path_digest: 5,
        external_effect_epoch: 6,
        rng_epoch: 20,
    };
    let receipt = StrafeExecutorReceipt::preflight(snapshot, f.clone()).unwrap();
    assert_eq!(receipt.validates(snapshot, &f), Ok(()));
    let mut stale = snapshot;
    stale.path_digest ^= 1;
    assert_eq!(
        receipt.validates(stale, &f),
        Err(StrafePlanError::ReceiptSnapshotMismatch)
    );
    let mut changed = f;
    changed.order.xx += 1;
    assert_eq!(
        receipt.validates(snapshot, &changed),
        Err(StrafePlanError::ReceiptFactsMismatch)
    );
}

#[test]
fn strict_frontier_stays_honestly_open() {
    assert_eq!(STRAFE_OPEN_TAILS.len(), 10);
    assert!(STRAFE_OPEN_TAILS.contains(&"AirPhysicsAtomicRngAdapter"));
    assert!(STRAFE_OPEN_TAILS.contains(&"LiveTickAtomicCommit"));
}
