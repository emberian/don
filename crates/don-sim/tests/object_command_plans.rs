#[path = "../src/systems/object_command_plans.rs"]
mod object_command_plans;

use object_command_plans::*;

fn command(who: i32, object: i32) -> RenameCityCommand {
    let mut name = [0; RENAME_CITY_NAME_UNITS];
    for (index, unit) in name.iter_mut().enumerate() {
        *unit = 0x4100 + index as u16;
    }
    RenameCityCommand { who, object, name }
}

fn object_receipt(
    command: &RenameCityCommand,
    flags: u8,
    city_type: Option<i16>,
) -> RenameCityObjectLookupReceipt {
    RenameCityObjectLookupReceipt {
        request: RenameCityObjectLookupRequest {
            who: command.who,
            object: command.object,
        },
        object_flags: flags,
        city_type,
    }
}

fn normalized(slot: i32, who: u8, members: &[i16]) -> NormalizeSelectionReceipt {
    NormalizeSelectionReceipt {
        request: NormalizeSelectionRequest {
            slot,
            normalize_flag: 1,
        },
        post: NormalizedSelectionView {
            who,
            members: members.to_vec(),
        },
    }
}

#[test]
fn rename_city_decoder_pins_all_fixed_offsets_and_raw_utf16_units() {
    let mut wire = [0u8; RENAME_CITY_WIRE_BYTES];
    wire[0] = RENAME_CITY_OPCODE;
    wire[1..5].copy_from_slice(&(-7i32).to_le_bytes());
    wire[5..9].copy_from_slice(&0x1234_5678i32.to_le_bytes());
    for index in 0..RENAME_CITY_NAME_UNITS {
        let unit = if index == 8 { 0 } else { 0xd700 + index as u16 };
        wire[9 + index * 2..11 + index * 2].copy_from_slice(&unit.to_le_bytes());
    }

    let decoded = decode_rename_city(&wire).unwrap();
    assert_eq!(decoded.who, -7);
    assert_eq!(decoded.object, 0x1234_5678);
    assert_eq!(decoded.name[0], 0xd700);
    assert_eq!(decoded.name[8], 0);
    assert_eq!(decoded.name[21], 0xd715);

    assert_eq!(
        decode_rename_city(&wire[..52]),
        Err(ObjectCommandDecodeError::WrongLength {
            expected: 53,
            actual: 52,
        })
    );
    wire[0] = 74;
    assert_eq!(
        decode_rename_city(&wire),
        Err(ObjectCommandDecodeError::WrongOpcode {
            expected: 75,
            actual: 74,
        })
    );
}

#[test]
fn city_write_precedes_selection_normalization_and_both_name_updates() {
    let command = command(2, 47);
    let first = command.who * HOTKEY_GROUPS_PER_OWNER;
    let facts = RenameCityHostFacts {
        object: object_receipt(&command, 0x21, Some(-3)),
        normalized_groups: vec![
            normalized(first, 2, &[1, 2]),
            normalized(first + 1, 2, &[47, 9]),
        ],
        frame: 0x1122_3344,
    };

    let plan = plan_rename_city(&command, &facts).unwrap();
    assert_eq!(plan.matching_group, Some(first + 1));
    assert!(matches!(plan.steps[0], RenameCityStep::Presentation(_)));
    assert_eq!(
        plan.steps[1],
        RenameCityStep::LookupObject(facts.object.request)
    );
    assert_eq!(
        plan.steps[2],
        RenameCityStep::WriteCityName(CityNameMutation {
            who: 2,
            city_type: -3,
            name: command.name,
        })
    );
    assert!(matches!(
        plan.steps[3],
        RenameCityStep::NormalizeSelection(_)
    ));
    assert!(matches!(
        plan.steps[4],
        RenameCityStep::NormalizeSelection(_)
    ));
    assert_eq!(
        plan.steps[5],
        RenameCityStep::StampSelectionFrame {
            slot: first + 1,
            frame: 0x1122_3344,
        }
    );
    assert_eq!(
        plan.steps[6],
        RenameCityStep::Presentation(RenameCityPresentationReceipt::HotKeyNameUpdate {
            slot: first + 1,
            mode: 0,
            origin: HotKeyNameUpdateOrigin::FindGroup,
        })
    );
    assert_eq!(
        plan.steps[7],
        RenameCityStep::Presentation(RenameCityPresentationReceipt::HotKeyNameUpdate {
            slot: first + 1,
            mode: 0,
            origin: HotKeyNameUpdateOrigin::RenameCityCaller,
        })
    );
}

