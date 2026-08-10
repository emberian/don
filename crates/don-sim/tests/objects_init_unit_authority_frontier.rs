#[path = "../src/systems/objects_init_unit_authority_frontier.rs"]
mod frontier;

use frontier::*;

fn request() -> BhsInitUnitRequest {
    BhsInitUnitRequest {
        owner: 2,
        type_index: 150,
        x: 4_032,
        y: 2_496,
        exact_o: -1,
        external_previous: -1,
        external_next: -1,
    }
}

fn track_facts() -> TrackUnitTypeFacts {
    TrackUnitTypeFacts {
        has_attack: true,
        training_where: TrainingWhere::Barracks,
        domain: 0,
        is_peasant: true,
        is_scholar: false,
        role_has_scout_bit: false,
        is_type_0x42: false,
        is_type_0x4b: false,
        member_former_type_is_0x34: false,
    }
}

fn type_facts(uber_size: i32) -> UnitTypeAuthorityFacts {
    UnitTypeAuthorityFacts {
        uber_size,
        control_cost: 5,
        is_fighter_bomber: false,
        is_government_hero: false,
        track: track_facts(),
    }
}

fn extended(ordinal: u32, owner: i32, cursor: i32) -> FindFreeUnitReceipt {
    FindFreeUnitReceipt {
        ordinal,
        owner,
        start: 0,
        limit: 2_000,
        exact_o: -1,
        cursor_before: cursor,
        cursor_after: cursor + 1,
        returned: cursor,
        disposition: FindFreeDisposition::ConstructedAndRegistered {
            class: if owner < 8 {
                UnitBandStorageClass::Unit
            } else {
                UnitBandStorageClass::Animal
            },
            object_list_registered: true,
            unit_projection_registered: true,
        },
    }
}

fn reused(ordinal: u32, owner: i32, cursor: i32, o: i32) -> FindFreeUnitReceipt {
    FindFreeUnitReceipt {
        ordinal,
        owner,
        start: 0,
        limit: 2_000,
        exact_o: -1,
        cursor_before: cursor,
        cursor_after: cursor,
        returned: o,
        disposition: FindFreeDisposition::Reused {
            candidate: ReuseCandidateFacts {
                index: o,
                flags: 0,
                hold_frames: 0,
                is_unit: true,
                o_up: -1,
            },
            all_lower_indices_ineligible: true,
        },
    }
}

fn init(ordinal: u32, o: i32, returned: i32, unit_masks: u32) -> UnitInitReceipt {
    let request = request();
    UnitInitReceipt {
        ordinal,
        owner: request.owner,
        type_index: request.type_index,
        o,
        x: request.x,
        y: request.y,
        returned,
        extent: CompleteBody::UnitInit3732Bytes,
        after: UnitAfterInit {
            owner: request.owner,
            o,
            type_index: request.type_index,
            x: request.x,
            y: request.y,
            angle: 0x5555_5555,
            unit_masks,
        },
    }
}

fn captain(ordinal: u32, from_o: i32, captain_o: i32) -> ResolveCaptainReceipt {
    ResolveCaptainReceipt {
        ordinal,
        from_owner: request().owner,
        from_o,
        returned: captain_o,
        captain: CaptainFacts {
            owner: request().owner,
            o: captain_o,
            x: request().x,
            y: request().y,
            angle: 0x5555_5555,
            new_block_radius: 2,
        },
    }
}

fn leader_before() -> LeaderAccounting {
    LeaderAccounting {
        flags: LEADER_SPECIAL_TYPE_PRESENT_FLAG,
        num_units_for_type: 9,
        units_built: 30,
        active: 20,
        control: 100,
        peasants: 12,
        scholars: 8,
        scouts: 7,
        scholar_militia: 6,
        barracks_units: 10,
        stable_units: 11,
        factory_units: 12,
        combat_units: 13,
        dock_units: 14,
        air_units: 15,
    }
}

