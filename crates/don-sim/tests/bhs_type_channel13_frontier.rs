use don_sim::systems::bhs_type_channel13_frontier as frontier;
use don_sim::systems::bhs_type_factory::{RulesCompositionId, Sha256Digest, TypeBuiltinProvenance};
use don_sim::systems::bhs_type_table::{
    BuildRestoreFields, CommonRestoreFields, LeaderTypeMasks, ObjectRestoreFields, TribeRoster,
    TypeBackup, TypeBody, TypeBuiltinState, TypeRow, TypeTable, UnitRestoreFields, BUILD_BEGIN,
    BUILD_END, NUM_LEADERS, NUM_TRIBES, NUM_TYPES, REGULAR_UNIT_BEGIN, REGULAR_UNIT_END,
};
use frontier::*;

fn digest(byte: u8) -> Sha256Digest {
    Sha256Digest([byte; 32])
}

fn provenance() -> TypeBuiltinProvenance {
    TypeBuiltinProvenance {
        composition: RulesCompositionId(digest(1)),
        manifest_sha256: digest(2),
        type_rows_sha256: digest(3),
        tribe_roster_sha256: digest(4),
        leader_masks_sha256: digest(5),
    }
}

fn fixture_state() -> TypeBuiltinState {
    let mut rows: Vec<_> = (0..NUM_TYPES).map(TypeRow::empty).collect();
    rows[50].name = "Citizen".into();
    rows[50].type_name = "CitizenFamily".into();
    rows[414].name = "Small City".into();
    rows[414].type_name = "CityFamily".into();

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
    let tribes = TribeRoster::new(
        (0..NUM_TRIBES)
            .map(|index| format!("Tribe {index}"))
            .collect(),
    )
    .unwrap();
    let leaders: [LeaderTypeMasks; NUM_LEADERS] = std::array::from_fn(|_| Default::default());
    TypeBuiltinState::new(types, tribes, leaders)
}

fn empty_tail(kind: TypeRuleKind) -> TypeTailSource {
    match kind {
        TypeRuleKind::Good => TypeTailSource::Good(vec![0; 68]),
        TypeRuleKind::Unit => TypeTailSource::Unit {
            at_692: vec![0; 24],
            at_724: vec![0; 8],
            at_732: vec![0; 4],
            at_736: vec![0; 756],
        },
        TypeRuleKind::Build => TypeTailSource::Build(vec![0; 49]),
        TypeRuleKind::Tech => TypeTailSource::Tech(vec![0; 27]),
        TypeRuleKind::Spell => TypeTailSource::Spell(vec![0; 48]),
        TypeRuleKind::Object | TypeRuleKind::Type => TypeTailSource::None,
    }
}

fn source_for(state: &TypeBuiltinState) -> TypeWalkSource {
    let mut object_ordinal = 0;
    let rows = state
        .types
        .rows()
        .iter()
        .enumerate()
        .map(|(slot, owner_row)| {
            let kind = TypeRuleKind::for_shipped_slot(slot).unwrap();
            let mut type_base = vec![0; 90];
            // The otherwise-zero fixture still owns these nonzero TypeData fields.
            type_base[0..4].copy_from_slice(&(slot as i32).to_le_bytes());
            type_base[56..60].copy_from_slice(&(-1i32).to_le_bytes());
            type_base[60..64].copy_from_slice(&(-1i32).to_le_bytes());

            let (object, object_arrays) = if matches!(
                kind,
                TypeRuleKind::Good
                    | TypeRuleKind::Unit
                    | TypeRuleKind::Build
                    | TypeRuleKind::Object
            ) {
                // Retail walked 2,363 total elements across 962 nonempty arrays.  Every
                // non-strict array is nonempty; 418 strict arrays carry the remaining 1,819.
                let strict_count = if object_ordinal < 147 {
                    5
                } else if object_ordinal < 418 {
                    4
                } else {
                    0
                };
                object_ordinal += 1;
                let strict: Vec<u16> = (0..strict_count).map(|value| value as u16).collect();
                let non_strict = owner_row.is_list.clone();
                (
                    Some(vec![0; 152]),
                    Some([
                        U16ArraySource {
                            capacity: non_strict.len() as i32,
                            grow: 1,
                            flags: 0x40,
                            elements: non_strict,
                        },
                        U16ArraySource {
                            capacity: strict.len() as i32,
                            grow: 2,
                            flags: 0x40,
                            elements: strict,
                        },
                    ]),
                )
            } else {
                (None, None)
            };
            TypeWalkSourceRow {
                slot,
                kind: Some(kind),
                type_base: Some(type_base),
                object,
                object_arrays,
                tail: Some(empty_tail(kind)),
                strings: Some(TypeStringSource {
                    name: owner_row.name.clone(),
                    display_name: owner_row.common.display_name.clone(),
                    type_name: owner_row.type_name.clone(),
                }),
            }
        })
        .collect();
    let mut source = TypeWalkSource {
        provenance: provenance(),
        mode: TypeWalkSourceMode::ExactComposition {
            pristine_after_types: 0,
        },
        boundary: TypeWalkBoundaryProof {
            strings: Some(StringWalkProof::ChecksumGateAt006631b3),
            virtuals: Some(VirtualWalkProof::ShippedSlotBandsAt00669800),
            caches: Some(CacheWalkProof::NamedRangesOnly),
        },
        rows,
    };
    refresh_checkpoint(&mut source);
    source
}

