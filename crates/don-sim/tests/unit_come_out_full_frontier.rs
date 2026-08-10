#[path = "../src/systems/unit_come_out_full_frontier.rs"]
mod frontier;

use frontier::*;

fn object(owner: i8, object: i16) -> ObjectIdentity {
    ObjectIdentity::new(owner, object)
}

fn point(x: i32, y: i32) -> Point {
    Point { x, y }
}

fn stamp(seed: u32, draws: u64) -> RngStamp {
    RngStamp { seed, draws }
}

fn constants() -> ExitConstants {
    ExitConstants {
        land_min: 0x80,
        land_max: 0x140,
        water_min: 0xa0,
        water_max: 0x180,
        ordinary_padding: 0x180,
    }
}

fn container(identity: ObjectIdentity, location: Point) -> ContainerFacts {
    ContainerFacts {
        identity,
        point: location,
        angle: 0x1122_3344,
        matches_university: false,
        matches_oil_platform: false,
        is_build: false,
        is_wallbuild: false,
        gpiece: 9,
        object_flags_low: 0,
        type_block_radius: 0x40,
        type_x_size: 3,
        type_y_size: 2,
        gather: None,
    }
}

fn base_facts() -> UnitComeOutPrefixFacts {
    UnitComeOutPrefixFacts {
        actor: object(2, 17),
        argument: 0,
        point: point(100, 200),
        actor_type: 77,
        actor_domain: 0,
        actor_obj_masks: 0,
        actor_block_radius: 0x20,
        actor_big_radius: 0x30,
        actor_is_captain: true,
        captain_object: None,
        guy_hint_lengths: vec![4, 0],
        first_guy_turret_inc_bits: None,
        constants: constants(),
        initial_rng: stamp(0x1111_2222, 5),
        captain_recursive: None,
        inside: None,
        uncontained_searches: vec![],
    }
}

fn nearby_request(
    facts: &UnitComeOutPrefixFacts,
    centre: Point,
    min_radius: i32,
    max_radius: i32,
    base_angle: u32,
    filter: i32,
    accept_without_collision: i32,
    expanded: i32,
) -> NearbyRequest {
    NearbyRequest {
        receiver_type: facts.actor_type,
        centre,
        min_radius,
        max_radius,
        radial_step: 0,
        base_angle,
        filter,
        actor: facts.actor,
        accept_without_collision,
        expanded,
        overlap_object: -1,
        overlap_owner: 0,
        required_region: -1,
    }
}

fn observation(request: NearbyRequest, outcome: NearbyOutcome) -> NearbyObservation {
    NearbyObservation { request, outcome }
}

fn receipt(
    kind: ChildCallKind,
    result: i32,
    before: RngStamp,
    after: RngStamp,
) -> ChildCallReceipt {
    ChildCallReceipt {
        kind,
        result,
        before,
        after,
    }
}

#[test]
fn tranche_is_exactly_the_first_2724_bytes_and_leaves_7201() {
    assert_eq!(UNIT_COME_OUT_VA, 0x0061_7c10);
    assert_eq!(UNIT_COME_OUT_BYTES, 9_925);
    assert_eq!(UNIT_COME_OUT_END_VA, 0x0061_a2d5);
    assert_eq!(PREFIX_END_VA, 0x0061_86b4);
    assert_eq!(PREFIX_BYTES, 2_724);
    assert_eq!(RESIDUAL_BYTES, 7_201);
    assert!(PREFIX_DIRECT_RANDOM_GET_CALL_VAS.is_empty());
    assert_eq!(
        RESIDUAL_DIRECT_RANDOM_GET_CALL_VAS,
        [0x0061_a1bb, 0x0061_a1d5]
    );
}

#[test]
fn uncontained_path_uses_strict_then_permissive_search_and_preserves_rng() {
    let mut facts = base_facts();
    let first = nearby_request(
        &facts,
        facts.point,
        0x20,
        0x1a0,
        INITIAL_EXIT_ANGLE,
        3,
        0,
        0,
    );
    let second = NearbyRequest {
        accept_without_collision: 1,
        ..first
    };
    facts.uncontained_searches = vec![
        observation(first, NearbyOutcome::Blocked),
        observation(second, NearbyOutcome::Found(point(140, 230))),
    ];

    let plan = plan_unit_come_out_prefix(&facts).unwrap();
    assert_eq!(
        &plan.steps[..3],
        &[
            PrefixStep::ClearScratchGroup {
                call_va: 0x0061_7c66,
                argument: -1,
            },
            PrefixStep::ClearGuyAnimHints {
                store_va: 0x0061_7c8f,
                guy_index: 0,
                before: 4,
            },
            PrefixStep::ClearGuyAnimHints {
                store_va: 0x0061_7c8f,
                guy_index: 1,
                before: 0,
            },
        ]
    );
    assert_eq!(
        plan.exit,
        UnitComeOutPrefixExit::ContinueAtCommonRelease(UnitComeOutContinuation {
            resume_va: 0x0061_86b4,
            actor: facts.actor,
            point: point(140, 230),
            z: None,
            direct_container: None,
            placement_container: None,
            container_gpiece: 0,
            rng: facts.initial_rng,
        })
    );
    assert!(matches!(
        plan.steps[3],
        PrefixStep::NearbySearch {
            call_va: 0x0061_7d82,
            ..
        }
    ));
    assert!(matches!(
        plan.steps[4],
        PrefixStep::NearbySearch {
            call_va: 0x0061_7dd1,
            ..
        }
    ));
}