fn two_member_receipt(nearby_returned: i32) -> DetailedInitUnitReceipt {
    let facts = type_facts(2);
    let before = leader_before();
    let after = before.after_linked_member(facts, 0);
    let cap = captain(1, 11, 10);
    let nearby = InternalNearbyReceipt {
        request: InternalNearbyRequest::from_captain(1, request().type_index, 11, cap.captain),
        returned: nearby_returned,
        output_x: 4_128,
        output_y: 2_592,
        extent: CompleteBody::FindNearbySpot1433Bytes,
    };
    let (x, y) = if nearby_returned == 0 {
        (nearby.output_x, nearby.output_y)
    } else {
        (request().x, request().y)
    };

    DetailedInitUnitReceipt {
        request: request(),
        type_facts: facts,
        steps: vec![
            InitUnitStep::FindFree(extended(0, request().owner, 10)),
            InitUnitStep::UnitInit(init(0, 10, -77, 0)),
            InitUnitStep::SetPrevious {
                ordinal: 0,
                member_o: 10,
                previous_o: -1,
            },
            InitUnitStep::FindFree(extended(1, request().owner, 11)),
            InitUnitStep::UnitInit(init(1, 11, -88, 0)),
            InitUnitStep::SetPrevious {
                ordinal: 1,
                member_o: 11,
                previous_o: 10,
            },
            InitUnitStep::LeaderCorrection(LeaderCorrectionReceipt {
                ordinal: 1,
                owner: request().owner,
                type_index: request().type_index,
                member_o: 11,
                extent: CompleteBody::LeaderTrackUnitType326Bytes,
                before,
                after,
            }),
            InitUnitStep::ResolveCaptain(cap),
            InitUnitStep::Nearby(nearby),
            InitUnitStep::SetNewLocation(SetNewLocationReceipt {
                ordinal: 1,
                member_o: 11,
                x,
                y,
                tail: [1, 1],
                returned: -99,
                extent: CompleteBody::SetNewLocation1757Bytes,
            }),
            InitUnitStep::SetNext {
                ordinal: 1,
                previous_o: 10,
                member_o: 11,
            },
            InitUnitStep::ResolveCaptain(captain(2, 11, 10)),
        ],
        returned: 10,
    }
}

#[test]
fn native_addresses_sizes_and_layout_offsets_are_frozen() {
    assert_eq!(
        (OBJECTS_INIT_UNIT_VA, OBJECTS_INIT_UNIT_BYTES),
        (0x0065_e0c0, 1_603)
    );
    assert_eq!(
        (OBJECTS_FIND_FREE_VA, OBJECTS_FIND_FREE_BYTES),
        (0x0065_ad60, 1_101)
    );
    assert_eq!(OBJECTS_FIND_FREE_CALL_VA, 0x0065_e137);
    assert_eq!((UNIT_INIT_VA, UNIT_INIT_BYTES), (0x0061_2100, 3_732));
    assert_eq!(UNIT_INIT_VTABLE_OFFSET, 0x8c);
    assert_eq!(
        (LEADER_TRACK_UNIT_TYPE_VA, LEADER_TRACK_UNIT_TYPE_BYTES),
        (0x006e_0dd0, 326)
    );
    assert_eq!(
        (UNIT_FIND_NEARBY_SPOT_VA, UNIT_FIND_NEARBY_SPOT_BYTES),
        (0x0061_de70, 1_433)
    );
    assert_eq!(
        (UNIT_SET_NEW_LOCATION_VA, UNIT_SET_NEW_LOCATION_BYTES),
        (0x005f_8d20, 1_757)
    );
    assert_eq!(UNIT_GET_CAPTAIN_VA, 0x0061_0ab0);
    assert_eq!(UNIT_MARK_OFFSET, 0x15c);
    assert_eq!((UNIT_PREVIOUS_OFFSET, UNIT_NEXT_OFFSET), (0x8e, 0x90));
    assert_eq!(UNIT_MASKS_OFFSET, 0x68);
    assert_eq!(OBJECT_ANGLE_OFFSET, 0x50);
    assert_eq!(OBJECT_TYPE_NEW_BLOCK_RADIUS_OFFSET, 0x248);
    assert_eq!(UNIT_TYPE_CONTROL_COST_OFFSET, 0x2f0);
    assert_eq!(UNIT_TYPE_UBER_SIZE_OFFSET, 0x308);
    assert_eq!(FIGHTER_BOMBER_TYPE, 308);
    assert_eq!(GOVERNMENT_HERO_FLAG, 0x0400_0000);
}