fn refresh_checkpoint(source: &mut TypeWalkSource) {
    let (pristine_after_types, _, _) = candidate_source_checkpoint(source).unwrap();
    source.mode = TypeWalkSourceMode::ExactComposition {
        pristine_after_types,
    };
}

fn receipt(state: &TypeBuiltinState) -> InstalledTypeOwnerReceipt {
    InstalledTypeOwnerReceipt {
        provenance: provenance(),
        dirty: Some(state.is_dirty()),
        mutation_revision: Some(state.mutation_revision()),
    }
}

fn le_i32(row: &ProjectedTypeRow, offset: usize) -> i32 {
    i32::from_le_bytes(std::array::from_fn(|index| {
        row.walked_byte(offset + index).unwrap()
    }))
}

#[test]
fn shipped_bands_cardinality_order_and_walked_width_are_exact() {
    let state = fixture_state();
    let source = source_for(&state);
    let (checkpoint, bytes, array_elements) = candidate_source_checkpoint(&source).unwrap();
    assert_eq!(bytes, RETAIL_TYPE_WALKED_BYTES);
    assert_eq!(array_elements, SHIPPED_OBJECT_ARRAY_ELEMENTS);

    let projected = project_type_owner(&state, receipt(&state), &source).unwrap();
    assert_eq!(projected.rows().len(), 806);
    assert_eq!(projected.after_types(), checkpoint);
    assert_eq!(projected.pristine_after_types(), checkpoint);
    assert_eq!(projected.bytes_walked(), RETAIL_TYPE_WALKED_BYTES);
    assert_eq!(projected.rows()[0].kind(), TypeRuleKind::Good);
    assert_eq!(projected.rows()[49].kind(), TypeRuleKind::Good);
    assert_eq!(projected.rows()[50].kind(), TypeRuleKind::Unit);
    assert_eq!(projected.rows()[413].kind(), TypeRuleKind::Unit);
    assert_eq!(projected.rows()[414].kind(), TypeRuleKind::Build);
    assert_eq!(projected.rows()[542].kind(), TypeRuleKind::Build);
    assert_eq!(projected.rows()[543].kind(), TypeRuleKind::Object);
    assert_eq!(projected.rows()[544].kind(), TypeRuleKind::Tech);
    assert_eq!(projected.rows()[629].kind(), TypeRuleKind::Spell);
    assert_eq!(projected.rows()[684].kind(), TypeRuleKind::Type);
    assert_eq!(projected.rows()[805].slot(), 805);
}

