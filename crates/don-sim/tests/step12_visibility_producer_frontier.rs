#[path = "../src/systems/step12_visibility_producer_frontier.rs"]
mod step12_visibility_producer_frontier;

use step12_visibility_producer_frontier::*;

fn unit() -> Step12UnitFacts {
    Step12UnitFacts {
        leader_active: true,
        object_flags: OBJECT_VALID,
        inside_up: -1,
        who: 2,
        object_o: 17,
        fine_x: 12 * FINE_PER_FOG_CELL,
        fine_y: 9 * FINE_PER_FOG_CELL,
        resolved_los_tiles: 10,
        unit_domain: 0,
        type_unit_flags2: 0,
        unit_masks: 0,
        projected_small_los_center: None,
        grant_seen2_to: 0,
    }
}

fn los() -> UnitLosFacts {
    UnitLosFacts {
        mylos: 9,
        ptolemy_count: 0,
        unit_role: 0,
        has_ptolemy_general: None,
        ptolemy_los_bonus: 4,
        the_ceo_count: 0,
        unit_is_siege: None,
        has_the_ceo_general: None,
        the_ceo_unit_los: 6,
    }
}

fn live(row: usize, who: u8, object_o: i16) -> LiveStep12UnitState {
    LiveStep12UnitState {
        identity: LiveStep12UnitIdentity {
            row,
            who,
            object_o,
            uid: 1000 + row as u16,
            type_index: 50 + row as i32,
        },
        object_flags: OBJECT_VALID,
        inside_up: -1,
        fine_x: (10 + row as i32) * FINE_PER_FOG_CELL,
        fine_y: (20 + row as i32) * FINE_PER_FOG_CELL,
        unit_angle: 0x2000_0000,
        mylos: 10,
        visible: 0,
        unit_masks: 0,
        infiltrated: 0,
    }
}

fn authority(live: LiveStep12UnitState) -> Step12UnitAuthorityReceipt {
    let mut los_facts = los();
    los_facts.mylos = live.mylos;
    Step12UnitAuthorityReceipt {
        identity: live.identity,
        detector: Some(DetectorInstanceProvenance::ObjectInit {
            object_masks_at_init: 0,
        }),
        los_facts,
        unit_domain: None,
        type_unit_flags2: None,
        small_los_projection: None,
        local_seen_radius: None,
    }
}

fn snapshot<'a>(
    rows: &'a [LiveStep12UnitState],
    owner_band_lengths: [usize; LEADER_SLOTS],
) -> LiveStep12UnitBandSnapshot<'a> {
    LiveStep12UnitBandSnapshot {
        frame: 33,
        state_revision: 70,
        type_revision: 9,
        leader_active: std::array::from_fn(|who| owner_band_lengths[who] != 0),
        owner_band_lengths,
        rows,
    }
}

fn authority_batch(
    snapshot: LiveStep12UnitBandSnapshot<'_>,
    rows: Vec<Option<Step12UnitAuthorityReceipt>>,
) -> Step12VisibilityAuthorityReceipt {
    Step12VisibilityAuthorityReceipt {
        frame: snapshot.frame,
        state_revision: snapshot.state_revision,
        type_revision: snapshot.type_revision,
        leader_active: snapshot.leader_active,
        owner_band_lengths: snapshot.owner_band_lengths,
        rows,
    }
}

fn stamp(facts: Step12UnitFacts) -> Step12UnitStamp {
    let UnitStampDecision::Stamp(stamp) = plan_unit_stamp(facts).unwrap() else {
        panic!("expected admitted Unit stamp")
    };
    stamp
}

