#[path = "../src/systems/bhs_type_factory.rs"]
mod bhs_type_factory;
#[path = "../src/systems/bhs_type_table.rs"]
mod bhs_type_table;

use bhs_type_factory::*;
use bhs_type_table::*;

fn digest(byte: u8) -> Sha256Digest {
    Sha256Digest([byte; 32])
}

fn witness(role: TypeSourceRole, component_byte: u8) -> TypeSourceWitness {
    TypeSourceWitness {
        role,
        composition: RulesCompositionId(digest(1)),
        manifest_sha256: digest(2),
        component_sha256: digest(component_byte),
    }
}

fn source_row(slot: usize) -> ComposedTypeRow {
    let mut row = TypeRow::empty(slot);
    row.name = format!("Type {slot}");
    row.type_name = format!("Type family {slot}");
    row.common.display_name = format!("Display type {slot}");
    row.common.tribe_mask = 0x00ff_ffff;
    row.common.job_time = slot as u32 + 10;
    row.common.preq = [-1, -1, -1];
    row.from = -1;
    row.where_type = -1;

    ComposedTypeRow {
        index: row.index,
        name: row.name,
        type_name: row.type_name,
        common: row.common,
        from: row.from,
        where_type: row.where_type,
        modified: row.modified,
        grid_x: row.grid_x,
        grid_y: row.grid_y,
        is_non_strict: Some(row.is_list),
        body: row.body,
    }
}

fn fixture() -> TypeBuiltinFactoryInput {
    let types = (0..NUM_TYPES).map(|slot| Some(source_row(slot))).collect();
    let tribes = (0..NUM_TRIBES)
        .map(|slot| Some(format!("Tribe {slot}")))
        .collect();
    let mut leaders: Vec<_> = (0..NUM_LEADERS)
        .map(|_| Some(LeaderTypeMasks::default()))
        .collect();
    leaders[0] = Some(LeaderTypeMasks {
        leader_flags: 1,
        tribe: 3,
        ..LeaderTypeMasks::default()
    });

    TypeBuiltinFactoryInput {
        types: WitnessedTypeSource {
            witness: witness(TypeSourceRole::TypeRows, 3),
            value: types,
        },
        tribes: WitnessedTypeSource {
            witness: witness(TypeSourceRole::TribeRoster, 4),
            value: tribes,
        },
        leaders: WitnessedTypeSource {
            witness: witness(TypeSourceRole::LeaderMasks, 5),
            value: leaders,
        },
    }
}

#[test]
fn exact_projection_retains_provenance_and_captures_pristine_backups() {
    let produced = produce_type_builtin_state(fixture()).unwrap();
    assert_eq!(produced.state().types.rows().len(), NUM_TYPES);
    assert_eq!(produced.state().leaders.len(), NUM_LEADERS);
    assert!(!produced.state().is_dirty());
    assert_eq!(
        produced.provenance().composition,
        RulesCompositionId(digest(1))
    );
    assert_eq!(produced.provenance().type_rows_sha256, digest(3));
    assert_eq!(produced.provenance().tribe_roster_sha256, digest(4));
    assert_eq!(produced.provenance().leader_masks_sha256, digest(5));

    let (mut state, provenance) = produced.into_parts();
    let pristine = state.types.row(REGULAR_UNIT_BEGIN).common.clone();
    assert_eq!(state.disable_type("type 50").unwrap(), 50);
    state.types.row_mut(REGULAR_UNIT_BEGIN).common.job_time = 99_999;
    state.types.row_mut(REGULAR_UNIT_BEGIN).common.display_name = "mutated".into();
    assert_eq!(state.enable_type("TYPE 50").unwrap(), 50);
    assert_eq!(state.types.row(REGULAR_UNIT_BEGIN).common, pristine);
    assert_eq!(provenance.manifest_sha256, digest(2));
}

#[test]
fn mixed_or_unbound_source_witnesses_fail_before_projection() {
    let mut input = fixture();
    input.types.witness.role = TypeSourceRole::TribeRoster;
    assert_eq!(
        produce_type_builtin_state(input).unwrap_err(),
        TypeBuiltinFactoryError::WrongSourceRole {
            expected: TypeSourceRole::TypeRows,
            got: TypeSourceRole::TribeRoster,
        }
    );

    let mut input = fixture();
    input.tribes.witness.composition = RulesCompositionId(digest(9));
    assert_eq!(
        produce_type_builtin_state(input).unwrap_err(),
        TypeBuiltinFactoryError::MixedComposition {
            role: TypeSourceRole::TribeRoster,
        }
    );

    let mut input = fixture();
    input.leaders.witness.manifest_sha256 = digest(8);
    assert_eq!(
        produce_type_builtin_state(input).unwrap_err(),
        TypeBuiltinFactoryError::MixedManifest {
            role: TypeSourceRole::LeaderMasks,
        }
    );

    let mut input = fixture();
    input.types.witness.component_sha256 = Sha256Digest([0; 32]);
    assert_eq!(
        produce_type_builtin_state(input).unwrap_err(),
        TypeBuiltinFactoryError::EmptyComponentDigest {
            role: TypeSourceRole::TypeRows,
        }
    );
}

