#[path = "../src/systems/group_action_attack_deep_re.rs"]
mod lane;

use lane::*;

fn object(who: u8, o: i32, revision: u64) -> ObjectKey {
    ObjectKey { who, o, revision }
}

fn member(o: i32, domain: i32) -> MemberSnapshot {
    MemberSnapshot {
        key: object(2, o, o as u64 + 100),
        position: Coord {
            x: o * 10,
            y: o * 20,
        },
        valid: true,
        is_build: false,
        on_map: true,
        type_is_defense: false,
        is_special: false,
        is_type_0x3a: false,
        is_plane: false,
        is_plane_vslot_is_default: true,
        type_domain: domain,
        type_flags_0x2b4: 0,
        packing_or_unpacking: false,
        packing: false,
        mana: 0,
        mana_reserve_0x96: 0,
        order_kind: None,
        strafe: None,
        existing_attack: None,
        target_in_range: false,
        building_warning_invalid_range: false,
        melee_target: None,
    }
}

fn target() -> TargetSnapshot {
    TargetSnapshot {
        key: object(3, 77, 900),
        position: Coord { x: 4_000, y: 5_000 },
        uid_0x30: 0x1234,
        is_unit: true,
        is_wallbuild: false,
        is_build: false,
        is_active_build: false,
        capture_eligible: false,
        seen_by_group: true,
    }
}

fn attack_position() -> AttackPositionReceipt {
    AttackPositionReceipt {
        origin: Coord { x: 100, y: 200 },
        initial_in_range: true,
        called_find_attack_pos: false,
        find_succeeded: true,
        to: Coord { x: 100, y: 200 },
        rng: Vec::new(),
        world_digest: 0xfeed,
    }
}

fn request(members: Vec<MemberSnapshot>) -> AttackRequest {
    AttackRequest {
        group: GroupSnapshot {
            key: GroupKey {
                who: 2,
                slot: 9,
                id: 44,
                revision: 12,
                digest: 0x1122_3344,
            },
            on_map: true,
            buildings: false,
            is_local_player: false,
            disband: 7,
            order_num: 0x7fff_ffff,
            num: members.len() as i32,
            leader: members.first().map(|m| m.key),
            leader_position: Coord { x: 100, y: 200 },
            mandatory_one_loc_to: None,
            members,
        },
        ox: 77,
        whom: 3,
        mandatory: 1,
        queued: QueuePos::Last,
        ignore: 0,
        scenario: ScenarioSnapshot::default(),
        target: Some(target()),
        attack_position: Some(attack_position()),
        first_unavailable: None,
    }
}

#[test]
fn queue_first_is_the_insert_halt_reissue_new_finish_dance() {
    let mut r = request(vec![member(1, 0)]);
    r.queued = QueuePos::First;
    r.ignore = 0x55;

    let p = plan_action_attack(&r).unwrap();
    assert_eq!(p.exit, Exit::QueueFirstDelegated);
    assert_eq!(
        p.steps,
        vec![
            PlanStep::SetGroupDisband(0),
            PlanStep::QueueFirstDance {
                set_up_insert_va: GROUP_SET_UP_INSERT_VA,
                halt_arg: 0x55,
                recursive_queued: QueuePos::New,
                finish_insert_va: GROUP_FINISH_INSERT_VA,
            }
        ]
    );
}

#[test]
fn domain_passes_reorder_members_and_unseen_nonbuild_gets_move_then_attack() {
    let members = vec![member(20, 2), member(10, 0), member(15, 1)];
    let mut r = request(members);
    r.target.as_mut().unwrap().seen_by_group = false;

    let p = plan_action_attack(&r).unwrap();
    let mut installed = Vec::new();
    for step in &p.steps {
        match step {
            PlanStep::InstallMove { member, kind, .. } => installed.push((member.o, *kind)),
            PlanStep::InstallAttack { member, .. } => installed.push((member.o, OrderKind::Attack)),
            _ => {}
        }
    }
    assert_eq!(
        installed,
        vec![
            (10, OrderKind::MoveTo),
            (10, OrderKind::Attack),
            (15, OrderKind::MoveTo),
            (15, OrderKind::Attack),
            (20, OrderKind::MoveTo),
            (20, OrderKind::Attack),
        ]
    );
    assert!(!installed.iter().any(|(_, kind)| matches!(
        kind,
        OrderKind::AttackTo | OrderKind::ExploreTo | OrderKind::FleeTo
    )));
    assert_eq!(
        p.steps.last(),
        Some(&PlanStep::IncrementGroupOrderNum {
            before: 0x7fff_ffff,
            after: i32::MIN,
        })
    );
}

