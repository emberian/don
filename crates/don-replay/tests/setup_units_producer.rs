#[path = "../src/setup_units_producer.rs"]
mod producer;

use producer::*;

fn type_facts(base: i32, squad: i32) -> TypeResolutionFacts {
    TypeResolutionFacts {
        base,
        tribe_can_base: true,
        nation_variant: base,
        build_units_upgrade: base,
        place_unit_upgrade: base,
        uber_size: 1,
        squad_size: squad,
        crew_size: 0,
    }
}

fn inputs(starting_town: i32) -> BuildUnitsInputs {
    BuildUnitsInputs {
        owner: 2,
        start_index: 4,
        center_city_o: 2_000,
        start_tile_x: 20,
        start_tile_y: 30,
        starting_town,
        starting_resources: 0,
        reveal_map: 0,
        bonuses: StartingUnitBonuses::default(),
        rules: StartingUnitRuleFacts::default(),
        types: StartingUnitTypeFacts {
            scout: type_facts(BASE_SCOUT_TYPE, 1),
            citizen: type_facts(BASE_PEASANT_TYPE, 1),
            dutch_merchant: type_facts(DUTCH_MERCHANT_TYPE, 1),
        },
    }
}

#[test]
fn native_extents_switch_table_and_call_sites_are_frozen() {
    assert_eq!(
        (SETUP_BUILD_UNITS_VA, SETUP_BUILD_UNITS_BYTES),
        (0x005a_afc0, 1_952)
    );
    assert_eq!(
        (SETUP_PLACE_UNIT_VA, SETUP_PLACE_UNIT_BYTES),
        (0x005a_bca0, 749)
    );
    assert_eq!(SETUP_GET_STARTING_CITIZENS_VA, 0x005a_af50);
    assert_eq!(PLACE_UNIT_DIRECT_RANDOM_CALL_VA, 0x005a_bd76);
    assert_eq!(
        [0, 1, 2, 3].map(|starting_town| starting_citizen_counts(starting_town).unwrap()),
        [
            StartingCitizenCounts {
                total: 3,
                fixed_building_prefix: 0
            },
            StartingCitizenCounts {
                total: 4,
                fixed_building_prefix: 0
            },
            StartingCitizenCounts {
                total: 5,
                fixed_building_prefix: 2
            },
            StartingCitizenCounts {
                total: 10,
                fixed_building_prefix: 5
            },
        ]
    );
    assert_eq!(DUTCH_MERCHANT_CALL_VAS, [0x005a_b1c8, 0x005a_b1e3]);
}

#[test]
fn starting_town_zero_has_three_citizens_and_no_scout() {
    let plan = build_units_plan(inputs(0)).unwrap();
    assert_eq!(plan.citizens_after_modifiers, 3);
    assert_eq!(plan.stop, None);
    assert_eq!(plan.calls.len(), 3);
    assert!(plan
        .calls
        .iter()
        .all(|call| matches!(call.phase, StartingUnitPhase::Citizen { .. })));
    assert!(plan
        .calls
        .iter()
        .all(|call| call.call_va == CITIZEN_SIMPLE_CALL_VA));
    assert_eq!(
        (plan.calls[0].requested_x, plan.calls[0].requested_y),
        (15_744, 23_424)
    );
}

#[test]
fn planner_requires_the_center_build_identity_from_build_cities() {
    let mut i = inputs(0);
    i.center_city_o = -1;
    assert_eq!(
        build_units_plan(i),
        Err(BuildUnitsPlanError::CenterCityOutsideBuildBand { center_city_o: -1 })
    );
}