#[test]
fn shipped_entries_bits_and_checksum_owner_are_frozen() {
    assert_eq!(GAME_DAEMON_PROCESS_ALL_VA, 0x0073_2700);
    assert_eq!(GAME_DAEMON_UPDATE_ALL_SEEN_VA, 0x0073_2840);
    assert_eq!(OBJECT_INIT_VA, 0x0064_7750);
    assert_eq!(OBJECT_UPDATE_SEEN_VA, 0x0065_1b80);
    assert_eq!(UNIT_UPDATE_LOCAL_SEEN_VA, 0x0060_e410);
    assert_eq!(WALL_UPDATE_LOCAL_SEEN_VA, 0x0063_ed50);
    assert_eq!(WORLD_SET_LOCALLY_SEEN_VA, 0x006b_4bb0);
    assert_eq!(OBJECT_HAS_GENERAL_VA, 0x0064_6b00);
    assert_eq!(UNIT_DATA_LOS_VA, 0x0061_00c0);
    assert_eq!(UNIT_DATA_IS_ON_MAP_VA, 0x0046_ce30);
    assert_eq!(UNIT_DATA_IS_VALID_UNIT_VA, 0x0046_cda0);
    assert_eq!(WORLD_CLEAR_SEEN_VA, 0x006b_2250);
    assert_eq!(WORLD_SET_SEEN_VA, 0x006b_3c60);
    assert_eq!(PROJECT_VA, 0x0092_cf40);
    assert_eq!(FRAME_ZERO_EXPLORED_SHARING_BEGIN_VA, 0x0073_2bd8);
    assert_eq!(FRAME_ZERO_EXPLORED_SHARING_END_VA, 0x0073_2cf3);
    assert_eq!(
        UPDATE_ALL_SEEN_DIRECT_CALL_SITES,
        [
            0x0058_525d,
            0x0062_95f7,
            0x009f_c78d,
            0x009f_c82d,
            0x00a0_33f5
        ]
    );
    assert_eq!(OBJECT_VALID, 1);
    assert_eq!(OBJECT_DETECTOR, 0x40);
    assert_eq!(OBJMASK_DETECT, 0x0200_0000);
    assert_eq!((PTOLEMY_TYPE_INDEX, THE_CEO_TYPE_INDEX), (0x16a, 0x165));
    assert_eq!(
        (PTOLEMY_NUM_UNITS_INDEX, THE_CEO_NUM_UNITS_INDEX),
        (0x138, 0x133)
    );
    assert_eq!(PTOLEMY_ROLE_MASK, 0x420);
    assert_eq!(SMALL_LOS_STANDARD_TYPE_FLAGS2, 4);
    assert_eq!(SMALL_LOS_STANDARD_UNIT_MASKS, 1);
    assert_eq!(UNIT_ANGLE_OFFSET, 0x50);
    assert_eq!(SMALL_LOS_PROJECT_DISTANCE, FINE_PER_FOG_CELL);
    assert_eq!(CONSTANTS_PTOLEMY_LOS_BONUS_OFFSET, 0xb64);
    assert_eq!(CONSTANTS_THE_CEO_UNIT_LOS_OFFSET, 0xca8);
    assert_eq!((WORLD_CHECKSUM_CHANNEL, WORLD_FOG_WALK_SECTION), (12, 6));
}

#[test]
fn cadence_is_signed_frame_modulo_100_phase_33() {
    assert_eq!(
        visibility_cadence(32, 0),
        Step12VisibilityCadence::NotScheduled
    );
    assert_eq!(
        visibility_cadence(33, 0),
        Step12VisibilityCadence::FullRefresh
    );
    assert_eq!(
        visibility_cadence(133, 0),
        Step12VisibilityCadence::FullRefresh
    );
    assert_eq!(
        visibility_cadence(-67, 0),
        Step12VisibilityCadence::NotScheduled,
        "x86 signed remainder keeps the dividend's sign"
    );
}

#[test]
fn direct_entries_bypass_phase_but_not_the_producer_fog_option_return() {
    let direct = [
        Step12VisibilityTrigger::GameRun,
        Step12VisibilityTrigger::BuildClose,
        Step12VisibilityTrigger::ScenarioAddVisibility,
        Step12VisibilityTrigger::ScenarioRemoveVisibility,
        Step12VisibilityTrigger::ScenarioSetExploredShowBuildings,
    ];
    for trigger in direct {
        assert_eq!(
            visibility_entry(7, 0, trigger),
            Step12VisibilityCadence::FullRefresh
        );
        assert_eq!(
            visibility_entry(7, 3, trigger),
            Step12VisibilityCadence::SuppressedByFogOptionThree
        );
    }
    assert_eq!(
        visibility_entry(7, 0, Step12VisibilityTrigger::ScheduledStep12),
        Step12VisibilityCadence::NotScheduled
    );
}

