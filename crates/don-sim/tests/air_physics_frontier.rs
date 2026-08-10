// SPDX-License-Identifier: GPL-3.0-or-later
mod systems {
    pub mod groups_guys {
        pub use don_sim::systems::groups_guys::{angle_diff, cosx, sinx, vector_dist};
    }
}

mod trig {
    pub use don_sim::trig::find_angle;
}

#[path = "../src/systems/air_physics_frontier.rs"]
mod frontier;

use frontier::*;

fn known<T>(value: T) -> HostFact<T> {
    HostFact::Known(value)
}

fn identity() -> ObjectIdentity {
    ObjectIdentity {
        who: 1,
        o: 7,
        uid: 70,
    }
}

fn mutation(transaction: u64, mutation_digest: u64) -> MutationReceipt {
    MutationReceipt {
        transaction,
        mutation_digest,
    }
}

fn actor() -> ActorState {
    ActorState {
        identity: identity(),
        x: 1_000,
        y: 2_000,
        angle: 0x1000_0000,
        recharging: 0,
        queue_len: 1,
        first_guy_z: 1_500,
    }
}

fn order() -> AirOrderState {
    AirOrderState {
        home_o: -1,
        home_who: -1,
        cruising_alt: 1_600,
        sharp_turn: 0,
        old: 0,
        returning: 0,
    }
}

fn base() -> AirPhysicsFacts {
    let transaction = 0xabc;
    AirPhysicsFacts {
        snapshot: AirPhysicsSnapshot {
            transaction,
            actor: identity(),
            actor_version: 10,
            order_version: 11,
            queue_digest: 12,
            path_digest: 13,
            object_pool_epoch: 14,
            terrain_epoch: 15,
            animation_epoch: 16,
            external_effect_epoch: 17,
            rng_epoch: 20,
        },
        frame: 2,
        map_width_tiles: 128,
        map_height_tiles: 128,
        ai_speed: 1,
        actor_type: 0x120,
        actor_is_bomber: known(false),
        actor_is_animal: known(false),
        actor_is_helicopter: known(false),
        actor_speed: known(96),
        actor_min_range: known(4),
        actor: actor(),
        order: order(),
        aim_x: 3_000,
        aim_y: 4_000,
        cruise_draw: None,
        fuel: Some(FuelReceipt::Continue {
            mutation: mutation(transaction, 0x01),
            actor: actor(),
            order: order(),
            aim_x: 3_000,
            aim_y: 4_000,
            altitude_goal: 1_600,
        }),
        terrain_altitude: HostFact::Missing("terrain altitude"),
        home_is_active: HostFact::Missing("home active"),
        current_air_target: known(CurrentAirTarget::Rejected),
        bank: known(BankReceipt {
            mutation: mutation(transaction, 0x02),
            actor_angle_after: 0x1800_0000,
            speed_after: 90,
        }),
        pitch: known(PitchReceipt {
            mutation: mutation(transaction, 0x03),
            speed_before: 90,
            speed_after: 80,
        }),
        guy_set_angle: known(mutation(transaction, 0x04)),
        collision: known(CollisionReceipt {
            invalid_location: false,
            restricted: None,
            turn_draw: None,
            set_new_location: mutation(transaction, 0x05),
            actor_x_after: 1_030,
            actor_y_after: 1_926,
        }),
        land_plane: HostFact::Missing("land plane"),
        kill_current_order: HostFact::Missing("kill current order"),
        set_idle_animation: known(mutation(transaction, 0x06)),
    }
}

#[test]
fn pdb_extent_and_semantic_cfg_are_frozen() {
    assert_eq!(UNIT_DO_AIR_PHYSICS_VA, 0x005e_86d0);
    assert_eq!(UNIT_DO_AIR_PHYSICS_BYTES, 1_794);
    assert_eq!(UNIT_DO_AIR_PHYSICS_END, 0x005e_8dd2);
    assert_eq!(BOMBER_TYPE, 0x130);
    assert_eq!(UNIT_FLAG_HELICOPTER, 0x20);
    assert_eq!(AIR_DOMAIN, 2);
    assert_eq!(
        AIR_PHYSICS_CFG.first().unwrap().start,
        UNIT_DO_AIR_PHYSICS_VA
    );
    assert_eq!(AIR_PHYSICS_CFG.last().unwrap().start, 0x005e_8da9);
    assert!(AIR_PHYSICS_OPEN_HOSTS.contains(&"CheckFuelAtomicMutation"));
    assert!(AIR_PHYSICS_OPEN_HOSTS.contains(&"AtomicMainRng"));
}

