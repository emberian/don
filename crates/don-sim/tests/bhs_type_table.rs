#[path = "../src/systems/bhs_type_table.rs"]
mod bhs_type_table;

use bhs_type_table::*;

fn fixture() -> TypeBuiltinState {
    let mut rows: Vec<_> = (0..NUM_TYPES).map(TypeRow::empty).collect();

    rows[50].name = "Citizen".into();
    rows[50].type_name = "CitizenFamily".into();
    rows[50].common = CommonRestoreFields {
        job_time: 50,
        tribe_mask: 0x00ff_ffff,
        display_name: "Citizen display".into(),
        preq: [-1, -1, -1],
        costs: [2, 0, 0, 0, 0, 0],
    };
    rows[50].where_type = 414;
    rows[50].grid_x = 2;
    rows[50].grid_y = 3;
    rows[50].body = TypeBody::Unit {
        object: ObjectRestoreFields {
            attack: 40,
            min_range: 1,
            max_range: 2,
            hits: 100,
            armor: 3,
            los: 4,
            science_los: 5,
        },
        unit: UnitRestoreFields {
            moves: 25,
            turn_speed: 6,
            mana: 7,
            control_cost: 8,
        },
    };

    rows[51].name = "Citizen Korean".into();
    rows[51].type_name = "CitizenFamilyKorean".into();
    rows[51].is_list = vec![51, 50];
    rows[51].common = CommonRestoreFields {
        job_time: 60,
        tribe_mask: 1 << 3,
        display_name: "Korean Citizen display".into(),
        preq: [550, -1, -1],
        costs: [3, 4, 5, 6, 7, 8],
    };
    rows[51].where_type = 415;
    rows[51].grid_x = 4;
    rows[51].grid_y = 5;
    rows[51].body = TypeBody::Unit {
        object: ObjectRestoreFields {
            attack: 41,
            min_range: 11,
            max_range: 12,
            hits: 101,
            armor: 13,
            los: 14,
            science_los: 15,
        },
        unit: UnitRestoreFields {
            moves: 26,
            turn_speed: 16,
            mana: 17,
            control_cost: 18,
        },
    };

    rows[414].name = "Small City".into();
    rows[414].type_name = "CityFamily".into();
    rows[414].common.tribe_mask = 1;
    rows[414].body = TypeBody::Build {
        object: ObjectRestoreFields {
            attack: 80,
            min_range: 0,
            max_range: 10,
            hits: 1_200,
            armor: 3,
            los: 12,
            science_los: 2,
        },
        build: BuildRestoreFields {
            town_hits: 2,
            plunder_value: 10,
            plunder_good: 0,
            garrison_max: 10,
            base_arrows: 2,
            most_shots: 3,
            wonder_val: 0,
        },
    };

    rows[415].name = "Large City".into();
    rows[415].type_name = "CityFamilyLarge".into();
    rows[415].is_list = vec![415, 414];
    rows[415].common.tribe_mask = 1;
    rows[415].body = TypeBody::Build {
        object: ObjectRestoreFields {
            attack: 90,
            min_range: 0,
            max_range: 11,
            hits: 2_500,
            armor: 5,
            los: 14,
            science_los: 2,
        },
        build: BuildRestoreFields {
            town_hits: 3,
            plunder_value: 15,
            plunder_good: 0,
            garrison_max: 15,
            base_arrows: 3,
            most_shots: 4,
            wonder_val: 0,
        },
    };

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

    let mut tribe_names: Vec<_> = (0..NUM_TRIBES)
        .map(|index| format!("Tribe {index}"))
        .collect();
    tribe_names[3] = "Romans".into();
    let tribes = TribeRoster::new(tribe_names).unwrap();
    let leaders = std::array::from_fn(|_| LeaderTypeMasks::default());
    TypeBuiltinState::new(types, tribes, leaders)
}