#[test]
fn fog_option_three_returns_before_busy_or_plane_mutation() {
    assert_eq!(
        visibility_cadence(33, 3),
        Step12VisibilityCadence::SuppressedByFogOptionThree
    );
    assert_eq!(
        visibility_cadence(33, 2),
        Step12VisibilityCadence::FullRefresh
    );
    assert_eq!(
        visibility_cadence(33, 4),
        Step12VisibilityCadence::FullRefresh
    );
    assert_eq!(FULL_REFRESH_STAGES[0], FullRefreshStage::SetDaemonBusyFour);
    assert_eq!(UPDATE_ALL_SEEN_BUSY_VALUE, 4);
}

#[test]
fn refresh_stage_order_clears_detection_before_any_object_stamp() {
    assert_eq!(
        FULL_REFRESH_STAGES,
        [
            FullRefreshStage::SetDaemonBusyFour,
            FullRefreshStage::ClearSeenAndWcoordSeen,
            FullRefreshStage::ClearDetectedSeen3,
            FullRefreshStage::WalkBuildAndWallBands,
            FullRefreshStage::WalkUnitBands,
            FullRefreshStage::ConsiderScenarioRevealPoints,
            FullRefreshStage::ConsiderFrameZeroExploredSharing,
        ]
    );
}

#[test]
fn optional_global_visibility_stages_have_exact_gates_and_never_add_detection() {
    assert!(!scenario_reveal_points_enabled(0, 0));
    assert!(scenario_reveal_points_enabled(0x10, 0));
    assert!(scenario_reveal_points_enabled(0, 0x02));
    assert!(!scenario_reveal_points_enabled(0x08, 0x01));

    assert!(frame_zero_explored_sharing(0));
    assert!(!frame_zero_explored_sharing(33));
    assert!(!frame_zero_explored_sharing(133));
}

#[test]
fn object_init_materializes_detector_from_exact_type_mask() {
    assert_eq!(object_init_flags(0), OBJECT_VALID);
    assert_eq!(
        object_init_flags(OBJMASK_DETECT),
        OBJECT_VALID | OBJECT_DETECTOR
    );
    assert_eq!(
        object_init_flags(OBJMASK_DETECT | 0x8000_0000),
        OBJECT_VALID | OBJECT_DETECTOR
    );
}

#[test]
fn step12_reads_the_instance_detector_bit_without_rederiving_type() {
    let plain = stamp(unit());
    assert!(!plain.detector);

    let mut detector = unit();
    detector.object_flags = object_init_flags(OBJMASK_DETECT);
    assert!(stamp(detector).detector);

    detector.object_flags &= !OBJECT_DETECTOR;
    assert!(
        !stamp(detector).detector,
        "a later pass must not resurrect the bit from stale type facts"
    );
}

#[test]
fn unit_los_uses_signed_mylos_when_optimized_counts_are_zero() {
    assert_eq!(resolve_unit_los(los()), Ok(9));
}

#[test]
fn ptolemy_los_requires_count_role_and_in_radius_general() {
    let mut facts = los();
    facts.ptolemy_count = 1;
    facts.has_ptolemy_general = Some(true);
    assert_eq!(
        resolve_unit_los(facts),
        Ok(9),
        "role gate precedes the query"
    );

    facts.unit_role = PTOLEMY_ROLE_MASK;
    facts.has_ptolemy_general = Some(false);
    assert_eq!(resolve_unit_los(facts), Ok(9));
    facts.has_ptolemy_general = Some(true);
    assert_eq!(resolve_unit_los(facts), Ok(13));
}

#[test]
fn the_ceo_los_requires_count_non_siege_and_in_radius_general() {
    let mut facts = los();
    facts.the_ceo_count = 1;
    facts.unit_is_siege = Some(true);
    assert_eq!(resolve_unit_los(facts), Ok(9));

    facts.unit_is_siege = Some(false);
    facts.has_the_ceo_general = Some(false);
    assert_eq!(resolve_unit_los(facts), Ok(9));
    facts.has_the_ceo_general = Some(true);
    assert_eq!(resolve_unit_los(facts), Ok(15));
}

#[test]
fn reached_unit_los_facts_fail_closed_in_retail_query_order() {
    let mut facts = los();
    facts.ptolemy_count = 1;
    facts.unit_role = PTOLEMY_ROLE_MASK;
    assert_eq!(
        resolve_unit_los(facts),
        Err(UnitLosFault::MissingPtolemyGeneral)
    );

    facts = los();
    facts.the_ceo_count = 1;
    assert_eq!(
        resolve_unit_los(facts),
        Err(UnitLosFault::MissingTheCeoSiegeClass)
    );
    facts.unit_is_siege = Some(false);
    assert_eq!(
        resolve_unit_los(facts),
        Err(UnitLosFault::MissingTheCeoGeneral)
    );
}