#[test]
fn captain_redirect_returns_exact_child_result_and_rng_after_hint_clear() {
    let mut facts = base_facts();
    facts.actor_is_captain = false;
    facts.captain_object = Some(7);
    let after = stamp(0x2222_3333, 8);
    facts.captain_recursive = Some(receipt(
        ChildCallKind::CaptainComeOut,
        1,
        facts.initial_rng,
        after,
    ));

    let plan = plan_unit_come_out_prefix(&facts).unwrap();
    assert_eq!(
        plan.steps.last(),
        Some(&PrefixStep::CaptainRecursiveComeOut {
            call_va: 0x0061_7cf7,
            captain: object(2, 7),
            receipt: facts.captain_recursive.unwrap(),
        })
    );
    assert_eq!(
        plan.exit,
        UnitComeOutPrefixExit::Returned {
            value: 1,
            rng: after,
        }
    );
}

#[test]
fn oil_platform_success_orders_transport_bridge_mutations_and_receipts() {
    let mut facts = base_facts();
    let direct = object(2, 8);
    let transport = object(2, 20);
    let mut oil = container(direct, point(500, 600));
    oil.matches_oil_platform = true;
    let after_init = stamp(0x3333_4444, 7);
    let after_recursive = stamp(0x5555_6666, 11);
    facts.inside = Some(InsideFacts {
        direct_container: oil,
        placement_container: None,
        leader_flags_before: None,
        oil_bridge: Some(OilPlatformBridgeFacts {
            current_transport_upgrade: -1,
            transport: Some(transport),
            init: receipt(
                ChildCallKind::InitTransport,
                i32::from(transport.object),
                facts.initial_rng,
                after_init,
            ),
            recursive_come_out: Some(receipt(
                ChildCallKind::TransportComeOut,
                0,
                after_init,
                after_recursive,
            )),
            failed_transport_die: None,
        }),
        placement_searches: vec![],
        terrain_z: None,
    });

    let plan = plan_unit_come_out_prefix(&facts).unwrap();
    assert_eq!(
        &plan.steps[3..],
        &[
            PrefixStep::InitTransport {
                call_va: 0x0061_7eed,
                container: direct,
                type_index: TRANSPORT_BARGE_TYPE,
                receipt: receipt(
                    ChildCallKind::InitTransport,
                    20,
                    facts.initial_rng,
                    after_init,
                ),
            },
            PrefixStep::SameDamage {
                call_va: 0x0061_7f28,
                transport,
                actor: facts.actor,
            },
            PrefixStep::InsertInside {
                call_va: 0x0061_7f41,
                object: transport,
                container: facts.actor,
            },
            PrefixStep::RecursiveTransportComeOut {
                call_va: 0x0061_7f52,
                transport,
                receipt: receipt(
                    ChildCallKind::TransportComeOut,
                    0,
                    after_init,
                    after_recursive,
                ),
            },
            PrefixStep::RemoveFromInside {
                call_va: 0x0061_7f99,
                object: facts.actor,
            },
            PrefixStep::InsertInside {
                call_va: 0x0061_7fa8,
                object: facts.actor,
                container: transport,
            },
        ]
    );
    assert_eq!(
        plan.exit,
        UnitComeOutPrefixExit::Returned {
            value: 0,
            rng: after_recursive,
        }
    );
}

