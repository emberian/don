#[path = "../src/systems/step8_eject_contents.rs"]
mod step8_eject_contents;

use step8_eject_contents::*;

fn key(who: i8, object: i16, uid: u16) -> ObjectKey {
    ObjectKey { who, object, uid }
}

fn stamp(n: u32) -> DeterminismStamp {
    stamp_with_rng(n, n)
}

fn stamp_with_rng(channel_step: u32, rng_step: u32) -> DeterminismStamp {
    let mut channels = [0; CHECK_CHANNELS];
    channels[1] = channel_step.wrapping_mul(11);
    channels[2] = channel_step.wrapping_mul(13);
    channels[6] = channel_step.wrapping_mul(17);
    channels[7] = channel_step.wrapping_mul(19);
    channels[12] = channel_step.wrapping_mul(23);
    DeterminismStamp {
        rng_seed: 0x1234_0000 | rng_step,
        rng_draws: rng_step as u64,
        channels,
    }
}

fn receipt(
    token: u64,
    carrier: ObjectKey,
    passenger: ObjectKey,
    effect: ReceiptEffect,
    before: DeterminismStamp,
    after: DeterminismStamp,
) -> EffectReceipt {
    EffectReceipt {
        token,
        carrier,
        passenger,
        effect,
        before,
        after,
    }
}

fn carrier(immediate: bool) -> WallCarrierSnapshot {
    WallCarrierSnapshot {
        key: key(1, 90, 900),
        on_map: true,
        is_build: true,
        can_carry_air: false,
        immediate_ejection: immediate,
        build_masks: 0x0123,
        inside_head: Some(key(1, 10, 100)),
        is_airbase: false,
    }
}

fn two_passenger_snapshot() -> WallEjectSnapshot {
    let carrier = carrier(true);
    let first = key(1, 10, 100);
    let second = key(1, 11, 101);
    let first_release = ReceiptEffect::ResetAndComeOut {
        result: ComeOutResult::Released,
        carrier_head_after: Some(second),
    };
    let first_tail = ReceiptEffect::ReleasedTail {
        route: SpecialistRoute::MoveToCity {
            city: key(1, 40, 400),
            x: 0x1800,
            y: 0x2400,
        },
        airbase_strafe: false,
    };
    let second_release = ReceiptEffect::ResetAndComeOut {
        result: ComeOutResult::Blocked,
        carrier_head_after: Some(second),
    };
    let second_die = ReceiptEffect::DieFailedRelease {
        carrier_head_after: None,
    };
    WallEjectSnapshot {
        revision: 7,
        initial_stamp: stamp(0),
        carrier,
        passengers: vec![
            PassengerSnapshot {
                key: first,
                type_index: 0x34,
                unit_masks: UNIT_RESET_MASK | 0x55,
                path_count: 9,
                result: ComeOutResult::Released,
                head_after_come_out: Some(second),
                come_out: receipt(1, carrier.key, first, first_release, stamp(0), stamp(1)),
                released_tail: Some(receipt(
                    2,
                    carrier.key,
                    first,
                    first_tail,
                    stamp(1),
                    stamp_with_rng(2, 1),
                )),
                failed_die: None,
            },
            PassengerSnapshot {
                key: second,
                type_index: 0x80,
                unit_masks: UNIT_RESET_MASK | 0xAA,
                path_count: 3,
                result: ComeOutResult::Blocked,
                head_after_come_out: Some(second),
                come_out: receipt(
                    3,
                    carrier.key,
                    second,
                    second_release,
                    stamp_with_rng(2, 1),
                    stamp_with_rng(3, 2),
                ),
                released_tail: None,
                failed_die: Some(receipt(
                    4,
                    carrier.key,
                    second,
                    second_die,
                    stamp_with_rng(3, 2),
                    stamp_with_rng(4, 3),
                )),
            },
        ],
    }
}

#[test]
fn off_map_is_a_true_no_op() {
    let mut snapshot = two_passenger_snapshot();
    snapshot.carrier.on_map = false;
    let plan = plan_step8_wall_eject(&snapshot).unwrap();
    assert_eq!(
        plan,
        WallEjectPlan::OffMapNoOp {
            revision: 7,
            carrier: snapshot.carrier.key,
        }
    );
}

#[test]
fn normal_mode_defers_with_the_exact_build_mask() {
    let mut snapshot = two_passenger_snapshot();
    snapshot.carrier.immediate_ejection = false;
    snapshot.carrier.build_masks = 0x8210;
    let plan = plan_step8_wall_eject(&snapshot).unwrap();
    assert_eq!(
        plan,
        WallEjectPlan::DeferredMask {
            revision: 7,
            carrier: snapshot.carrier.key,
            before: 0x8210,
            after: 0xC210,
        }
    );
}

