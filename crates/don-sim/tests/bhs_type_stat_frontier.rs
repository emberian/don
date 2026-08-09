#[path = "../src/systems/bhs_type_stat_frontier.rs"]
mod bhs_type_stat_frontier;
#[path = "../src/systems/bhs_type_table.rs"]
mod bhs_type_table;

use bhs_type_stat_frontier::*;
use bhs_type_table::*;

fn fixture() -> TypeBuiltinState {
    let mut rows: Vec<_> = (0..NUM_TYPES).map(TypeRow::empty).collect();

    rows[50].name = "Infantry".into();
    rows[50].is_list = vec![50];
    rows[50].modified = 7;
    rows[50].body = TypeBody::Unit {
        object: ObjectRestoreFields {
            attack: 11,
            min_range: 12,
            max_range: 13,
            hits: 14,
            armor: 15,
            los: 16,
            science_los: 17,
        },
        unit: UnitRestoreFields {
            moves: 18,
            turn_speed: 19,
            mana: 20,
            control_cost: 21,
        },
    };

    rows[51].name = "Roman Infantry".into();
    rows[51].is_list = vec![51, 50];
    rows[51].body = TypeBody::Unit {
        object: ObjectRestoreFields {
            attack: 31,
            min_range: 32,
            max_range: 33,
            hits: 34,
            armor: 35,
            los: 36,
            science_los: 37,
        },
        unit: UnitRestoreFields {
            moves: 38,
            turn_speed: 39,
            mana: 40,
            control_cost: 41,
        },
    };

    rows[401].name = "Last Regular Unit".into();
    rows[401].is_list = vec![401, 50];
    rows[401].body = rows[51].body.clone();

    rows[402].name = "Gaia Hawk".into();
    rows[52].is_list = vec![52, 402];

    rows[414].name = "Tower".into();
    rows[414].is_list = vec![414];
    rows[414].body = TypeBody::Build {
        object: ObjectRestoreFields {
            attack: 51,
            min_range: 52,
            max_range: 53,
            hits: 54,
            armor: 55,
            los: 56,
            science_los: 57,
        },
        build: BuildRestoreFields::default(),
    };
    rows[415].name = "Greek Tower".into();
    rows[415].is_list = vec![415, 414];
    rows[415].body = rows[414].body.clone();
    rows[542].name = "Last Building".into();
    rows[542].is_list = vec![542, 414];
    rows[542].body = rows[414].body.clone();

    // Registration 814 sends every non-Unit selection through the Build candidate range.
    rows[544].name = "Barter".into();
    rows[416].is_list = vec![416, 544];
    rows[416].body = rows[414].body.clone();

    let backups = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            ((REGULAR_UNIT_BEGIN..REGULAR_UNIT_END).contains(&index)
                || (BUILD_BEGIN..BUILD_END).contains(&index))
            .then(|| TypeBackup::capture_pristine(row))
        })
        .collect();
    let types = TypeTable::new(rows, backups).unwrap();
    let tribes = TribeRoster::new((0..NUM_TRIBES).map(|i| format!("Tribe {i}")).collect()).unwrap();
    let mut leaders: [LeaderTypeMasks; NUM_LEADERS] = std::array::from_fn(|_| Default::default());
    leaders[0].leader_flags = 3;
    leaders[1].leader_flags = 1;
    leaders[2].leader_flags = 2;
    leaders[3].leader_flags = 7;
    TypeBuiltinState::new(types, tribes, leaders)
}

fn admitted(plan: TypeStatPlan) -> TypeStatMutationPlan {
    match plan {
        TypeStatPlan::Admitted(plan) => plan,
        TypeStatPlan::Rejected => panic!("expected admitted plan"),
    }
}

#[test]
fn registrations_vas_sizes_fields_and_census_are_frozen() {
    let expected = [
        (529, 0x009f_5da0, 433, TypeStatField::Hits, 124, 31),
        (531, 0x009f_5fb0, 433, TypeStatField::Armor, 4, 3),
        (532, 0x009f_6170, 433, TypeStatField::Attack, 4, 3),
        (533, 0x009f_6330, 433, TypeStatField::MaxRange, 4, 3),
        (534, 0x009f_64f0, 433, TypeStatField::MinRange, 0, 0),
        (535, 0x009f_66b0, 369, TypeStatField::Moves, 5, 4),
        (538, 0x009f_6980, 291, TypeStatField::Mana, 9, 9),
        (814, 0x00a0_0550, 327, TypeStatField::LineOfSight, 44, 6),
    ];
    for (row, expected) in TYPE_STAT_CORPUS_CENSUS.iter().zip(expected) {
        assert_eq!(row.builtin.registration(), expected.0);
        assert_eq!(row.builtin.retail_va(), expected.1);
        assert_eq!(row.builtin.retail_bytes(), expected.2);
        assert_eq!(row.builtin.arity(), 2);
        assert_eq!(row.builtin.field(), expected.3);
        assert_eq!(row.calls, expected.4);
        assert_eq!(row.files, expected.5);
    }
    assert_eq!(
        TYPE_STAT_CORPUS_CENSUS
            .iter()
            .map(|row| u32::from(row.calls))
            .sum::<u32>(),
        TYPE_STAT_CORPUS_CALLS
    );
    assert_eq!(TypeStatBuiltin::ALL.len(), 8);
}