#[test]
fn ordinary_success_orders_the_complete_top_level_transaction() {
    let plan = plan_air_physics(&base()).unwrap();
    assert_eq!(plan.returned, AirPhysicsReturn::Continue);
    assert!(plan.rng_draws.is_empty());
    assert_eq!(plan.rng_epoch_after, 20);
    assert_eq!(plan.steps[0], AirPhysicsStep::ClearPath);
    assert_eq!(
        plan.steps[1],
        AirPhysicsStep::CheckFuel {
            mutation_digest: 0x01
        }
    );
    assert!(matches!(
        plan.steps.as_slice(),
        [
            ..,
            AirPhysicsStep::BankAircraft { .. },
            AirPhysicsStep::PitchAircraft { .. },
            AirPhysicsStep::GuySetAngle { .. },
            AirPhysicsStep::ProjectLocation { .. },
            AirPhysicsStep::TestInvalidLocation { invalid: false },
            AirPhysicsStep::StoreSharpTurn(0),
            AirPhysicsStep::SetNewLocation { .. },
            AirPhysicsStep::SetAnimation { .. },
            AirPhysicsStep::Return(AirPhysicsReturn::Continue),
        ]
    ));
}

#[test]
fn cruise_draw_happens_before_path_clear_and_even_before_fuel_stop() {
    let mut facts = base();
    facts.frame = 1; // actor.o 7 + frame 1 == 0 mod 8
    facts.cruise_draw = Some(RngDraw {
        lo: 0,
        hi_exclusive: 0xffff,
        value: 15,
    });
    let mut stopped_order = facts.order;
    stopped_order.cruising_alt = 1_400;
    facts.fuel = Some(FuelReceipt::Stopped {
        mutation: mutation(facts.snapshot.transaction, 0x77),
        order: stopped_order,
    });
    let plan = plan_air_physics(&facts).unwrap();
    assert_eq!(plan.returned, AirPhysicsReturn::Stop);
    assert_eq!(plan.final_order.cruising_alt, 1_400);
    assert_eq!(plan.rng_epoch_after, 21);
    assert_eq!(
        plan.steps,
        vec![
            AirPhysicsStep::DrawCruisingAltitude(facts.cruise_draw.unwrap()),
            AirPhysicsStep::StoreCruisingAltitude(1_400),
            AirPhysicsStep::ClearPath,
            AirPhysicsStep::CheckFuel {
                mutation_digest: 0x77,
            },
            AirPhysicsStep::Return(AirPhysicsReturn::Stop),
        ]
    );
}

#[test]
fn returning_arrival_lands_before_path_append() {
    let mut facts = base();
    facts.order.returning = 1;
    facts.aim_x = 1_030;
    facts.aim_y = 2_020;
    facts.fuel = Some(FuelReceipt::Continue {
        mutation: mutation(facts.snapshot.transaction, 0x01),
        actor: actor(),
        order: facts.order,
        aim_x: facts.aim_x,
        aim_y: facts.aim_y,
        altitude_goal: 1_600,
    });
    facts.land_plane = known(mutation(facts.snapshot.transaction, 0x88));
    let plan = plan_air_physics(&facts).unwrap();
    assert_eq!(plan.returned, AirPhysicsReturn::Stop);
    assert_eq!(
        plan.steps.last().unwrap(),
        &AirPhysicsStep::Return(AirPhysicsReturn::Stop)
    );
    assert!(plan.steps.contains(&AirPhysicsStep::LandPlane {
        mutation_digest: 0x88
    }));
    assert!(!plan
        .steps
        .iter()
        .any(|step| matches!(step, AirPhysicsStep::PushPath { .. })));
}