#[test]
fn global_disable_uses_first_name_and_non_strict_relation() {
    let mut state = fixture();
    state.types.row_mut(52).name = "Citizen".into();

    assert_eq!(state.disable_type("cItIzEn").unwrap(), 50);
    for index in [50, 51] {
        let row = state.types.row(index);
        assert_eq!(row.common.preq[0], -2);
        assert_eq!(row.common.tribe_mask, 0);
        assert_eq!(row.modified, 1);
    }
    assert_ne!(state.types.row(52).modified, 1);
    assert!(state.is_dirty());
}

#[test]
fn global_enable_restores_the_exact_unit_backup_and_only_that_backup() {
    let mut state = fixture();
    let saved_50 = state.types.row(50).clone();
    let saved_51 = state.types.row(51).clone();
    state.disable_type("Citizen").unwrap();

    for index in [50, 51] {
        let row = state.types.row_mut(index);
        row.common.job_time = 999;
        row.common.display_name = "runtime label".into();
        row.common.costs = [9; 6];
        row.where_type = 542;
        row.grid_x = -86; // raw byte 0xAA
        row.grid_y = -69; // raw byte 0xBB
        if let TypeBody::Unit { object, unit } = &mut row.body {
            *object = ObjectRestoreFields::default();
            *unit = UnitRestoreFields::default();
        }
    }
    state.types.row_mut(51).name = "runtime internal name".into();
    state.types.row_mut(51).type_name = "runtime type name".into();

    assert_eq!(state.enable_type("CITIZEN").unwrap(), 50);
    for (index, saved) in [(50, saved_50), (51, saved_51)] {
        let row = state.types.row(index);
        assert_eq!(row.common, saved.common);
        assert_eq!(row.body, saved.body);
        assert_eq!(row.modified, 1, "restore does not clear modified");
        assert_eq!(row.where_type, 542, "where is outside TypeBak restore");
        assert_eq!(row.grid_x, -86);
        assert_eq!(row.grid_y, -69);
    }
    assert_eq!(state.types.row(51).name, "runtime internal name");
    assert_eq!(state.types.row(51).type_name, "runtime type name");
}

#[test]
fn global_enable_restores_the_exact_build_backup() {
    let mut state = fixture();
    let saved = state.types.row(415).clone();
    state.disable_type("Small City").unwrap();
    state.types.row_mut(415).common.job_time = 77;
    state.types.row_mut(415).body = TypeBody::Build {
        object: ObjectRestoreFields::default(),
        build: BuildRestoreFields::default(),
    };

    assert_eq!(state.enable_type("small city").unwrap(), 414);
    assert_eq!(state.types.row(415).common, saved.common);
    assert_eq!(state.types.row(415).body, saved.body);
    assert_eq!(state.types.row(415).modified, 1);
}

#[test]
fn tribe_mutations_update_the_type_and_exact_leader_masks() {
    let mut state = fixture();
    for leader in &mut state.leaders[..3] {
        leader.tech.set(51, true);
        leader.obs_flags.set(51, true);
    }
    state.leaders[0].leader_flags = 1;
    state.leaders[0].tribe = 3;
    state.leaders[1].leader_flags = 1;
    state.leaders[1].tribe = 3;
    state.leaders[1].tech.flags = 7;
    state.leaders[1].obs_flags.flags = 9;
    state.leaders[2].leader_flags = 0;
    state.leaders[2].tribe = 3;

    assert_eq!(
        state
            .disable_type_by_tribe("citizen korean", "ROMANS")
            .unwrap(),
        51
    );
    assert_eq!(state.types.row(51).common.tribe_mask & (1 << 3), 0);
    assert!(!state.leaders[0].tech.get(51));
    assert_eq!(state.leaders[0].tech.flags, 2);
    assert!(!state.leaders[1].tech.get(51));
    assert_eq!(state.leaders[1].tech.flags, 7);
    assert!(state.leaders[2].tech.get(51));

    assert_eq!(
        state
            .enable_type_by_tribe("Citizen Korean", "Romans")
            .unwrap(),
        51
    );
    assert_ne!(state.types.row(51).common.tribe_mask & (1 << 3), 0);
    assert!(!state.leaders[0].obs_flags.get(51));
    assert_eq!(state.leaders[0].obs_flags.flags, 2);
    assert!(!state.leaders[1].obs_flags.get(51));
    assert_eq!(state.leaders[1].obs_flags.flags, 9);
    assert!(state.leaders[2].obs_flags.get(51));
}