#[test]
fn packing_new_seen_clears_then_casts_then_appends_attack() {
    let mut pack = member(1, 0);
    pack.packing_or_unpacking = true;
    pack.packing = true;
    pack.target_in_range = false;
    let mut deploy = member(2, 0);
    deploy.packing_or_unpacking = true;
    deploy.packing = false;
    deploy.target_in_range = true;
    let mut r = request(vec![pack, deploy]);
    r.queued = QueuePos::New;

    let p = plan_action_attack(&r).unwrap();
    let body = p
        .steps
        .iter()
        .filter(|step| {
            matches!(
                step,
                PlanStep::ClearOrders { .. }
                    | PlanStep::InstallCast { .. }
                    | PlanStep::InstallAttack { .. }
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        body,
        vec![
            PlanStep::ClearOrders {
                member: object(2, 1, 101)
            },
            PlanStep::InstallCast {
                member: object(2, 1, 101),
                spell: SPELL_PACK,
                queue: QueuePos::New,
                raw_first_four: [-1; 4],
                raw_last: 0,
            },
            PlanStep::InstallAttack {
                member: object(2, 1, 101),
                target: object(3, 77, 900),
                queue: QueuePos::Last,
                mandatory: 1,
                raw_last: 1,
            },
            PlanStep::ClearOrders {
                member: object(2, 2, 102)
            },
            PlanStep::InstallCast {
                member: object(2, 2, 102),
                spell: SPELL_DEPLOY,
                queue: QueuePos::New,
                raw_first_four: [-1; 4],
                raw_last: 0,
            },
            PlanStep::InstallAttack {
                member: object(2, 2, 102),
                target: object(3, 77, 900),
                queue: QueuePos::Last,
                mandatory: 1,
                raw_last: 1,
            },
        ]
    );
}

#[test]
fn qualifying_strafe_is_retargeted_once_per_domain_pass() {
    let mut plane = member(4, 2);
    plane.is_plane = true;
    plane.mana = 9;
    plane.mana_reserve_0x96 = 3;
    plane.order_kind = Some(OrderKind::Strafe);
    plane.strafe = Some(StrafeReceipt { order_revision: 88 });

    let p = plan_action_attack(&request(vec![plane])).unwrap();
    let passes = p
        .steps
        .iter()
        .filter_map(|step| match step {
            PlanStep::RetargetStrafe {
                domain_pass,
                target_uid,
                field_0x3c,
                ..
            } => {
                assert_eq!((*target_uid, *field_0x3c), (0x1234, 0));
                Some(*domain_pass)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(passes, vec![0, 1, 2]);
}

#[test]
fn capture_eligible_build_calls_capture_then_delegates_literal_move_to() {
    let mut r = request(vec![member(8, 0)]);
    let t = r.target.as_mut().unwrap();
    t.is_unit = false;
    t.is_build = true;
    t.is_active_build = true;
    t.capture_eligible = true;

    let p = plan_action_attack(&r).unwrap();
    assert_eq!(p.exit, Exit::CaptureMoveDelegated);
    assert_eq!(
        &p.steps[2..],
        &[
            PlanStep::CheckCapture {
                building: object(3, 77, 900),
                attacker: object(2, 8, 108),
            },
            PlanStep::DelegateMoveTo {
                destination: Coord { x: 4_000, y: 5_000 },
                queue: QueuePos::Last,
                order: OrderKind::MoveTo,
                raw_tail: [0, 0, 1, 1, -1, -1],
            },
        ]
    );
}

#[test]
fn existing_attack_preservation_skips_the_member() {
    let mut m = member(5, 0);
    m.existing_attack = Some(ExistingAttackReceipt {
        target: object(3, 77, 1),
        active_byte_0x1c: true,
        unit_counter_0xd8: 1,
        primary_in_range: false,
        current_order_is_attack: false,
        payload_in_range: false,
    });
    let p = plan_action_attack(&request(vec![m])).unwrap();
    assert!(!p.steps.iter().any(|step| matches!(
        step,
        PlanStep::InstallAttack { .. } | PlanStep::InstallMove { .. }
    )));
}

#[test]
fn mandatory_zero_records_melee_probe_and_uses_its_nonnegative_target() {
    let mut m = member(6, 0);
    let replacement = object(4, 91, 777);
    m.melee_target = Some(MeleeTargetReceipt {
        requested_distance: 0x500,
        world_dimension_times_0x240: 0x9000,
        target_class_arg: 1,
        result: Some(replacement),
    });
    let mut r = request(vec![m]);
    r.mandatory = 0;

    let p = plan_action_attack(&r).unwrap();
    assert!(p.steps.iter().any(|step| matches!(
        step,
        PlanStep::MeleeTargetProbe {
            member,
            receipt: MeleeTargetReceipt {
                target_class_arg: 1,
                ..
            }
        } if member.o == 6
    )));
    assert!(p.steps.iter().any(|step| matches!(
        step,
        PlanStep::InstallAttack { target, .. } if *target == replacement
    )));
}

#[test]
fn building_group_writes_exact_three_fields_and_returns() {
    let mut r = request(vec![member(3, 0)]);
    r.group.buildings = true;
    r.group.members[0].is_build = true;
    let p = plan_action_attack(&r).unwrap();
    assert_eq!(p.exit, Exit::BuildingsHandled);
    assert_eq!(
        p.steps.last(),
        Some(&PlanStep::BuildingAttack {
            building: object(2, 3, 103),
            or_flags_0x60: 4,
            target_o_0x7c: 77,
            target_who_0x81: 3,
        })
    );
}

#[test]
fn unavailable_host_read_and_bad_rng_receipt_fail_closed() {
    let mut r = request(vec![member(1, 0)]);
    let boundary = ExternalBoundary {
        call_va: 0x0071_24b0,
        callee_va: GROUP_IS_ON_MAP_VA,
        symbol: "GroupData::is_on_map",
        detail: "canonical group owner unavailable",
    };
    r.first_unavailable = Some(boundary.clone());
    assert_eq!(plan_action_attack(&r), Err(PlanError::External(boundary)));

    r.first_unavailable = None;
    r.attack_position = Some(AttackPositionReceipt {
        origin: Coord { x: 100, y: 200 },
        initial_in_range: false,
        called_find_attack_pos: true,
        find_succeeded: true,
        to: Coord::default(),
        rng: vec![RngDraw {
            call_va: RANDOM_GET_VA_IN_FIND_ATTACK_POS,
            rng_va: RANDOM_GET_VA,
            low: 1,
            high: 0xffff,
            value: 7,
            state_before: 10,
            state_after: 20,
        }],
        world_digest: 9,
    });
    assert_eq!(
        plan_action_attack(&r),
        Err(PlanError::InvalidRngReceipt { draw_index: 0 })
    );
}