#[test]
fn sparse_rows_and_names_never_become_inert_owner_entries() {
    let mut input = fixture();
    input.types.value.pop();
    assert_eq!(
        produce_type_builtin_state(input).unwrap_err(),
        TypeBuiltinFactoryError::WrongComponentCount {
            component: FactoryComponent::TypeRows,
            expected: NUM_TYPES,
            got: NUM_TYPES - 1,
        }
    );

    let mut input = fixture();
    input.types.value[177] = None;
    assert_eq!(
        produce_type_builtin_state(input).unwrap_err(),
        TypeBuiltinFactoryError::MissingTypeRow { slot: 177 }
    );

    let mut input = fixture();
    input.types.value[12].as_mut().unwrap().name = "  ".into();
    assert_eq!(
        produce_type_builtin_state(input).unwrap_err(),
        TypeBuiltinFactoryError::MissingName {
            slot: 12,
            field: RequiredName::Internal,
        }
    );

    let mut input = fixture();
    input.types.value[31].as_mut().unwrap().type_name.clear();
    assert_eq!(
        produce_type_builtin_state(input).unwrap_err(),
        TypeBuiltinFactoryError::MissingName {
            slot: 31,
            field: RequiredName::Type,
        }
    );
}

#[test]
fn relation_and_domain_projection_fail_closed() {
    let mut input = fixture();
    input.types.value[90].as_mut().unwrap().is_non_strict = None;
    assert_eq!(
        produce_type_builtin_state(input).unwrap_err(),
        TypeBuiltinFactoryError::MissingNonStrictRelation { slot: 90 }
    );

    let mut input = fixture();
    input.types.value[90].as_mut().unwrap().is_non_strict = Some(vec![89]);
    assert_eq!(
        produce_type_builtin_state(input).unwrap_err(),
        TypeBuiltinFactoryError::RelationMissingSelf { slot: 90 }
    );

    let mut input = fixture();
    input.types.value[90].as_mut().unwrap().is_non_strict = Some(vec![90, 90]);
    assert_eq!(
        produce_type_builtin_state(input).unwrap_err(),
        TypeBuiltinFactoryError::DuplicateRelationTarget {
            slot: 90,
            target: 90,
        }
    );

    let mut input = fixture();
    input.types.value[90].as_mut().unwrap().is_non_strict = Some(vec![90, 806]);
    assert_eq!(
        produce_type_builtin_state(input).unwrap_err(),
        TypeBuiltinFactoryError::RelationOutOfRange {
            slot: 90,
            target: 806,
        }
    );

    let mut input = fixture();
    input.types.value[REGULAR_UNIT_BEGIN].as_mut().unwrap().body = TypeBody::Other;
    assert_eq!(
        produce_type_builtin_state(input).unwrap_err(),
        TypeBuiltinFactoryError::DomainMismatch {
            slot: REGULAR_UNIT_BEGIN,
            expected: TypeDomain::Unit,
            got: TypeDomain::Other,
        }
    );

    let mut input = fixture();
    input.types.value[12].as_mut().unwrap().common.tribe_mask = 0x0100_0000;
    assert_eq!(
        produce_type_builtin_state(input).unwrap_err(),
        TypeBuiltinFactoryError::TribeMaskOutOfRange {
            slot: 12,
            mask: 0x0100_0000,
        }
    );
}

#[test]
fn tribes_and_leaders_are_exact_not_zero_filled() {
    let mut input = fixture();
    input.tribes.value[6] = None;
    assert_eq!(
        produce_type_builtin_state(input).unwrap_err(),
        TypeBuiltinFactoryError::MissingTribe { slot: 6 }
    );

    let mut input = fixture();
    input.leaders.value[2] = None;
    assert_eq!(
        produce_type_builtin_state(input).unwrap_err(),
        TypeBuiltinFactoryError::MissingLeader { slot: 2 }
    );

    let mut input = fixture();
    let leader = input.leaders.value[0].as_mut().unwrap();
    leader.tribe = NUM_TRIBES as i32;
    assert_eq!(
        produce_type_builtin_state(input).unwrap_err(),
        TypeBuiltinFactoryError::ActiveLeaderTribeOutOfRange {
            leader: 0,
            tribe: NUM_TRIBES as i32,
        }
    );
}

#[test]
fn leader_mask_headers_and_unused_tail_bits_are_validated() {
    let mut input = fixture();
    input.leaders.value[0].as_mut().unwrap().tech.bits = 805;
    assert_eq!(
        produce_type_builtin_state(input).unwrap_err(),
        TypeBuiltinFactoryError::LeaderMaskHeaderMismatch {
            leader: 0,
            mask: LeaderMaskKind::Tech,
            bits: 805,
            size: 101,
        }
    );

    let mut input = fixture();
    input.leaders.value[1].as_mut().unwrap().obs_flags.bytes[100] = 0x80;
    assert_eq!(
        produce_type_builtin_state(input).unwrap_err(),
        TypeBuiltinFactoryError::LeaderMaskPaddingSet {
            leader: 1,
            mask: LeaderMaskKind::ObservationFlags,
            tail: 0x80,
        }
    );
}