#[test]
fn starting_town_one_preserves_scout_bonus_merchant_then_citizen_order() {
    let mut i = inputs(1);
    i.starting_resources = 7;
    i.reveal_map = 2;
    i.bonuses.extra_scouts = true;
    i.bonuses.dutch_merchants = true;
    i.bonuses.extra_citizens = true;
    i.rules.bonus_scouts = 2;
    i.rules.bonus_citizens = 2;
    let plan = build_units_plan(i).unwrap();

    assert_eq!(plan.citizens_after_modifiers, 14);
    assert_eq!(plan.calls.len(), 1 + 2 + 1 + 2 + 14);
    assert!(matches!(plan.calls[0].phase, StartingUnitPhase::BaseScout));
    assert!(matches!(
        plan.calls[1].phase,
        StartingUnitPhase::BonusScout { index: 0 }
    ));
    assert!(matches!(
        plan.calls[2].phase,
        StartingUnitPhase::BonusScout { index: 1 }
    ));
    assert!(matches!(
        plan.calls[3].phase,
        StartingUnitPhase::BonusScoutRevealMap
    ));
    assert!(matches!(
        plan.calls[4].phase,
        StartingUnitPhase::DutchMerchant { index: 0 }
    ));
    assert!(matches!(
        plan.calls[5].phase,
        StartingUnitPhase::DutchMerchant { index: 1 }
    ));
    assert!(plan.calls[6..]
        .iter()
        .all(|c| matches!(c.phase, StartingUnitPhase::Citizen { .. })));
    assert!(plan
        .calls
        .iter()
        .enumerate()
        .all(|(n, c)| c.ordinal == n as u32));
}

#[test]
fn type_resolution_retains_nation_and_both_upgrade_steps() {
    let mut i = inputs(1);
    i.types.citizen = TypeResolutionFacts {
        base: BASE_PEASANT_TYPE,
        tribe_can_base: false,
        nation_variant: 51,
        build_units_upgrade: 80,
        place_unit_upgrade: 81,
        uber_size: 1,
        squad_size: 2,
        crew_size: 1,
    };
    let plan = build_units_plan(i).unwrap();
    let citizen = plan.calls.last().unwrap();
    assert_eq!(
        (
            citizen.base_type,
            citizen.selected_before_upgrade,
            citizen.build_units_upgrade,
            citizen.place_unit_upgrade,
            citizen.squad_size,
            citizen.crew_size,
        ),
        (50, 51, 80, 81, 2, 1)
    );
}

#[test]
fn scholar_and_late_starting_town_boundaries_return_nonmutating_prefixes() {
    let mut scholar = inputs(1);
    scholar.bonuses.scholars = true;
    scholar.rules.bonus_scholars = -1;
    let plan = build_units_plan(scholar).unwrap();
    assert_eq!(
        plan.calls.len(),
        1,
        "the Scout prefix precedes the scholar search"
    );
    assert_eq!(
        plan.stop,
        Some(BuildUnitsStop::ScholarBuildingAndContainment {
            boundary_va: SCHOLAR_BUILDING_SEARCH_VA,
            scholar_count: -1,
        })
    );

    let mut late = inputs(2);
    late.bonuses.starting_town_citizens = true;
    late.bonuses.fewer_citizens = true;
    late.bonuses.extra_citizens = true;
    late.rules.starting_town_citizens = 8;
    late.rules.bonus_citizens = 2;
    let plan = build_units_plan(late).unwrap();
    assert_eq!(plan.citizens_after_modifiers, 9); // 5 + (8-3) - 3 + 2
    assert_eq!(plan.calls.len(), 1);
    assert_eq!(
        plan.stop,
        Some(BuildUnitsStop::ExistingBuildingCitizenSelection {
            boundary_va: CITIZEN_EXISTING_BUILDING_BRANCH_VA,
            fixed_building_prefix: 2,
        })
    );
}

fn member(call: PlaceUnitCall, id: u32, generation: u32, o: i32) -> UnitMemberAuthorityReceipt {
    let guy_len = call.squad_size + call.crew_size;
    UnitMemberAuthorityReceipt {
        identity: StableUnitIdentityReceipt {
            id,
            generation,
            owner: call.owner,
            o,
        },
        ptype_index: call.place_unit_upgrade,
        launching_is_null: true,
        path: EngineContainerShapeReceipt {
            length: 0,
            capacity: 10,
            increment: -1,
            flags: 0,
        },
        order_count: 0,
        guys: EngineContainerShapeReceipt {
            length: guy_len,
            capacity: guy_len,
            increment: 1,
            flags: 0,
        },
        guy_mark: call.squad_size as i8,
        guy_identities: (0..guy_len)
            .map(|slot| GuyIdentityReceipt {
                slot,
                who: call.owner as i8,
                o: o as i16,
                guy_num: slot as i8,
            })
            .collect(),
        units_authority_key: (id, generation),
        guys_authority_key: (id, generation),
    }
}