#[test]
fn unit_band_admission_matches_leader_valid_and_inside_up_gates() {
    let mut facts = unit();
    facts.leader_active = false;
    assert_eq!(
        plan_unit_stamp(facts),
        Ok(UnitStampDecision::Skip(UnitStampSkip::InactiveLeader))
    );

    facts = unit();
    facts.object_flags = 0;
    assert_eq!(
        plan_unit_stamp(facts),
        Ok(UnitStampDecision::Skip(UnitStampSkip::InvalidUnit))
    );

    facts = unit();
    facts.inside_up = 0;
    assert_eq!(
        plan_unit_stamp(facts),
        Ok(UnitStampDecision::Skip(UnitStampSkip::ContainedOrLaunched))
    );
    facts.inside_up = i16::MIN;
    assert!(matches!(
        plan_unit_stamp(facts),
        Ok(UnitStampDecision::Stamp(_))
    ));
}

#[test]
fn zero_los_skips_but_one_tile_stamps_the_center_cell() {
    let mut facts = unit();
    facts.resolved_los_tiles = 0;
    assert_eq!(
        plan_unit_stamp(facts),
        Ok(UnitStampDecision::Skip(UnitStampSkip::ZeroLos))
    );
    facts.resolved_los_tiles = 1;
    facts.projected_small_los_center = Some((facts.fine_x + 384, facts.fine_y));
    let stamp = stamp(facts);
    assert_eq!(stamp.radius_fog_cells, 0);
    assert_eq!(stamp.center, UnitStampCenter::ProjectedSmallLandUnit);
    assert_eq!(stamp.fog_x, fine_to_fog(facts.fine_x + 384));
}

#[test]
fn reached_small_land_projection_is_mandatory_and_standard_bits_bypass_it() {
    let mut facts = unit();
    facts.resolved_los_tiles = 6;
    assert_eq!(
        plan_unit_stamp(facts),
        Err(UnitStampFault::MissingSmallLosProjectedCenter)
    );

    facts.projected_small_los_center = Some((facts.fine_x + 384, facts.fine_y - 384));
    let projected = stamp(facts);
    assert_eq!(projected.radius_fog_cells, 3);
    assert_eq!(projected.center, UnitStampCenter::ProjectedSmallLandUnit);
    assert_eq!(
        (projected.stamp_fine_x, projected.stamp_fine_y),
        (facts.fine_x + 384, facts.fine_y - 384)
    );

    facts.projected_small_los_center = None;
    facts.type_unit_flags2 = SMALL_LOS_STANDARD_TYPE_FLAGS2;
    let type_forced = stamp(facts);
    assert_eq!(type_forced.center, UnitStampCenter::ObjectPosition);
    assert_eq!(
        (type_forced.stamp_fine_x, type_forced.stamp_fine_y),
        (facts.fine_x, facts.fine_y)
    );

    facts.type_unit_flags2 = 0;
    facts.unit_masks = SMALL_LOS_STANDARD_UNIT_MASKS;
    assert_eq!(stamp(facts).center, UnitStampCenter::ObjectPosition);
    facts.unit_masks = 0;
    facts.unit_domain = 1;
    assert_eq!(stamp(facts).center, UnitStampCenter::ObjectPosition);
}

#[test]
fn resolved_los_converts_by_192_over_384_and_clamps_at_64() {
    let mut facts = unit();
    facts.resolved_los_tiles = 10;
    assert_eq!(stamp(facts).radius_fog_cells, 5);
    facts.resolved_los_tiles = 128;
    assert_eq!(stamp(facts).radius_fog_cells, 64);
    facts.resolved_los_tiles = 500;
    assert_eq!(stamp(facts).radius_fog_cells, 64);
}

#[test]
fn negative_or_wrapped_los_fails_instead_of_defaulting_to_zero() {
    let mut facts = unit();
    facts.resolved_los_tiles = -1;
    assert_eq!(
        plan_unit_stamp(facts),
        Err(UnitStampFault::NegativeResolvedLos(-1))
    );
    facts.resolved_los_tiles = i32::MAX;
    assert!(matches!(
        plan_unit_stamp(facts),
        Err(UnitStampFault::CorruptWrappedRadius { .. })
    ));
}