#[test]
fn oil_platform_allocation_failure_returns_one_without_link_mutations() {
    let mut facts = base_facts();
    let direct = object(2, 8);
    let mut oil = container(direct, point(500, 600));
    oil.matches_oil_platform = true;
    let after_init = stamp(0x3333_4444, 7);
    let init = receipt(
        ChildCallKind::InitTransport,
        -1,
        facts.initial_rng,
        after_init,
    );
    facts.inside = Some(InsideFacts {
        direct_container: oil,
        placement_container: None,
        leader_flags_before: None,
        oil_bridge: Some(OilPlatformBridgeFacts {
            current_transport_upgrade: -1,
            transport: None,
            init,
            recursive_come_out: None,
            failed_transport_die: None,
        }),
        placement_searches: vec![],
        terrain_z: None,
    });

    let plan = plan_unit_come_out_prefix(&facts).unwrap();
    assert_eq!(
        plan.steps.last(),
        Some(&PrefixStep::InitTransport {
            call_va: 0x0061_7eed,
            container: direct,
            type_index: TRANSPORT_BARGE_TYPE,
            receipt: init,
        })
    );
    assert_eq!(
        plan.exit,
        UnitComeOutPrefixExit::Returned {
            value: 1,
            rng: after_init,
        }
    );
}

#[test]
fn oil_platform_recursive_failure_dispatches_death_after_continuous_rng_receipts() {
    let mut facts = base_facts();
    let direct = object(2, 8);
    let transport = object(2, 20);
    let mut oil = container(direct, point(500, 600));
    oil.matches_oil_platform = true;
    let after_init = stamp(0x3333_4444, 7);
    let after_recursive = stamp(0x5555_6666, 11);
    let after_die = stamp(0x7777_8888, 12);
    let die = receipt(ChildCallKind::TransportDie, 0, after_recursive, after_die);
    facts.inside = Some(InsideFacts {
        direct_container: oil,
        placement_container: None,
        leader_flags_before: None,
        oil_bridge: Some(OilPlatformBridgeFacts {
            current_transport_upgrade: 0x141,
            transport: Some(transport),
            init: receipt(
                ChildCallKind::InitTransport,
                20,
                facts.initial_rng,
                after_init,
            ),
            recursive_come_out: Some(receipt(
                ChildCallKind::TransportComeOut,
                1,
                after_init,
                after_recursive,
            )),
            failed_transport_die: Some(die),
        }),
        placement_searches: vec![],
        terrain_z: None,
    });

    let plan = plan_unit_come_out_prefix(&facts).unwrap();
    assert_eq!(
        plan.steps.last(),
        Some(&PrefixStep::DieTransport {
            call_va: 0x0061_7f84,
            transport,
            receipt: die,
        })
    );
    assert_eq!(
        plan.exit,
        UnitComeOutPrefixExit::Returned {
            value: 1,
            rng: after_die,
        }
    );
}

#[test]
fn child_rng_receipts_must_join_exactly_at_the_preceding_stamp() {
    let mut facts = base_facts();
    facts.actor_is_captain = false;
    facts.captain_object = Some(7);
    facts.captain_recursive = Some(receipt(
        ChildCallKind::CaptainComeOut,
        0,
        stamp(0x9999_aaaa, 5),
        stamp(0xbbbb_cccc, 6),
    ));

    assert_eq!(
        plan_unit_come_out_prefix(&facts),
        Err(UnitComeOutPrefixError::ReceiptContinuity)
    );
}

#[test]
fn direct_container_mask_sets_turret_point_z_and_unlinks_before_continuation() {
    let mut facts = base_facts();
    facts.actor_obj_masks = DIRECT_CONTAINER_LOCATION_MASK;
    facts.first_guy_turret_inc_bits = Some(0x3f80_0000);
    let direct = container(object(2, 8), point(500, 600));
    facts.inside = Some(InsideFacts {
        direct_container: direct,
        placement_container: None,
        leader_flags_before: None,
        oil_bridge: None,
        placement_searches: vec![],
        terrain_z: Some(23),
    });

    let plan = plan_unit_come_out_prefix(&facts).unwrap();
    assert_eq!(
        &plan.steps[3..],
        &[
            PrefixStep::SetFirstGuyTurretIncrement {
                store_va: 0x0061_8003,
                before_bits: 0x3f80_0000,
                after_bits: FIRST_GUY_TURRET_INC_90_BITS,
            },
            PrefixStep::SetActorPoint {
                x_store_va: 0x0061_8667,
                y_store_va: 0x0061_8675,
                before: facts.point,
                after: direct.point,
            },
            PrefixStep::SetActorZ {
                terrain_call_va: 0x0061_8694,
                store_va: 0x0061_86a4,
                after: 23,
            },
            PrefixStep::RemoveFromInside {
                call_va: 0x0061_86a7,
                object: facts.actor,
            },
        ]
    );
    assert!(matches!(
        plan.exit,
        UnitComeOutPrefixExit::ContinueAtCommonRelease(UnitComeOutContinuation {
            resume_va: 0x0061_86b4,
            point: Point { x: 500, y: 600 },
            z: Some(23),
            ..
        })
    ));
}