fn direct_draw(state: i32) -> DirectRandomDrawReceipt {
    let mut rng = don_sim::rng::Random::new(state);
    let returned = rng.get(0, 0xffff);
    DirectRandomDrawReceipt {
        call_va: PLACE_UNIT_DIRECT_RANDOM_CALL_VA,
        state_before: state,
        returned,
        state_after: rng.state(),
    }
}

#[test]
fn receipt_binds_rng_sparse_identity_and_both_channel_authorities() {
    let mut i = inputs(0);
    i.rules.bonus_citizens = -2;
    i.bonuses.extra_citizens = true;
    let plan = build_units_plan(i).unwrap();
    assert_eq!(plan.calls.len(), 1);
    let call = plan.calls[0];
    let draw = direct_draw(12345);
    let nested = InitUnitRngSpan {
        body_va: OBJECTS_INIT_UNIT_VA,
        body_bytes: OBJECTS_INIT_UNIT_BYTES,
        state_before: draw.state_after,
        state_after: -99,
    };
    let receipt = BuildUnitsPrefixReceipt {
        rng_initial: 12345,
        rng_final: -99,
        placements: vec![PlaceUnitReceipt {
            call,
            rng_before: 12345,
            rng_after: -99,
            rng_events: vec![
                PlacementRngEvent::DirectOffset(draw),
                PlacementRngEvent::InitUnit(nested),
            ],
            outcome: PlacementOutcomeReceipt::Spawned(InitUnitAuthorityReceipt {
                validated_body_va: OBJECTS_INIT_UNIT_VA,
                validated_body_bytes: OBJECTS_INIT_UNIT_BYTES,
                unit_mark_before: 0,
                unit_mark_after: 1,
                returned_captain_o: 0,
                members: vec![member(call, 100, 7, 0)],
            }),
        }],
    };
    validate_build_units_prefix_receipt(&plan, &receipt).unwrap();

    let mut bad = receipt.clone();
    if let PlacementOutcomeReceipt::Spawned(init) = &mut bad.placements[0].outcome {
        init.members[0].guys_authority_key = (999, 7);
    }
    assert_eq!(
        validate_build_units_prefix_receipt(&plan, &bad),
        Err(BuildUnitsReceiptError::InvalidMemberAuthority {
            ordinal: 0,
            member: 0
        })
    );
}

#[test]
fn receipt_rejects_duplicate_generational_identity_across_calls() {
    let plan = build_units_plan(inputs(0)).unwrap();
    let placements = plan
        .calls
        .iter()
        .copied()
        .map(|call| PlaceUnitReceipt {
            call,
            rng_before: 1,
            rng_after: 1,
            rng_events: vec![PlacementRngEvent::InitUnit(InitUnitRngSpan {
                body_va: OBJECTS_INIT_UNIT_VA,
                body_bytes: OBJECTS_INIT_UNIT_BYTES,
                state_before: 1,
                state_after: 1,
            })],
            outcome: PlacementOutcomeReceipt::Spawned(InitUnitAuthorityReceipt {
                validated_body_va: OBJECTS_INIT_UNIT_VA,
                validated_body_bytes: OBJECTS_INIT_UNIT_BYTES,
                unit_mark_before: 0,
                unit_mark_after: 1,
                returned_captain_o: 0,
                members: vec![member(call, 7, 1, 0)],
            }),
        })
        .collect();
    let receipt = BuildUnitsPrefixReceipt {
        rng_initial: 1,
        rng_final: 1,
        placements,
    };
    assert_eq!(
        validate_build_units_prefix_receipt(&plan, &receipt),
        Err(BuildUnitsReceiptError::DuplicateIdentity {
            ordinal: 1,
            member: 0
        })
    );
}