#[test]
fn object_stat_setters_use_first_match_narrow_relation_and_positive_gate() {
    let state = fixture();
    let cases = [
        (TypeStatBuiltin::SetMaxHealth, [14, 34, 34]),
        (TypeStatBuiltin::SetArmor, [15, 35, 35]),
        (TypeStatBuiltin::SetAttack, [11, 31, 31]),
        (TypeStatBuiltin::SetMaxRange, [13, 33, 33]),
        (TypeStatBuiltin::SetMinRange, [12, 32, 32]),
    ];
    for (builtin, previous) in cases {
        let plan = admitted(plan_type_stat_mutation(&state, builtin, "iNfAnTrY", 99).unwrap());
        assert_eq!(plan.selected, 50);
        assert_eq!(plan.return_value, 50);
        assert_eq!(
            plan.writes
                .iter()
                .map(|write| write.row)
                .collect::<Vec<_>>(),
            [50, 51, 401]
        );
        assert_eq!(
            plan.writes
                .iter()
                .map(|write| write.expected_value)
                .collect::<Vec<_>>(),
            previous
        );
        assert!(plan
            .writes
            .iter()
            .all(|write| { write.replacement_value == 99 && write.replacement_modified == 1 }));
        assert_eq!(plan.writes[0].expected_modified, 7);
        assert_eq!(
            plan.leader_recalcs,
            [
                LeaderStatRecalcCall {
                    leader_slot: 0,
                    kind: LeaderStatRecalc::WallThenUnit,
                },
                LeaderStatRecalcCall {
                    leader_slot: 3,
                    kind: LeaderStatRecalc::WallThenUnit,
                },
            ]
        );
    }

    assert_eq!(
        plan_type_stat_mutation(&state, TypeStatBuiltin::SetAttack, "Infantry", 0).unwrap(),
        TypeStatPlan::Rejected
    );
    assert_eq!(
        plan_type_stat_mutation(&state, TypeStatBuiltin::SetAttack, "Infantry", -1).unwrap(),
        TypeStatPlan::Rejected
    );
    assert_eq!(
        plan_type_stat_mutation(&state, TypeStatBuiltin::SetAttack, "Barter", 1).unwrap(),
        TypeStatPlan::Rejected
    );
}

#[test]
fn building_object_stats_use_only_414_through_542() {
    let state = fixture();
    let plan = admitted(
        plan_type_stat_mutation(&state, TypeStatBuiltin::SetMaxHealth, "Tower", 800).unwrap(),
    );
    assert_eq!(plan.selected, 414);
    assert_eq!(plan.return_value, 414);
    assert_eq!(
        plan.writes
            .iter()
            .map(|write| write.row)
            .collect::<Vec<_>>(),
        [414, 415, 542]
    );
    assert_eq!(
        plan.writes
            .iter()
            .map(|write| write.expected_value)
            .collect::<Vec<_>>(),
        [54, 54, 54]
    );
}

#[test]
fn speed_is_unit_only_and_preserves_gaia_selection_as_a_relation_root() {
    let state = fixture();
    let normal = admitted(
        plan_type_stat_mutation(&state, TypeStatBuiltin::SetUnitSpeed, "Infantry", 77).unwrap(),
    );
    assert_eq!(
        normal
            .writes
            .iter()
            .map(|write| write.row)
            .collect::<Vec<_>>(),
        [50, 51, 401]
    );
    assert_eq!(normal.writes[0].field, TypeStatField::Moves);

    let gaia = admitted(
        plan_type_stat_mutation(&state, TypeStatBuiltin::SetUnitSpeed, "Gaia Hawk", 77).unwrap(),
    );
    assert_eq!(gaia.selected, 402);
    assert_eq!(gaia.return_value, 402);
    assert_eq!(
        gaia.writes
            .iter()
            .map(|write| write.row)
            .collect::<Vec<_>>(),
        [52]
    );

    assert_eq!(
        plan_type_stat_mutation(&state, TypeStatBuiltin::SetUnitSpeed, "Tower", 77).unwrap(),
        TypeStatPlan::Rejected
    );
}