#[test]
fn invalid_collision_consumes_the_second_direct_rng_site_only_when_sharp_turn_is_zero() {
    let mut facts = base();
    let draw = RngDraw {
        lo: 0,
        hi_exclusive: 0xffff,
        value: 8,
    };
    facts.collision = known(CollisionReceipt {
        invalid_location: true,
        restricted: Some((1_050, 1_900)),
        turn_draw: Some(draw),
        set_new_location: mutation(facts.snapshot.transaction, 0x55),
        actor_x_after: 1_050,
        actor_y_after: 1_900,
    });
    let plan = plan_air_physics(&facts).unwrap();
    assert_eq!(plan.rng_draws, vec![draw]);
    assert_eq!(plan.final_order.sharp_turn, -1, "even draw selects -1");
    assert!(plan
        .steps
        .contains(&AirPhysicsStep::RestrictLocation { x: 1_050, y: 1_900 }));
    assert!(plan.steps.contains(&AirPhysicsStep::StoreSharpTurn(-1)));

    facts.order.sharp_turn = 1;
    if let FuelReceipt::Continue { order, .. } = facts.fuel.as_mut().unwrap() {
        order.sharp_turn = 1;
    }
    assert_eq!(
        plan_air_physics(&facts),
        Err(AirPhysicsPlanError::UnexpectedRngDraw(
            "collision-turn draw"
        ))
    );
}

#[test]
fn close_helicopter_with_a_following_order_kills_after_aircraft_helpers() {
    let mut facts = base();
    facts.actor_is_helicopter = known(true);
    facts.actor.queue_len = 2;
    facts.aim_x = 1_010;
    facts.aim_y = 2_010;
    facts.fuel = Some(FuelReceipt::Continue {
        mutation: mutation(facts.snapshot.transaction, 0x01),
        actor: facts.actor,
        order: facts.order,
        aim_x: facts.aim_x,
        aim_y: facts.aim_y,
        altitude_goal: 1_600,
    });
    facts.kill_current_order = known(mutation(facts.snapshot.transaction, 0x99));
    let plan = plan_air_physics(&facts).unwrap();
    assert_eq!(plan.returned, AirPhysicsReturn::Stop);
    let bank = plan
        .steps
        .iter()
        .position(|step| matches!(step, AirPhysicsStep::BankAircraft { .. }))
        .unwrap();
    let kill = plan
        .steps
        .iter()
        .position(|step| matches!(step, AirPhysicsStep::KillCurrentOrder { .. }))
        .unwrap();
    assert!(bank < kill);
    assert!(!plan
        .steps
        .iter()
        .any(|step| matches!(step, AirPhysicsStep::ProjectLocation { .. })));
}

#[test]
fn type_193_reads_terrain_only_for_a_single_order_and_resets_at_the_tail() {
    let mut facts = base();
    facts.actor_type = BIRD_TERRAIN_TYPE;
    facts.actor_is_animal = known(true);
    facts.actor.queue_len = 1;
    facts.fuel = None;
    facts.terrain_altitude = known(777);
    let plan = plan_air_physics(&facts).unwrap();
    assert!(plan.steps.contains(&AirPhysicsStep::StoreReturning(1)));
    assert!(plan
        .steps
        .contains(&AirPhysicsStep::ReadTerrainAltitude(777)));
    assert_eq!(plan.final_order.returning, 0);
    assert!(matches!(
        plan.steps.as_slice(),
        [
            ..,
            AirPhysicsStep::StoreReturning(0),
            AirPhysicsStep::StoreRecharge(1),
            AirPhysicsStep::Return(AirPhysicsReturn::Continue)
        ]
    ));
}

#[test]
fn reached_missing_fact_and_cross_transaction_mutation_fail_closed() {
    let mut facts = base();
    facts.bank = HostFact::Missing("bank aircraft");
    assert_eq!(
        plan_air_physics(&facts),
        Err(AirPhysicsPlanError::MissingHostFact("bank aircraft"))
    );

    facts.bank = known(BankReceipt {
        mutation: mutation(facts.snapshot.transaction + 1, 0x02),
        actor_angle_after: 0,
        speed_after: 1,
    });
    assert_eq!(
        plan_air_physics(&facts),
        Err(AirPhysicsPlanError::InvalidMutationTransaction)
    );
}

#[test]
fn atomic_commit_receipt_binds_the_entire_plan_and_rng_epoch() {
    let plan = plan_air_physics(&base()).unwrap();
    let mut receipt = AirPhysicsCommitReceipt::applied(&plan);
    assert!(receipt.validates(&plan));
    receipt.committed_steps -= 1;
    assert!(!receipt.validates(&plan));
    let mut receipt = AirPhysicsCommitReceipt::applied(&plan);
    receipt.snapshot.path_digest ^= 1;
    assert!(!receipt.validates(&plan));
}