#[test]
fn common_object_and_unit_mutations_land_at_exact_walk_offsets() {
    let mut state = fixture_state();
    let source = source_for(&state);
    state.set_type_job_time("Citizen", 300).unwrap();
    let row = state.types.row_mut(50);
    row.common = CommonRestoreFields {
        job_time: 3,
        tribe_mask: 0x0012_3456,
        display_name: "excluded String".into(),
        preq: [-2, 9, 10],
        costs: [11, 12, 13, 14, 15, 16],
    };
    row.from = 44;
    row.where_type = 414;
    row.modified = 1;
    row.grid_x = -86;
    row.grid_y = -69;
    row.body = TypeBody::Unit {
        object: ObjectRestoreFields {
            attack: 101,
            min_range: 102,
            max_range: 103,
            hits: 104,
            armor: 105,
            los: 106,
            science_los: 107,
        },
        unit: UnitRestoreFields {
            moves: 201,
            turn_speed: 202,
            mana: 203,
            control_cost: 204,
        },
    };

    let projected = project_type_owner(&state, receipt(&state), &source).unwrap();
    let row = &projected.rows()[50];
    assert_eq!(le_i32(row, 4), 50);
    assert_eq!(le_i32(row, 8), 3);
    assert_eq!(le_i32(row, 16), 0x0012_3456);
    assert_eq!(le_i32(row, 24), 11);
    assert_eq!(le_i32(row, 44), 16);
    assert_eq!(le_i32(row, 48), -2);
    assert_eq!(le_i32(row, 60), 44);
    assert_eq!(le_i32(row, 64), 414);
    assert_eq!(le_i32(row, 88), 1);
    assert_eq!(row.walked_byte(92), Some(0xaa));
    assert_eq!(row.walked_byte(93), Some(0xbb));
    assert_eq!(le_i32(row, 488), 101);
    assert_eq!(le_i32(row, 504), 102);
    assert_eq!(le_i32(row, 544), 107);
    assert_eq!(le_i32(row, 704), 201);
    assert_eq!(le_i32(row, 708), 202);
    assert_eq!(le_i32(row, 748), 203);
    assert_eq!(le_i32(row, 752), 204);
    assert_eq!(
        row.walked_byte(116),
        None,
        "String bytes are absent, not zeroed"
    );
    assert_ne!(projected.after_types(), projected.pristine_after_types());
}

#[test]
fn build_restore_fields_land_in_the_single_exact_tail() {
    let mut state = fixture_state();
    let source = source_for(&state);
    state.set_type_job_time("Small City", 200).unwrap();
    state.types.row_mut(414).body = TypeBody::Build {
        object: ObjectRestoreFields {
            attack: 1,
            min_range: 2,
            max_range: 3,
            hits: 4,
            armor: 5,
            los: 6,
            science_los: 7,
        },
        build: BuildRestoreFields {
            town_hits: 21,
            plunder_value: 22,
            plunder_good: 23,
            garrison_max: 24,
            base_arrows: 25,
            most_shots: 26,
            wonder_val: 27,
        },
    };

    let projected = project_type_owner(&state, receipt(&state), &source).unwrap();
    let row = &projected.rows()[414];
    assert_eq!(le_i32(row, 692), 21);
    assert_eq!(le_i32(row, 708), 26);
    assert_eq!(le_i32(row, 712), 24);
    assert_eq!(le_i32(row, 716), 25);
    assert_eq!(le_i32(row, 720), 27);
    assert_eq!(le_i32(row, 724), 22);
    assert_eq!(le_i32(row, 728), 23);
}

#[test]
fn checksum_neutral_rename_still_requires_the_save_owner() {
    let mut state = fixture_state();
    let source = source_for(&state);
    state.rename_type("Citizen", "市民 🛶").unwrap();

    let projected = project_type_owner(&state, receipt(&state), &source).unwrap();
    assert_eq!(projected.after_types(), projected.pristine_after_types());
    let save = projected.persistence_owner();
    assert!(save.dirty());
    assert_eq!(save.mutation_revision(), 1);
    assert_eq!(save.provenance(), provenance());
    assert_eq!(save.projected_after_types(), projected.after_types());
    assert_eq!(
        save.state().types.row(50).common.display_name,
        "市民 🛶",
        "the checksum-neutral String mutation is retained for persistence"
    );
}

#[test]
fn relation_arrays_must_match_the_canonical_owner() {
    let state = fixture_state();
    let mut source = source_for(&state);
    source.rows[50].object_arrays.as_mut().unwrap()[0].elements = vec![51];
    refresh_checkpoint(&mut source);
    assert_eq!(
        project_type_owner(&state, receipt(&state), &source).unwrap_err(),
        TypeChannel13Error::RelationProjectionMismatch { slot: 50 }
    );
}