#[test]
fn max_craft_writes_pdb_mana_and_recomputes_only_unit_stats() {
    let state = fixture();
    let plan = admitted(
        plan_type_stat_mutation(&state, TypeStatBuiltin::SetUnitMaxCraft, "Infantry", 123).unwrap(),
    );
    assert_eq!(plan.writes[0].field, TypeStatField::Mana);
    assert_eq!(plan.writes[0].expected_value, 20);
    assert_eq!(plan.writes[0].replacement_value, 123);
    assert_eq!(
        plan.leader_recalcs,
        [
            LeaderStatRecalcCall {
                leader_slot: 0,
                kind: LeaderStatRecalc::UnitOnly,
            },
            LeaderStatRecalcCall {
                leader_slot: 3,
                kind: LeaderStatRecalc::UnitOnly,
            },
        ]
    );
}

#[test]
fn line_of_sight_clamps_accepts_nonunit_root_and_returns_literal_one() {
    let state = fixture();
    for (input, stored) in [(i32::MIN, 0), (-1, 0), (0, 0), (63, 63), (64, 64), (65, 64)] {
        let plan = admitted(
            plan_type_stat_mutation(&state, TypeStatBuiltin::SetLineOfSight, "Infantry", input)
                .unwrap(),
        );
        assert_eq!(plan.return_value, 1);
        assert_eq!(plan.stored_value, stored);
        assert!(plan
            .writes
            .iter()
            .all(|write| write.replacement_value == stored));
    }

    let nonunit = admitted(
        plan_type_stat_mutation(&state, TypeStatBuiltin::SetLineOfSight, "Barter", 12).unwrap(),
    );
    assert_eq!(nonunit.selected, 544);
    assert_eq!(nonunit.return_value, 1);
    assert_eq!(
        nonunit
            .writes
            .iter()
            .map(|write| write.row)
            .collect::<Vec<_>>(),
        [416]
    );
}

#[test]
fn failed_queries_are_mutation_free_and_unicode_mod_names_fail_closed() {
    let state = fixture();
    let before = state.clone();

    for query in ["", "missing"] {
        let plan =
            plan_type_stat_mutation(&state, TypeStatBuiltin::SetLineOfSight, query, i32::MAX)
                .unwrap();
        assert_eq!(plan, TypeStatPlan::Rejected);
        assert_eq!(plan.return_value(), -1);
    }
    assert_eq!(
        plan_type_stat_mutation(&state, TypeStatBuiltin::SetAttack, "Infantrý", 1),
        Err(TypeStatFrontierError::NonAsciiRetailName)
    );
    assert_eq!(state, before, "frontier planning never mutates the owner");
    assert!(!state.is_dirty());
    assert_eq!(state.mutation_revision(), 0);
}

#[test]
fn field_offsets_match_the_pdb_layout() {
    assert_eq!(TypeStatField::Attack.retail_offset(), 0x1e8);
    assert_eq!(TypeStatField::MinRange.retail_offset(), 0x1f8);
    assert_eq!(TypeStatField::MaxRange.retail_offset(), 0x1fc);
    assert_eq!(TypeStatField::Hits.retail_offset(), 0x210);
    assert_eq!(TypeStatField::Armor.retail_offset(), 0x214);
    assert_eq!(TypeStatField::LineOfSight.retail_offset(), 0x21c);
    assert_eq!(TypeStatField::Moves.retail_offset(), 0x2c0);
    assert_eq!(TypeStatField::Mana.retail_offset(), 0x2ec);
}

#[test]
fn canonical_commit_is_atomic_and_rejects_a_stale_relation_receipt() {
    let mut state = fixture();
    let plan = admitted(
        plan_type_stat_mutation(&state, TypeStatBuiltin::SetAttack, "Infantry", 99).unwrap(),
    );

    let TypeBody::Unit { object, .. } = &mut state.types.row_mut(51).body else {
        panic!("fixture row 51 is a Unit");
    };
    object.attack = 777;

    assert_eq!(
        state.apply_type_stat_plan(&plan),
        Err(TypeStatFrontierError::StaleWrite {
            row: 51,
            field: TypeStatField::Attack,
            expected_value: 31,
            observed_value: 777,
            expected_modified: 0,
            observed_modified: 0,
        })
    );
    let TypeBody::Unit { object, .. } = &state.types.row(50).body else {
        panic!("fixture row 50 is a Unit");
    };
    assert_eq!(
        object.attack, 11,
        "the earlier relation row was not partially written"
    );
    assert_eq!(state.mutation_revision(), 0);
    assert!(!state.is_dirty());
}

#[test]
fn canonical_commit_writes_the_whole_family_and_advances_once() {
    let mut state = fixture();
    let plan = admitted(
        plan_type_stat_mutation(&state, TypeStatBuiltin::SetArmor, "Infantry", 88).unwrap(),
    );
    state.apply_type_stat_plan(&plan).unwrap();

    for row in [50, 51, 401] {
        let TypeBody::Unit { object, .. } = &state.types.row(row).body else {
            panic!("fixture relation row is a Unit");
        };
        assert_eq!(object.armor, 88);
        assert_eq!(state.types.row(row).modified, 1);
    }
    assert_eq!(state.mutation_revision(), 1);
    assert!(state.is_dirty());
}