#[test]
fn zero_block_radius_doubles_permissive_radii_then_falls_back_to_container() {
    let mut facts = base_facts();
    facts.actor_block_radius = 0;
    let direct = container(object(2, 8), point(500, 600));
    let first = nearby_request(&facts, direct.point, 0x40, 0x1c0, direct.angle, 0, 0, 0);
    let second = NearbyRequest {
        min_radius: 0x80,
        max_radius: 0x380,
        accept_without_collision: 1,
        ..first
    };
    facts.inside = Some(InsideFacts {
        direct_container: direct,
        placement_container: Some(direct),
        leader_flags_before: None,
        oil_bridge: None,
        placement_searches: vec![
            observation(first, NearbyOutcome::Blocked),
            observation(second, NearbyOutcome::Blocked),
        ],
        terrain_z: Some(31),
    });

    let plan = plan_unit_come_out_prefix(&facts).unwrap();
    assert!(matches!(
        plan.steps[3],
        PrefixStep::NearbySearch {
            call_va: 0x0061_84ee,
            ..
        }
    ));
    assert!(matches!(
        plan.steps[4],
        PrefixStep::NearbySearch {
            call_va: 0x0061_855c,
            ..
        }
    ));
    assert!(matches!(
        plan.exit,
        UnitComeOutPrefixExit::ContinueAtCommonRelease(UnitComeOutContinuation {
            point: Point { x: 500, y: 600 },
            z: Some(31),
            ..
        })
    ));
}

#[test]
fn nonzero_block_radius_second_collision_failure_returns_one_without_unlink() {
    let mut facts = base_facts();
    let direct = container(object(2, 8), point(500, 600));
    let first = nearby_request(&facts, direct.point, 0x40, 0x1c0, direct.angle, 3, 0, 0);
    let second = NearbyRequest {
        accept_without_collision: 1,
        ..first
    };
    facts.inside = Some(InsideFacts {
        direct_container: direct,
        placement_container: Some(direct),
        leader_flags_before: None,
        oil_bridge: None,
        placement_searches: vec![
            observation(first, NearbyOutcome::Blocked),
            observation(second, NearbyOutcome::Blocked),
        ],
        terrain_z: None,
    });

    let plan = plan_unit_come_out_prefix(&facts).unwrap();
    assert_eq!(
        plan.exit,
        UnitComeOutPrefixExit::Returned {
            value: 1,
            rng: facts.initial_rng,
        }
    );
    assert!(!plan
        .steps
        .iter()
        .any(|step| matches!(step, PrefixStep::RemoveFromInside { .. })));
}

#[test]
fn nonzero_argument_noncaptain_uses_same_owner_captain_as_placement_container() {
    let mut facts = base_facts();
    facts.argument = 1;
    facts.actor_is_captain = false;
    facts.captain_object = Some(7);
    let direct = container(object(4, 8), point(480, 580));
    let placement = container(object(2, 7), point(500, 600));
    let request = nearby_request(
        &facts,
        placement.point,
        0x40,
        0x1c0,
        placement.angle,
        3,
        0,
        0,
    );
    facts.inside = Some(InsideFacts {
        direct_container: direct,
        placement_container: Some(placement),
        leader_flags_before: None,
        oil_bridge: None,
        placement_searches: vec![observation(request, NearbyOutcome::Found(point(620, 600)))],
        terrain_z: Some(35),
    });

    let plan = plan_unit_come_out_prefix(&facts).unwrap();
    assert!(matches!(
        plan.exit,
        UnitComeOutPrefixExit::ContinueAtCommonRelease(UnitComeOutContinuation {
            direct_container: Some(ObjectIdentity {
                owner: 4,
                object: 8
            }),
            placement_container: Some(ObjectIdentity {
                owner: 2,
                object: 7
            }),
            container_gpiece: 9,
            ..
        })
    ));
}

#[test]
fn wallbuild_water_radii_include_span_big_radius_and_flagged_minimum() {
    let mut facts = base_facts();
    facts.actor_domain = 1;
    let mut direct = container(object(2, 8), point(500, 600));
    direct.is_wallbuild = true;
    direct.object_flags_low = 1;
    let request = nearby_request(
        &facts,
        direct.point,
        0x1c0,
        0x2a0,
        INITIAL_EXIT_ANGLE,
        3,
        0,
        0,
    );
    facts.inside = Some(InsideFacts {
        direct_container: direct,
        placement_container: Some(direct),
        leader_flags_before: None,
        oil_bridge: None,
        placement_searches: vec![observation(request, NearbyOutcome::Found(point(620, 600)))],
        terrain_z: Some(35),
    });

    let plan = plan_unit_come_out_prefix(&facts).unwrap();
    assert!(plan.steps.contains(&PrefixStep::NearbySearch {
        call_va: 0x0061_85de,
        observation: observation(request, NearbyOutcome::Found(point(620, 600))),
    }));
    assert!(matches!(
        plan.exit,
        UnitComeOutPrefixExit::ContinueAtCommonRelease(UnitComeOutContinuation {
            point: Point { x: 620, y: 600 },
            container_gpiece: 9,
            ..
        })
    ));
}