#[test]
fn find_free_reuses_only_an_inactive_unheld_captain_without_advancing_mark() {
    let good = reused(0, 2, 50, 7);
    assert!(good.validate_bhs(0, 2, 50));

    for bad_candidate in [
        ReuseCandidateFacts {
            flags: 1,
            ..match good.disposition {
                FindFreeDisposition::Reused { candidate, .. } => candidate,
                _ => unreachable!(),
            }
        },
        ReuseCandidateFacts {
            hold_frames: 1,
            ..match good.disposition {
                FindFreeDisposition::Reused { candidate, .. } => candidate,
                _ => unreachable!(),
            }
        },
        ReuseCandidateFacts {
            o_up: 3,
            ..match good.disposition {
                FindFreeDisposition::Reused { candidate, .. } => candidate,
                _ => unreachable!(),
            }
        },
    ] {
        let mut bad = good;
        bad.disposition = FindFreeDisposition::Reused {
            candidate: bad_candidate,
            all_lower_indices_ineligible: true,
        };
        assert!(!bad.validate_bhs(0, 2, 50));
    }
}

#[test]
fn find_free_new_slot_registers_both_owner_bands_and_uses_owner_class_split() {
    assert!(extended(0, 7, 12).validate_bhs(0, 7, 12));
    assert!(extended(0, 8, 12).validate_bhs(0, 8, 12));
    assert!(FindFreeUnitReceipt {
        ordinal: 0,
        owner: 2,
        start: 0,
        limit: 2_000,
        exact_o: -1,
        cursor_before: 12,
        cursor_after: 13,
        returned: 12,
        disposition: FindFreeDisposition::ExtendedExistingStorage,
    }
    .validate_bhs(0, 2, 12));

    let mut missing_projection = extended(0, 2, 12);
    let FindFreeDisposition::ConstructedAndRegistered {
        ref mut unit_projection_registered,
        ..
    } = missing_projection.disposition
    else {
        unreachable!()
    };
    *unit_projection_registered = false;
    assert!(!missing_projection.validate_bhs(0, 2, 12));

    let mut wrong_class = extended(0, 8, 12);
    let FindFreeDisposition::ConstructedAndRegistered { ref mut class, .. } =
        wrong_class.disposition
    else {
        unreachable!()
    };
    *class = UnitBandStorageClass::Unit;
    assert!(!wrong_class.validate_bhs(0, 8, 12));
}

#[test]
fn two_member_transaction_preserves_exact_commit_order_and_ignored_returns() {
    let receipt = two_member_receipt(0);
    let effects = receipt.validate().unwrap();
    assert_eq!(effects.initialized_members, vec![10, 11]);
    assert_eq!(effects.terminal_find_free_failure, None);
    assert_eq!(effects.returned_captain_or_failure, 10);
    assert_eq!(
        (effects.unit_mark_before, effects.unit_mark_after),
        (10, 12)
    );

    let tags: Vec<&str> = receipt
        .steps
        .iter()
        .map(|step| match step {
            InitUnitStep::FindFree(_) => "find",
            InitUnitStep::UnitInit(_) => "init",
            InitUnitStep::SetPrevious { .. } => "up",
            InitUnitStep::LeaderCorrection(_) => "leader",
            InitUnitStep::ResolveCaptain(_) => "captain",
            InitUnitStep::Nearby(_) => "nearby",
            InitUnitStep::SetNewLocation(_) => "location",
            InitUnitStep::SetNext { .. } => "down",
        })
        .collect();
    assert_eq!(
        tags,
        [
            "find", "init", "up", "find", "init", "up", "leader", "captain", "nearby", "location",
            "down", "captain"
        ]
    );
}

#[test]
fn nearby_nonzero_falls_back_to_post_init_coordinates_and_does_not_abort() {
    let receipt = two_member_receipt(1);
    let location = receipt
        .steps
        .iter()
        .find_map(|step| match step {
            InitUnitStep::SetNewLocation(value) => Some(value),
            _ => None,
        })
        .unwrap();
    assert_eq!((location.x, location.y), (request().x, request().y));
    assert_eq!(receipt.validate().unwrap().returned_captain_or_failure, 10);

    let mut wrong = receipt;
    let location = wrong
        .steps
        .iter_mut()
        .find_map(|step| match step {
            InitUnitStep::SetNewLocation(value) => Some(value),
            _ => None,
        })
        .unwrap();
    location.x += 1;
    assert_eq!(wrong.validate(), Err(InitUnitReceiptError::InvalidLocation));
}