#[test]
fn with_type_name_uses_a_distinct_first_match_field() {
    let mut state = fixture();
    state.types.row_mut(50).type_name = "SharedFamily".into();
    state.types.row_mut(51).type_name = "SharedFamily".into();
    state.types.row_mut(50).common.tribe_mask = 0;
    state.types.row_mut(51).common.tribe_mask = 0;

    assert_eq!(
        state
            .enable_type_by_tribe_with_type_name("sharedfamily", "Romans")
            .unwrap(),
        50
    );
    assert_ne!(state.types.row(50).common.tribe_mask & (1 << 3), 0);
    assert_eq!(state.types.row(51).common.tribe_mask & (1 << 3), 0);
}

#[test]
fn five_arg_unit_path_is_non_atomic_and_truncates_grid_bytes() {
    let mut state = fixture();
    state.types.row_mut(50).common.tribe_mask = 0;

    assert_eq!(
        state
            .enable_type_by_tribe_at("Citizen", "Romans", "missing", 300, -1)
            .unwrap(),
        -1
    );
    assert_ne!(
        state.types.row(50).common.tribe_mask & (1 << 3),
        0,
        "two-argument base mutation happens before builder failure"
    );

    assert_eq!(
        state
            .enable_type_by_tribe_at("Citizen", "Romans", "Small City", 300, -1)
            .unwrap(),
        1
    );
    let row = state.types.row(50);
    assert_eq!(row.where_type, 414);
    assert_eq!(row.grid_x, -1, "column low byte is stored in grid_x");
    assert_eq!(row.grid_y, 44, "row is stored in grid_y");

    assert_eq!(
        state
            .enable_type_by_tribe_with_type_name_at(
                "CitizenFamily",
                "Romans",
                "Large City",
                -2,
                258,
            )
            .unwrap(),
        1
    );
    let row = state.types.row(50);
    assert_eq!(row.where_type, 415);
    assert_eq!(row.grid_x, 2);
    assert_eq!(row.grid_y, -2);
}

#[test]
fn five_arg_build_path_returns_one_and_ignores_placement_arguments() {
    let mut state = fixture();
    state.types.row_mut(414).where_type = 99;
    state.types.row_mut(414).grid_x = 7;
    state.types.row_mut(414).grid_y = 8;

    assert_eq!(
        state
            .enable_type_by_tribe_at("Small City", "Romans", "missing", 300, -1)
            .unwrap(),
        1
    );
    let row = state.types.row(414);
    assert_eq!((row.where_type, row.grid_x, row.grid_y), (99, 7, 8));
}

#[test]
fn constructor_fails_closed_without_the_full_exact_owner() {
    let rows: Vec<_> = (0..NUM_TYPES).map(TypeRow::empty).collect();
    let backups = vec![None; NUM_TYPES];
    assert_eq!(
        TypeTable::new(rows, backups),
        Err(TypeTableError::MissingBackup { slot: 50 })
    );

    let mut rows: Vec<_> = (0..NUM_TYPES).map(TypeRow::empty).collect();
    rows[50].name = "non-ascii-é".into();
    let backups = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            ((REGULAR_UNIT_BEGIN..REGULAR_UNIT_END).contains(&index)
                || (BUILD_BEGIN..BUILD_END).contains(&index))
            .then(|| TypeBackup::capture_pristine(row))
        })
        .collect();
    assert_eq!(
        TypeTable::new(rows, backups),
        Err(TypeTableError::NonAsciiRetailName)
    );
}