#[test]
fn build_gather_probe_uses_authoritative_angle_for_final_search() {
    let mut facts = base_facts();
    let mut direct = container(object(2, 8), point(500, 600));
    direct.is_build = true;
    direct.matches_university = true;
    let gather_source = object(3, 25);
    let gather_request = nearby_request(
        &facts,
        point(900, 700),
        0,
        0x600,
        GATHER_EXIT_ANGLE,
        3,
        0,
        0,
    );
    direct.gather = Some(GatherGateFacts {
        list_non_empty: true,
        gather_inside: false,
        direction: Some(GatherDirectionFacts {
            source: GatherPointSource::Object {
                identity: gather_source,
                point: point(900, 700),
            },
            observation: observation(gather_request, NearbyOutcome::Found(point(760, 600))),
            angle_after: Some(0x4000_0000),
        }),
    });
    let final_request = nearby_request(&facts, direct.point, 0x40, 0x1c0, 0x4000_0000, 3, 0, 0);
    facts.inside = Some(InsideFacts {
        direct_container: direct,
        placement_container: Some(direct),
        leader_flags_before: Some(0x10),
        oil_bridge: None,
        placement_searches: vec![observation(
            final_request,
            NearbyOutcome::Found(point(700, 600)),
        )],
        terrain_z: Some(40),
    });

    let plan = plan_unit_come_out_prefix(&facts).unwrap();
    assert!(plan.steps.contains(&PrefixStep::SetLeaderFlags {
        store_va: 0x0061_80ea,
        owner: facts.actor.owner,
        before: 0x10,
        after: 0x0200_0010,
    }));
    assert!(plan.steps.contains(&PrefixStep::CopyGatherHotKey {
        call_va: 0x0061_81f6,
        source: gather_source,
        actor: facts.actor,
    }));
    assert!(plan.steps.contains(&PrefixStep::SetContainerAngle {
        store_va: 0x0061_8374,
        container: direct.identity,
        before: direct.angle,
        after: 0x4000_0000,
    }));
    assert!(plan.steps.iter().any(|step| matches!(
        step,
        PrefixStep::NearbySearch {
            call_va: 0x0061_85de,
            observation: NearbyObservation {
                request: NearbyRequest {
                    base_angle: 0x4000_0000,
                    ..
                },
                ..
            },
        }
    )));
}

#[test]
fn missing_canonical_gather_angle_is_rejected_instead_of_approximated() {
    let mut facts = base_facts();
    let mut direct = container(object(2, 8), point(500, 600));
    direct.is_build = true;
    let gather_request = nearby_request(
        &facts,
        point(900, 700),
        0,
        0x600,
        GATHER_EXIT_ANGLE,
        3,
        0,
        0,
    );
    direct.gather = Some(GatherGateFacts {
        list_non_empty: true,
        gather_inside: false,
        direction: Some(GatherDirectionFacts {
            source: GatherPointSource::Coordinate(point(900, 700)),
            observation: observation(gather_request, NearbyOutcome::Found(point(760, 600))),
            angle_after: None,
        }),
    });
    facts.inside = Some(InsideFacts {
        direct_container: direct,
        placement_container: Some(direct),
        leader_flags_before: None,
        oil_bridge: None,
        placement_searches: vec![],
        terrain_z: None,
    });

    assert_eq!(
        plan_unit_come_out_prefix(&facts),
        Err(UnitComeOutPrefixError::Missing(MissingFact::GatherAngle))
    );
}

#[test]
fn search_tuple_mismatch_is_a_hard_error() {
    let mut facts = base_facts();
    let wrong = nearby_request(
        &facts,
        facts.point,
        0x20,
        0x1a1,
        INITIAL_EXIT_ANGLE,
        3,
        0,
        0,
    );
    facts.uncontained_searches = vec![observation(wrong, NearbyOutcome::Blocked)];

    assert!(matches!(
        plan_unit_come_out_prefix(&facts),
        Err(UnitComeOutPrefixError::SearchRequestMismatch { .. })
    ));
}
