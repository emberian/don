//! Path-import mutation pins for the late fixed-size command plans.
//!
//! The source module is intentionally not exported or wired yet: root convergence can land
//! and validate it without touching the shared command dispatcher during this swarm wave.

#[path = "../src/systems/late_command_plans.rs"]
mod subject;

use subject::*;

fn cannon_facts() -> CannonTimeFacts {
    CannonTimeFacts {
        package_player_who: Some(6),
        display_who: 2,
        frame: i32::MIN,
        game_cannon_time_start: i32::MAX,
        remaining_uses: Some(4),
        state: CannonTimeState {
            active_player: -1,
            start_frame: 123,
            saved_speed: -5,
            current_speed: 3,
            cannon_time_start: -7,
        },
    }
}

#[test]
fn mutation_cannon_wire_state_is_diagnostic_only_and_player_who_is_indirected() {
    let a = decode_cannon_time(-99, &[CANNON_TIME_OPCODE, 0]).unwrap();
    let b = decode_cannon_time(-99, &[CANNON_TIME_OPCODE, 0xff]).unwrap();
    let facts = cannon_facts();
    let pa = plan_cannon_time(&a, &facts).unwrap();
    let pb = plan_cannon_time(&b, &facts).unwrap();
    assert_eq!(pa.state, pb.state);
    assert_eq!(pa.remaining_uses, pb.remaining_uses);
    assert_ne!(pa.effects[0], pb.effects[0]);
    assert_eq!(
        pa.state.active_player, 6,
        "PlayerData::who, not package.play"
    );
    assert!(decode_cannon_time(0, &[CANNON_TIME_OPCODE]).is_none());
}

#[test]
fn mutation_cannon_success_preserves_state_and_ordered_audio_boundaries() {
    let request = decode_cannon_time(4, &[CANNON_TIME_OPCODE, 0xa5]).unwrap();
    let facts = cannon_facts();
    let plan = plan_cannon_time(&request, &facts).unwrap();
    assert_eq!(plan.remaining_uses, Some(3));
    assert_eq!(
        plan.state,
        CannonTimeState {
            active_player: 6,
            start_frame: i32::MIN,
            saved_speed: 3,
            current_speed: 0,
            cannon_time_start: i32::MAX,
        }
    );
    assert_eq!(
        plan.effects,
        vec![
            CannonTimeEffect::Diagnostic {
                wire_state: 0xa5,
                frame: i32::MIN,
            },
            CannonTimeEffect::SetRemainingUses { who: 6, value: 3 },
            CannonTimeEffect::SetActivePlayer(6),
            CannonTimeEffect::SetStartFrame(i32::MIN),
            CannonTimeEffect::SetSavedSpeed(3),
            CannonTimeEffect::SetCannonTimeStart(i32::MAX),
            CannonTimeEffect::Ui(CannonTimeUi::StartedRemote { who: 6 }),
            CannonTimeEffect::Audio {
                category: SOUND_CANNON_STARTED,
            },
            CannonTimeEffect::SetCurrentSpeed(0),
            CannonTimeEffect::RetargetWallClock {
                target_time: CANNON_TARGET_TIME,
            },
            CannonTimeEffect::Ui(CannonTimeUi::SpeedChangedToZero),
            CannonTimeEffect::Audio {
                category: SOUND_CANNON_SPEED_CHANGED,
            },
        ]
    );
}

#[test]
fn mutation_cannon_denials_are_local_only_and_do_not_read_unneeded_charges() {
    let request = decode_cannon_time(0, &[CANNON_TIME_OPCODE, 1]).unwrap();
    let mut facts = cannon_facts();
    facts.package_player_who = Some(2);
    facts.state.active_player = 7;
    facts.remaining_uses = None;
    let active_local = plan_cannon_time(&request, &facts).unwrap();
    assert_eq!(
        &active_local.effects[1..],
        &[
            CannonTimeEffect::Ui(CannonTimeUi::AlreadyActive),
            CannonTimeEffect::Audio {
                category: SOUND_CANNON_DENIED,
            },
        ]
    );

    facts.package_player_who = Some(6);
    let active_remote = plan_cannon_time(&request, &facts).unwrap();
    assert_eq!(active_remote.effects.len(), 1);

    facts.state.active_player = -1;
    facts.package_player_who = Some(2);
    facts.remaining_uses = Some(0);
    let empty_local = plan_cannon_time(&request, &facts).unwrap();
    assert!(matches!(
        empty_local.effects[1],
        CannonTimeEffect::Ui(CannonTimeUi::NoUsesRemaining)
    ));

    facts.package_player_who = None;
    assert!(plan_cannon_time(&request, &facts).is_none());
}

#[test]
fn mutation_cannon_zero_speed_omits_pacing_and_second_sound_call() {
    let request = decode_cannon_time(0, &[CANNON_TIME_OPCODE, 1]).unwrap();
    let mut facts = cannon_facts();
    facts.state.current_speed = 0;
    let plan = plan_cannon_time(&request, &facts).unwrap();
    assert_eq!(plan.state.saved_speed, 0);
    assert!(!plan.effects.iter().any(|effect| matches!(
        effect,
        CannonTimeEffect::RetargetWallClock { .. }
            | CannonTimeEffect::Audio {
                category: SOUND_CANNON_SPEED_CHANGED,
            }
    )));
}

#[test]
fn mutation_receipts_recompute_and_reject_a_one_bit_plan_change() {
    let request = decode_cannon_time(0, &[CANNON_TIME_OPCODE, 1]).unwrap();
    let facts = cannon_facts();
    let plan = plan_cannon_time(&request, &facts).unwrap();
    let receipt = CannonTimeReceipt {
        request,
        facts: Some(facts),
        status: PlanStatus::Planned,
        plan: Some(plan.clone()),
    };
    assert!(receipt.validates(&request));

    let mut mutated = receipt;
    mutated.plan.as_mut().unwrap().state.start_frame ^= 1;
    assert!(!mutated.validates(&request));
    assert!(CannonTimeReceipt::unavailable(request).validates(&request));
}

#[test]
fn mutation_ungraceful_drop_preserves_bytes_and_exact_network_gate() {
    let request = decode_ungraceful_drop(&[UNGRACEFUL_DROP_OPCODE, 0xff, 3]).unwrap();
    assert_eq!(request.play, 0xff);
    assert_eq!(request.state, 3);

    let solo = plan_ungraceful_drop(request, 0xef);
    assert_eq!(
        solo.effects,
        vec![UngracefulDropEffect::Diagnostic {
            play: 0xff,
            state: 3,
        }]
    );
    assert!(!solo.downstream_required);

    let network = plan_ungraceful_drop(request, GAME_SEM_NETWORK_DROP | 0x80);
    assert_eq!(
        network.effects[1],
        UngracefulDropEffect::DelegateToDropControl {
            play: 0xff,
            state: 3,
        }
    );
    assert!(network.downstream_required);

    let receipt = UngracefulDropReceipt {
        request,
        game_semaphore: Some(GAME_SEM_NETWORK_DROP | 0x80),
        status: PlanStatus::Planned,
        plan: Some(network),
    };
    assert!(receipt.validates(request));
    let mut mutated = receipt;
    mutated.plan.as_mut().unwrap().downstream_required = false;
    assert!(!mutated.validates(request));

    assert!(decode_ungraceful_drop(&[UNGRACEFUL_DROP_OPCODE, 1]).is_none());
}