#[test]
fn leader_track_and_direct_sidecars_are_one_exact_subordinate_correction() {
    let before = leader_before();
    let after = before.after_linked_member(type_facts(2), 0);
    assert_eq!(after.num_units_for_type, 8);
    assert_eq!(after.barracks_units, 9);
    assert_eq!(after.combat_units, 12);
    assert_eq!(after.peasants, 11);
    assert_eq!(after.control, 95);
    assert_eq!(after.active, 19);
    assert_eq!(after.units_built, 29);
    assert_eq!(after.stable_units, before.stable_units);

    let immune = before.after_linked_member(type_facts(2), 1);
    assert_eq!(immune.control, before.control);

    let mut wrapping = before;
    wrapping.num_units_for_type = 0;
    let wrapped = wrapping.after_linked_member(type_facts(2), 0);
    assert_eq!(wrapped.num_units_for_type, u16::MAX);

    for (training_where, domain, selected) in [
        (TrainingWhere::Stable, 0, "stable"),
        (TrainingWhere::Factory, 0, "factory"),
        (TrainingWhere::Dock, 0, "dock"),
        (TrainingWhere::Other, 2, "air"),
        (TrainingWhere::Other, 0, "none"),
    ] {
        let mut facts = type_facts(2);
        facts.track.training_where = training_where;
        facts.track.domain = domain;
        let after = before.after_linked_member(facts, 0);
        assert_eq!(
            after.stable_units == before.stable_units - 1,
            selected == "stable"
        );
        assert_eq!(
            after.factory_units == before.factory_units - 1,
            selected == "factory"
        );
        assert_eq!(
            after.dock_units == before.dock_units - 1,
            selected == "dock"
        );
        assert_eq!(after.air_units == before.air_units - 1, selected == "air");
    }
}

#[test]
fn fighter_bomber_and_government_hero_are_the_zero_control_tracking_gates() {
    let mut facts = type_facts(2);
    facts.control_cost = 0;
    assert!(!facts.tracks_linked_member());
    facts.is_fighter_bomber = true;
    assert!(facts.tracks_linked_member());
    facts.is_fighter_bomber = false;
    facts.is_government_hero = true;
    assert!(facts.tracks_linked_member());
}

#[test]
fn capacity_failure_returns_exact_minus_one_and_retains_earlier_member() {
    let facts = type_facts(2);
    let receipt = DetailedInitUnitReceipt {
        request: request(),
        type_facts: facts,
        steps: vec![
            InitUnitStep::FindFree(reused(0, request().owner, 2_000, 10)),
            InitUnitStep::UnitInit(init(0, 10, -123, 0)),
            InitUnitStep::SetPrevious {
                ordinal: 0,
                member_o: 10,
                previous_o: -1,
            },
            InitUnitStep::FindFree(FindFreeUnitReceipt {
                ordinal: 1,
                owner: request().owner,
                start: 0,
                limit: 2_000,
                exact_o: -1,
                cursor_before: 2_000,
                cursor_after: 2_000,
                returned: -1,
                disposition: FindFreeDisposition::CapacityFailure {
                    all_existing_indices_ineligible: true,
                },
            }),
        ],
        returned: -1,
    };
    let effects = receipt.validate().unwrap();
    assert_eq!(effects.initialized_members, vec![10]);
    assert_eq!(effects.terminal_find_free_failure, Some(-1));
    assert_eq!(effects.returned_captain_or_failure, -1);
    assert_eq!(
        (effects.unit_mark_before, effects.unit_mark_after),
        (2_000, 2_000)
    );

    let mut invented_failure = receipt;
    invented_failure.returned = -9;
    assert_eq!(
        invented_failure.validate(),
        Err(InitUnitReceiptError::InvalidReturn)
    );
}

#[test]
fn previous_next_link_cannot_be_published_before_location() {
    let mut receipt = two_member_receipt(0);
    receipt.steps.swap(9, 10);
    assert!(matches!(
        receipt.validate(),
        Err(InitUnitReceiptError::WrongStep | InitUnitReceiptError::InvalidLocation)
    ));
}

#[test]
fn final_public_result_is_a_second_captain_resolution_from_the_last_member() {
    let mut receipt = two_member_receipt(0);
    receipt.returned = 11;
    assert_eq!(receipt.validate(), Err(InitUnitReceiptError::InvalidReturn));

    let mut receipt = two_member_receipt(0);
    let InitUnitStep::ResolveCaptain(final_resolve) = receipt.steps.last_mut().unwrap() else {
        unreachable!()
    };
    final_resolve.ordinal = 1;
    assert_eq!(
        receipt.validate(),
        Err(InitUnitReceiptError::InvalidCaptain)
    );
}