#[test]
fn owner_and_object_identity_fail_before_any_stamp() {
    let mut facts = unit();
    facts.who = 8;
    assert_eq!(plan_unit_stamp(facts), Err(UnitStampFault::InvalidOwner(8)));
    facts = unit();
    facts.object_o = -1;
    assert_eq!(
        plan_unit_stamp(facts),
        Err(UnitStampFault::InvalidObjectSlot(-1))
    );
}

#[test]
fn div3_table_floor_semantics_cover_negative_off_map_coordinates() {
    assert_eq!(div3_floor(2), 0);
    assert_eq!(div3_floor(3), 1);
    assert_eq!(div3_floor(-1), -1);
    assert_eq!(div3_floor(-3), -1);
    assert_eq!(div3_floor(-4), -2);
    assert_eq!(fine_to_fog(-1), -1);
    assert_eq!(fine_to_fog(12 * FINE_PER_FOG_CELL), 12);
}

#[test]
fn external_cell_sample_reads_current_seen_and_seen3_from_one_exact_cell() {
    let mut seen = vec![0; 8 * 6];
    let mut seen3 = vec![0; 8 * 6];
    let index = 3 * 8 + 5;
    seen[index] = 0b0011_0101;
    seen3[index] = 0b1000_0010;

    let sample = sample_visibility_cell(
        5 * FINE_PER_FOG_CELL,
        3 * FINE_PER_FOG_CELL,
        8,
        6,
        &seen,
        &seen3,
    )
    .unwrap();
    assert_eq!(
        sample,
        VisibilityCellSample {
            fog_x: 5,
            fog_y: 3,
            index,
            cell_seen_mask: 0b0011_0101,
            cell_detected_mask: 0b1000_0010,
        }
    );
}

#[test]
fn external_cell_sample_rejects_stale_shape_and_off_map_rows() {
    assert_eq!(
        sample_visibility_cell(0, 0, 2, 2, &[0; 3], &[0; 4]),
        Err(VisibilityCellSampleFault::PlaneCardinality {
            expected: 4,
            seen: 3,
            seen3: 4,
        })
    );
    assert_eq!(
        sample_visibility_cell(-1, 0, 2, 2, &[0; 4], &[0; 4]),
        Err(VisibilityCellSampleFault::OffMap {
            fog_x: -1,
            fog_y: 0,
        })
    );
}

#[test]
fn live_no_mutation_entries_do_not_read_or_validate_authority() {
    let rows = [live(0, 9, -1)];
    let malformed = LiveStep12UnitBandSnapshot {
        frame: 7,
        state_revision: 1,
        type_revision: 1,
        leader_active: [false; LEADER_SLOTS],
        owner_band_lengths: [0; LEADER_SLOTS],
        rows: &rows,
    };
    assert_eq!(
        prepare_live_unit_pass(0, Step12VisibilityTrigger::ScheduledStep12, malformed, None,),
        Ok(LiveStep12Preparation::NoMutation(
            Step12VisibilityCadence::NotScheduled
        ))
    );

    let option_three = LiveStep12UnitBandSnapshot {
        frame: 33,
        ..malformed
    };
    assert_eq!(
        prepare_live_unit_pass(
            3,
            Step12VisibilityTrigger::ScheduledStep12,
            option_three,
            None,
        ),
        Ok(LiveStep12Preparation::NoMutation(
            Step12VisibilityCadence::SuppressedByFogOptionThree
        ))
    );
}

#[test]
fn live_rows_stopped_by_retail_admission_need_no_authority() {
    let mut rows = [live(1, 0, 1), live(2, 0, 2)];
    rows[0].object_flags = 0;
    rows[1].inside_up = 0;
    let snapshot = snapshot(&rows, [2, 0, 0, 0, 0, 0, 0, 0]);
    let LiveStep12Preparation::UnitPass(pass) =
        prepare_live_unit_pass(0, Step12VisibilityTrigger::ScheduledStep12, snapshot, None)
            .unwrap()
    else {
        panic!("expected preflighted Unit subpass")
    };
    assert!(!pass.authority_bound());
    assert_eq!(pass.stamps(), 0);
    assert_eq!(pass.rows().len(), 2);
    assert_eq!(
        pass.rows()[0].decision(),
        UnitStampDecision::Skip(UnitStampSkip::InvalidUnit)
    );
    assert_eq!(
        pass.rows()[1].decision(),
        UnitStampDecision::Skip(UnitStampSkip::ContainedOrLaunched)
    );
}