#[test]
fn immediate_mode_preserves_cleanup_release_tail_and_death_order() {
    let snapshot = two_passenger_snapshot();
    let plan = plan_step8_wall_eject(&snapshot).unwrap();
    let WallEjectPlan::Drained {
        initial_stamp,
        final_stamp,
        ops,
        ..
    } = plan
    else {
        panic!("expected drained plan");
    };
    assert_eq!(initial_stamp, stamp(0));
    assert_eq!(final_stamp, stamp_with_rng(4, 3));
    assert_eq!(ops.len(), 4);

    let ImmediateOp::ResetAndComeOut(first) = ops[0] else {
        panic!("first operation must be cleanup plus come_out");
    };
    assert_eq!(first.passenger, key(1, 10, 100));
    assert_eq!(first.expected_unit_masks, UNIT_RESET_MASK | 0x55);
    assert_eq!(first.unit_masks_after, 0x55);
    assert_eq!(first.expected_path_count, 9);
    assert_eq!(first.path_count_after, 0);
    assert_eq!(first.close_orders_arg, 0);
    assert_eq!(first.come_out_arg, 0);

    assert!(matches!(
        ops[1],
        ImmediateOp::ReleasedTail {
            passenger,
            route: SpecialistRoute::MoveToCity { x: 0x1800, y: 0x2400, .. },
            airbase_strafe: false,
            ..
        } if passenger == key(1, 10, 100)
    ));
    assert!(matches!(
        ops[2],
        ImmediateOp::ResetAndComeOut(ResetAndComeOutOp {
            passenger,
            receipt: EffectReceipt {
                effect: ReceiptEffect::ResetAndComeOut {
                    result: ComeOutResult::Blocked,
                    ..
                },
                ..
            },
            ..
        }) if passenger == key(1, 11, 101)
    ));
    assert!(matches!(
        ops[3],
        ImmediateOp::KillFailedRelease {
            passenger,
            die_arg_1: 0,
            die_arg_2: -1,
            die_arg_3_bits: 0,
            ..
        } if passenger == key(1, 11, 101)
    ));
}

#[test]
fn discontinuous_nested_receipt_fails_before_a_plan_exists() {
    let mut snapshot = two_passenger_snapshot();
    snapshot.passengers[1].come_out.before = stamp(99);
    assert_eq!(
        plan_step8_wall_eject(&snapshot),
        Err(WallEjectPlanError::ReceiptContinuityMismatch(key(
            1, 11, 101
        )))
    );
}

#[test]
fn come_out_allows_at_most_one_draw_and_release_tail_allows_none() {
    let mut snapshot = two_passenger_snapshot();
    snapshot.passengers[0].come_out.after.rng_draws = 2;
    assert_eq!(
        plan_step8_wall_eject(&snapshot),
        Err(WallEjectPlanError::TooManyComeOutDraws {
            passenger: key(1, 10, 100),
            draws: 2,
        })
    );

    let mut snapshot = two_passenger_snapshot();
    let tail = snapshot.passengers[0].released_tail.as_mut().unwrap();
    tail.after.rng_draws = tail.before.rng_draws + 1;
    assert_eq!(
        plan_step8_wall_eject(&snapshot),
        Err(WallEjectPlanError::ReleasedTailConsumedRng {
            passenger: key(1, 10, 100),
            draws: 1,
        })
    );
}

#[test]
fn repeated_head_and_wrong_caller_guard_fail_closed() {
    let mut snapshot = two_passenger_snapshot();
    snapshot.passengers[0].head_after_come_out = Some(key(1, 10, 100));
    snapshot.passengers[0].come_out.effect = ReceiptEffect::ResetAndComeOut {
        result: ComeOutResult::Released,
        carrier_head_after: Some(key(1, 10, 100)),
    };
    assert_eq!(
        plan_step8_wall_eject(&snapshot),
        Err(WallEjectPlanError::NoProgress(key(1, 10, 100)))
    );

    let mut snapshot = two_passenger_snapshot();
    snapshot.carrier.can_carry_air = true;
    assert_eq!(
        plan_step8_wall_eject(&snapshot),
        Err(WallEjectPlanError::CallerCanCarryAir)
    );
}

#[test]
fn specialist_and_airbase_facts_are_not_approximated() {
    let mut snapshot = two_passenger_snapshot();
    let tail = snapshot.passengers[0].released_tail.as_mut().unwrap();
    let route = SpecialistRoute::MilitiaUpgrade { type_index: 0x42 };
    tail.effect = ReceiptEffect::ReleasedTail {
        route,
        airbase_strafe: false,
    };
    assert_eq!(
        plan_step8_wall_eject(&snapshot),
        Err(WallEjectPlanError::InvalidSpecialistRoute {
            passenger_type: 0x34,
            route,
        })
    );

    let mut snapshot = two_passenger_snapshot();
    snapshot.carrier.is_airbase = true;
    assert_eq!(
        plan_step8_wall_eject(&snapshot),
        Err(WallEjectPlanError::AirbaseMismatch {
            expected: true,
            observed: false,
        })
    );
}

#[derive(Clone)]
struct FakeHost {
    snapshot: WallEjectSnapshot,
    commits: usize,
    reject_commit: bool,
}

impl Step8EjectHost for FakeHost {
    type Error = &'static str;

    fn snapshot(&mut self, carrier: ObjectKey) -> Result<WallEjectSnapshot, Self::Error> {
        if carrier != self.snapshot.carrier.key {
            return Err("wrong carrier");
        }
        Ok(self.snapshot.clone())
    }

    fn commit(&mut self, _plan: &WallEjectPlan) -> Result<(), Self::Error> {
        if self.reject_commit {
            return Err("stale revision");
        }
        self.commits += 1;
        Ok(())
    }
}

#[test]
fn host_commit_is_single_and_stale_revision_can_abort_it() {
    let snapshot = two_passenger_snapshot();
    let mut host = FakeHost {
        snapshot: snapshot.clone(),
        commits: 0,
        reject_commit: false,
    };
    let plan = execute_step8_wall_eject(&mut host, snapshot.carrier.key).unwrap();
    assert!(matches!(plan, WallEjectPlan::Drained { .. }));
    assert_eq!(host.commits, 1);

    let mut host = FakeHost {
        snapshot,
        commits: 0,
        reject_commit: true,
    };
    let carrier_key = host.snapshot.carrier.key;
    assert_eq!(
        execute_step8_wall_eject(&mut host, carrier_key),
        Err(WallEjectExecuteError::Host("stale revision"))
    );
    assert_eq!(host.commits, 0);
}