#[test]
fn non_city_or_invalid_object_still_normalizes_all_eighteen_groups() {
    let command = command(0, 12);
    let facts = RenameCityHostFacts {
        object: object_receipt(&command, 0x20, Some(4)),
        normalized_groups: (0..HOTKEY_GROUPS_PER_OWNER)
            .map(|slot| normalized(slot, 0, &[12]))
            .collect(),
        frame: 8,
    };

    // City but not Object-valid: write the city name, then scan every group without a hit.
    let plan = plan_rename_city(&command, &facts).unwrap();
    assert_eq!(plan.matching_group, None);
    assert!(plan
        .steps
        .iter()
        .any(|step| matches!(step, RenameCityStep::WriteCityName(_))));
    assert_eq!(
        plan.steps
            .iter()
            .filter(|step| matches!(step, RenameCityStep::NormalizeSelection(_)))
            .count(),
        HOTKEY_GROUPS_PER_OWNER as usize
    );
    assert!(!plan.steps.iter().any(|step| matches!(
        step,
        RenameCityStep::Presentation(RenameCityPresentationReceipt::HotKeyNameUpdate { .. })
    )));
}

#[test]
fn signed_i16_member_comparison_does_not_alias_large_object_indices() {
    let command = command(1, 0x1_0001);
    let first = HOTKEY_GROUPS_PER_OWNER;
    let facts = RenameCityHostFacts {
        object: object_receipt(&command, 0x01, None),
        normalized_groups: (first..first + HOTKEY_GROUPS_PER_OWNER)
            .map(|slot| normalized(slot, 1, &[1]))
            .collect(),
        frame: 11,
    };

    let plan = plan_rename_city(&command, &facts).unwrap();
    assert_eq!(plan.matching_group, None);
    assert!(!plan
        .steps
        .iter()
        .any(|step| matches!(step, RenameCityStep::WriteCityName(_))));
}

#[test]
fn echoed_world_receipts_must_be_exact_ordered_and_minimal() {
    let command = command(3, 5);
    let first = 3 * HOTKEY_GROUPS_PER_OWNER;
    let base = RenameCityHostFacts {
        object: object_receipt(&command, 0x01, None),
        normalized_groups: vec![normalized(first, 3, &[5])],
        frame: 99,
    };
    let plan = plan_rename_city(&command, &base).unwrap();
    let applied = RenameCityTransactionReceipt {
        request: command.clone(),
        status: RenameCityTransactionStatus::Applied,
        facts: Some(base.clone()),
        plan: Some(plan),
    };
    assert!(applied.validates(&command));

    let mut wrong_lookup = base.clone();
    wrong_lookup.object.request.object = 6;
    assert_eq!(
        plan_rename_city(&command, &wrong_lookup),
        Err(RenameCityPlanError::ObjectReceiptMismatch)
    );

    let mut extra = base.clone();
    extra.normalized_groups.push(normalized(first + 1, 3, &[]));
    assert_eq!(
        plan_rename_city(&command, &extra),
        Err(RenameCityPlanError::ExtraNormalizeReceipt {
            first_extra_slot: first + 1,
        })
    );

    let unavailable = RenameCityTransactionReceipt::unavailable(command.clone());
    assert!(unavailable.validates(&command));
}

#[test]
fn unsafe_retail_indices_fail_closed_before_receipt_consumption() {
    let command = command(8, -1);
    let facts = RenameCityHostFacts {
        object: object_receipt(&command, 0, None),
        normalized_groups: Vec::new(),
        frame: 0,
    };
    assert_eq!(
        plan_rename_city(&command, &facts),
        Err(RenameCityPlanError::UnsafeObjectIndex { who: 8, object: -1 })
    );
}