#[test]
fn unknown_string_virtual_and_cache_boundaries_fail_closed() {
    let state = fixture_state();
    let source = source_for(&state);
    for (which, expected) in [
        (0, TypeChannel13Error::UnknownStringWalk),
        (1, TypeChannel13Error::UnknownVirtualWalk),
        (2, TypeChannel13Error::UnknownCacheWalk),
    ] {
        let mut source = source.clone();
        match which {
            0 => source.boundary.strings = None,
            1 => source.boundary.virtuals = None,
            2 => source.boundary.caches = None,
            _ => unreachable!(),
        }
        assert_eq!(
            project_type_owner(&state, receipt(&state), &source).unwrap_err(),
            expected
        );
    }

    let mut missing_strings = source;
    missing_strings.rows[50].strings = None;
    assert_eq!(
        project_type_owner(&state, receipt(&state), &missing_strings).unwrap_err(),
        TypeChannel13Error::UnknownStrings { slot: 50 }
    );
}

#[test]
fn virtual_kind_and_slot_order_are_not_count_only_checks() {
    let state = fixture_state();
    let source = source_for(&state);

    let mut bad_kind = source.clone();
    bad_kind.rows[50].kind = Some(TypeRuleKind::Build);
    assert_eq!(
        project_type_owner(&state, receipt(&state), &bad_kind).unwrap_err(),
        TypeChannel13Error::KindOrder {
            slot: 50,
            expected: TypeRuleKind::Unit,
            actual: TypeRuleKind::Build,
        }
    );

    let mut bad_slot = source;
    bad_slot.rows[50].slot = 51;
    assert_eq!(
        project_type_owner(&state, receipt(&state), &bad_slot).unwrap_err(),
        TypeChannel13Error::SlotOrder {
            position: 50,
            slot: 51,
        }
    );
}

#[test]
fn provenance_and_dirty_revision_ambiguity_fail_before_projection() {
    let state = fixture_state();
    let source = source_for(&state);

    let mut wrong = receipt(&state);
    wrong.provenance.type_rows_sha256 = digest(9);
    assert_eq!(
        project_type_owner(&state, wrong, &source).unwrap_err(),
        TypeChannel13Error::ProvenanceMismatch
    );

    let ambiguous = InstalledTypeOwnerReceipt {
        provenance: provenance(),
        dirty: Some(false),
        mutation_revision: Some(1),
    };
    assert_eq!(
        project_type_owner(&state, ambiguous, &source).unwrap_err(),
        TypeChannel13Error::DirtyRevisionAmbiguous {
            dirty: false,
            revision: 1,
        }
    );

    let mut unknown = receipt(&state);
    unknown.dirty = None;
    assert_eq!(
        project_type_owner(&state, unknown, &source).unwrap_err(),
        TypeChannel13Error::UnknownDirtyState
    );
}

#[test]
fn a_clean_owner_cannot_diverge_from_its_pristine_projection() {
    let mut state = fixture_state();
    let source = source_for(&state);
    state.types.row_mut(50).common.job_time = 9;
    assert!(!state.is_dirty());
    let error = project_type_owner(&state, receipt(&state), &source).unwrap_err();
    let TypeChannel13Error::CleanOwnerDiverged {
        pristine,
        projected,
    } = error
    else {
        panic!("unexpected error: {error:?}");
    };
    let TypeWalkSourceMode::ExactComposition {
        pristine_after_types,
    } = source.mode
    else {
        unreachable!()
    };
    assert_eq!(pristine, pristine_after_types);
    assert_ne!(projected, pristine);
}

#[test]
fn a_clean_checksum_neutral_string_write_is_also_rejected() {
    let mut state = fixture_state();
    let source = source_for(&state);
    state.types.row_mut(50).common.display_name = "unreceipted".into();
    assert_eq!(
        project_type_owner(&state, receipt(&state), &source).unwrap_err(),
        TypeChannel13Error::CleanDisplayStringDiverged { slot: 50 }
    );
}

#[test]
fn shipped_mode_enforces_live_array_cardinality_before_checkpoint() {
    let state = fixture_state();
    let mut source = source_for(&state);
    source.rows[0].object_arrays.as_mut().unwrap()[1]
        .elements
        .pop();
    source.mode = TypeWalkSourceMode::ShippedRetail;
    assert_eq!(
        project_type_owner(&state, receipt(&state), &source).unwrap_err(),
        TypeChannel13Error::ShippedArrayCardinality {
            expected: SHIPPED_OBJECT_ARRAY_ELEMENTS,
            actual: SHIPPED_OBJECT_ARRAY_ELEMENTS - 1,
        }
    );
}