#[test]
fn reached_live_row_requires_a_bound_authority_receipt() {
    let rows = [live(4, 0, 0)];
    let snapshot = snapshot(&rows, [1, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        prepare_live_unit_pass(0, Step12VisibilityTrigger::ScheduledStep12, snapshot, None,),
        Err(LiveStep12PrepareFault::MissingAuthorityReceipt)
    );
}

#[test]
fn bound_live_rows_prepare_exact_plain_and_detector_stamps() {
    let mut rows = [live(4, 0, 0), live(9, 1, 3)];
    rows[1].object_flags |= OBJECT_DETECTOR;
    let snapshot = snapshot(&rows, [1, 1, 0, 0, 0, 0, 0, 0]);
    let mut second = authority(rows[1]);
    second.detector = Some(DetectorInstanceProvenance::ObjectInit {
        object_masks_at_init: OBJMASK_DETECT,
    });
    let receipt = authority_batch(snapshot, vec![Some(authority(rows[0])), Some(second)]);
    let LiveStep12Preparation::UnitPass(pass) = prepare_live_unit_pass(
        0,
        Step12VisibilityTrigger::ScheduledStep12,
        snapshot,
        Some(&receipt),
    )
    .unwrap() else {
        panic!("expected preflighted Unit subpass")
    };
    assert!(pass.authority_bound());
    assert_eq!(
        (pass.frame(), pass.state_revision(), pass.type_revision()),
        (33, 70, 9)
    );
    assert_eq!(pass.stamps(), 2);
    let UnitStampDecision::Stamp(plain) = pass.rows()[0].decision() else {
        panic!("expected first stamp")
    };
    let UnitStampDecision::Stamp(detector) = pass.rows()[1].decision() else {
        panic!("expected second stamp")
    };
    assert!(!plain.detector);
    assert!(detector.detector);
    assert_eq!(pass.rows()[1].identity(), rows[1].identity);
}

#[test]
fn detector_false_and_true_require_matching_explicit_provenance() {
    let rows = [live(0, 0, 0)];
    let snapshot = snapshot(&rows, [1, 0, 0, 0, 0, 0, 0, 0]);
    let mut row_authority = authority(rows[0]);
    row_authority.detector = None;
    let receipt = authority_batch(snapshot, vec![Some(row_authority)]);
    assert_eq!(
        prepare_live_unit_pass(
            0,
            Step12VisibilityTrigger::ScheduledStep12,
            snapshot,
            Some(&receipt),
        ),
        Err(LiveStep12PrepareFault::MissingDetectorProvenance { row: 0 })
    );

    row_authority.detector = Some(DetectorInstanceProvenance::ObjectInit {
        object_masks_at_init: OBJMASK_DETECT,
    });
    let receipt = authority_batch(snapshot, vec![Some(row_authority)]);
    assert!(matches!(
        prepare_live_unit_pass(
            0,
            Step12VisibilityTrigger::ScheduledStep12,
            snapshot,
            Some(&receipt),
        ),
        Err(LiveStep12PrepareFault::DetectorProvenance {
            fault: DetectorProvenanceFault::ObjectInitDetectorMismatch { .. },
            ..
        })
    ));
}

#[test]
fn local_seen_radius_is_lazy_exact_and_bound_only_after_positive_los_and_visible() {
    let mut rows = [live(0, 0, 0)];
    rows[0].visible = 0b0000_0100;
    let live_snapshot = snapshot(&rows, [1, 0, 0, 0, 0, 0, 0, 0]);
    let mut row_authority = authority(rows[0]);
    let receipt = authority_batch(live_snapshot, vec![Some(row_authority)]);
    assert_eq!(
        prepare_live_unit_pass(
            0,
            Step12VisibilityTrigger::ScheduledStep12,
            live_snapshot,
            Some(&receipt),
        ),
        Err(LiveStep12PrepareFault::MissingLocalSeenRadius { row: 0 })
    );

    row_authority.local_seen_radius = Some(MAX_FOG_RADIUS + 1);
    let receipt = authority_batch(live_snapshot, vec![Some(row_authority)]);
    assert_eq!(
        prepare_live_unit_pass(
            0,
            Step12VisibilityTrigger::ScheduledStep12,
            live_snapshot,
            Some(&receipt),
        ),
        Err(LiveStep12PrepareFault::InvalidLocalSeenRadius {
            row: 0,
            radius: MAX_FOG_RADIUS + 1,
        })
    );

    row_authority.local_seen_radius = Some(7);
    let receipt = authority_batch(live_snapshot, vec![Some(row_authority)]);
    let LiveStep12Preparation::UnitPass(pass) = prepare_live_unit_pass(
        0,
        Step12VisibilityTrigger::ScheduledStep12,
        live_snapshot,
        Some(&receipt),
    )
    .unwrap() else {
        panic!("expected preflighted Unit subpass")
    };
    assert_eq!(pass.rows()[0].local_seen_radius(), Some(7));

    rows[0].mylos = 0;
    let snapshot = snapshot(&rows, [1, 0, 0, 0, 0, 0, 0, 0]);
    row_authority = authority(rows[0]);
    let receipt = authority_batch(snapshot, vec![Some(row_authority)]);
    let LiveStep12Preparation::UnitPass(pass) = prepare_live_unit_pass(
        0,
        Step12VisibilityTrigger::ScheduledStep12,
        snapshot,
        Some(&receipt),
    )
    .unwrap() else {
        panic!("zero LOS must return before local-seen type authority")
    };
    assert_eq!(pass.rows()[0].local_seen_radius(), None);
}

#[test]
fn zero_los_does_not_read_type_projection_or_detector_provenance() {
    let mut rows = [live(0, 0, 0)];
    rows[0].mylos = 0;
    let snapshot = snapshot(&rows, [1, 0, 0, 0, 0, 0, 0, 0]);
    let mut row_authority = authority(rows[0]);
    row_authority.detector = None;
    let receipt = authority_batch(snapshot, vec![Some(row_authority)]);
    let LiveStep12Preparation::UnitPass(pass) = prepare_live_unit_pass(
        0,
        Step12VisibilityTrigger::ScheduledStep12,
        snapshot,
        Some(&receipt),
    )
    .unwrap() else {
        panic!("expected preflighted Unit subpass")
    };
    assert_eq!(pass.stamps(), 0);
    assert_eq!(
        pass.rows()[0].decision(),
        UnitStampDecision::Skip(UnitStampSkip::ZeroLos)
    );
}

#[test]
fn reached_small_los_type_and_projection_facts_are_lazy_and_exact() {
    let mut rows = [live(0, 0, 0)];
    rows[0].mylos = 6;
    let snapshot = snapshot(&rows, [1, 0, 0, 0, 0, 0, 0, 0]);
    let mut row_authority = authority(rows[0]);
    let receipt = authority_batch(snapshot, vec![Some(row_authority)]);
    assert_eq!(
        prepare_live_unit_pass(
            0,
            Step12VisibilityTrigger::ScheduledStep12,
            snapshot,
            Some(&receipt),
        ),
        Err(LiveStep12PrepareFault::MissingSmallLosDomain { row: 0 })
    );

    row_authority.unit_domain = Some(0);
    let receipt = authority_batch(snapshot, vec![Some(row_authority)]);
    assert_eq!(
        prepare_live_unit_pass(
            0,
            Step12VisibilityTrigger::ScheduledStep12,
            snapshot,
            Some(&receipt),
        ),
        Err(LiveStep12PrepareFault::MissingSmallLosTypeFlags2 { row: 0 })
    );

    row_authority.type_unit_flags2 = Some(0);
    let receipt = authority_batch(snapshot, vec![Some(row_authority)]);
    assert_eq!(
        prepare_live_unit_pass(
            0,
            Step12VisibilityTrigger::ScheduledStep12,
            snapshot,
            Some(&receipt),
        ),
        Err(LiveStep12PrepareFault::Stamp {
            row: 0,
            fault: UnitStampFault::MissingSmallLosProjectedCenter,
        })
    );

    row_authority.small_los_projection = Some(SmallLosProjectionReceipt {
        source_fine_x: rows[0].fine_x,
        source_fine_y: rows[0].fine_y,
        unit_angle: rows[0].unit_angle,
        distance: SMALL_LOS_PROJECT_DISTANCE,
        projected_fine_x: rows[0].fine_x + FINE_PER_FOG_CELL,
        projected_fine_y: rows[0].fine_y,
    });
    let receipt = authority_batch(snapshot, vec![Some(row_authority)]);
    let LiveStep12Preparation::UnitPass(pass) = prepare_live_unit_pass(
        0,
        Step12VisibilityTrigger::ScheduledStep12,
        snapshot,
        Some(&receipt),
    )
    .unwrap() else {
        panic!("expected preflighted Unit subpass")
    };
    let UnitStampDecision::Stamp(stamp) = pass.rows()[0].decision() else {
        panic!("expected projected stamp")
    };
    assert_eq!(stamp.center, UnitStampCenter::ProjectedSmallLandUnit);
    assert_eq!(stamp.stamp_fine_x, rows[0].fine_x + FINE_PER_FOG_CELL);

    let mut stale = row_authority;
    stale.small_los_projection.as_mut().unwrap().unit_angle ^= 1;
    let receipt = authority_batch(snapshot, vec![Some(stale)]);
    assert!(matches!(
        prepare_live_unit_pass(
            0,
            Step12VisibilityTrigger::ScheduledStep12,
            snapshot,
            Some(&receipt),
        ),
        Err(LiveStep12PrepareFault::Projection {
            fault: SmallLosProjectionFault::AngleMismatch { .. },
            ..
        })
    ));
}

#[test]
fn reached_general_facts_and_live_mylos_cannot_be_omitted_or_stale() {
    let rows = [live(0, 0, 0)];
    let snapshot = snapshot(&rows, [1, 0, 0, 0, 0, 0, 0, 0]);
    let mut row_authority = authority(rows[0]);
    row_authority.los_facts.ptolemy_count = 1;
    row_authority.los_facts.unit_role = PTOLEMY_ROLE_MASK;
    row_authority.los_facts.has_ptolemy_general = None;
    let receipt = authority_batch(snapshot, vec![Some(row_authority)]);
    assert_eq!(
        prepare_live_unit_pass(
            0,
            Step12VisibilityTrigger::ScheduledStep12,
            snapshot,
            Some(&receipt),
        ),
        Err(LiveStep12PrepareFault::Los {
            row: 0,
            fault: UnitLosFault::MissingPtolemyGeneral,
        })
    );

    row_authority = authority(rows[0]);
    row_authority.los_facts.mylos += 1;
    let receipt = authority_batch(snapshot, vec![Some(row_authority)]);
    assert_eq!(
        prepare_live_unit_pass(
            0,
            Step12VisibilityTrigger::ScheduledStep12,
            snapshot,
            Some(&receipt),
        ),
        Err(LiveStep12PrepareFault::AuthorityMylosMismatch {
            row: 0,
            snapshot: rows[0].mylos,
            receipt: rows[0].mylos + 1,
        })
    );
}

#[test]
fn live_batch_rejects_partial_reordered_or_revision_mismatched_views() {
    let rows = [live(0, 0, 1), live(1, 0, 0)];
    let reordered_snapshot = snapshot(&rows, [2, 0, 0, 0, 0, 0, 0, 0]);
    assert!(matches!(
        prepare_live_unit_pass(
            0,
            Step12VisibilityTrigger::ScheduledStep12,
            reordered_snapshot,
            None,
        ),
        Err(LiveStep12PrepareFault::OutOfRetailOrder { .. })
    ));

    let ordered = [live(0, 0, 0)];
    let snapshot = snapshot(&ordered, [1, 0, 0, 0, 0, 0, 0, 0]);
    let mut receipt = authority_batch(snapshot, vec![Some(authority(ordered[0]))]);
    receipt.state_revision += 1;
    assert_eq!(
        prepare_live_unit_pass(
            0,
            Step12VisibilityTrigger::ScheduledStep12,
            snapshot,
            Some(&receipt),
        ),
        Err(LiveStep12PrepareFault::AuthorityStateRevisionMismatch {
            snapshot: 70,
            receipt: 71,
        })
    );

    let partial = LiveStep12UnitBandSnapshot {
        owner_band_lengths: [2, 0, 0, 0, 0, 0, 0, 0],
        ..snapshot
    };
    assert_eq!(
        prepare_live_unit_pass(0, Step12VisibilityTrigger::ScheduledStep12, partial, None,),
        Err(LiveStep12PrepareFault::UnitBandCardinality {
            declared: 2,
            rows: 1,
        })
    );
}
