use don_bhs::disasm::asm;
use don_bhs::{builtin, Program, Script, ScriptFile, ScriptTy, Value, VarRef};
use don_sim::bhs_session::{BhsSession, BhsSessionSetupError};
use don_sim::script_runtime::{ScriptBinding, ScriptRuntime};
use don_sim::systems::bhs_type_channel13_frontier::*;
use don_sim::systems::bhs_type_factory::*;
use don_sim::systems::bhs_type_runtime::TypeBuiltinBoundaryError;
use don_sim::systems::bhs_type_table::*;
use don_sim::systems::save_load::SaveError;
use don_sim::tick::Sim;

fn digest(byte: u8) -> Sha256Digest {
    Sha256Digest([byte; 32])
}

fn witness(role: TypeSourceRole, component: u8) -> TypeSourceWitness {
    TypeSourceWitness {
        role,
        composition: RulesCompositionId(digest(1)),
        manifest_sha256: digest(2),
        component_sha256: digest(component),
    }
}

fn factory_input() -> TypeBuiltinFactoryInput {
    TypeBuiltinFactoryInput {
        types: WitnessedTypeSource {
            witness: witness(TypeSourceRole::TypeRows, 3),
            value: (0..NUM_TYPES)
                .map(|slot| {
                    let mut row = TypeRow::empty(slot);
                    row.name = format!("Internal {slot}");
                    row.type_name = format!("Family {slot}");
                    if slot == 50 {
                        row.name = "Citizen".into();
                        row.common.job_time = 50;
                    }
                    Some(ComposedTypeRow {
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
                    })
                })
                .collect(),
        },
        tribes: WitnessedTypeSource {
            witness: witness(TypeSourceRole::TribeRoster, 4),
            value: (0..NUM_TRIBES)
                .map(|slot| Some(format!("Tribe {slot}")))
                .collect(),
        },
        leaders: WitnessedTypeSource {
            witness: witness(TypeSourceRole::LeaderMasks, 5),
            value: (0..NUM_LEADERS)
                .map(|_| Some(LeaderTypeMasks::default()))
                .collect(),
        },
    }
}

fn put_i32(bytes: &mut [u8], absolute_offset: usize, range_start: usize, value: i32) {
    let offset = absolute_offset - range_start;
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], absolute_offset: usize, range_start: usize, value: u32) {
    let offset = absolute_offset - range_start;
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
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

fn put_tail_i32(tail: &mut TypeTailSource, absolute_offset: usize, value: i32) {
    let target = match tail {
        TypeTailSource::Good(bytes) => (&mut bytes[..], 692),
        TypeTailSource::Unit { at_692, .. } if absolute_offset < 716 => (&mut at_692[..], 692),
        TypeTailSource::Unit { at_724, .. } if absolute_offset < 732 => (&mut at_724[..], 724),
        TypeTailSource::Unit { at_732, .. } if absolute_offset < 736 => (&mut at_732[..], 732),
        TypeTailSource::Unit { at_736, .. } => (&mut at_736[..], 736),
        TypeTailSource::Build(bytes) => (&mut bytes[..], 692),
        TypeTailSource::Tech(bytes) => (&mut bytes[..], 456),
        TypeTailSource::Spell(bytes) => (&mut bytes[..], 456),
        TypeTailSource::None => panic!("fixture wrote a scalar outside this walk"),
    };
    put_i32(target.0, absolute_offset, target.1, value);
}

fn source_for(state: &TypeBuiltinState, provenance: TypeBuiltinProvenance) -> TypeWalkSource {
    let rows = state
        .types
        .rows()
        .iter()
        .enumerate()
        .map(|(slot, owner)| {
            let kind = TypeRuleKind::for_shipped_slot(slot).unwrap();
            let mut type_base = vec![0; 90];
            put_i32(&mut type_base, 4, 4, owner.index);
            put_u32(&mut type_base, 8, 4, owner.common.job_time);
            put_u32(&mut type_base, 16, 4, owner.common.tribe_mask);
            for (index, value) in owner.common.costs.iter().enumerate() {
                put_i32(&mut type_base, 24 + index * 4, 4, *value);
            }
            for (index, value) in owner.common.preq.iter().enumerate() {
                put_i32(&mut type_base, 48 + index * 4, 4, *value);
            }
            put_i32(&mut type_base, 60, 4, owner.from);
            put_i32(&mut type_base, 64, 4, owner.where_type);
            put_i32(&mut type_base, 88, 4, owner.modified);
            type_base[88] = owner.grid_x as u8;
            type_base[89] = owner.grid_y as u8;

            let mut object = matches!(
                kind,
                TypeRuleKind::Good
                    | TypeRuleKind::Unit
                    | TypeRuleKind::Build
                    | TypeRuleKind::Object
            )
            .then(|| vec![0; 152]);
            let mut tail = empty_tail(kind);
            match &owner.body {
                TypeBody::Unit {
                    object: fields,
                    unit,
                } => {
                    let bytes = object.as_mut().unwrap();
                    for (offset, value) in [
                        (488, fields.attack),
                        (504, fields.min_range),
                        (508, fields.max_range),
                        (528, fields.hits),
                        (532, fields.armor),
                        (540, fields.los),
                        (544, fields.science_los),
                    ] {
                        put_i32(bytes, offset, 484, value);
                    }
                    for (offset, value) in [
                        (704, unit.moves),
                        (708, unit.turn_speed),
                        (748, unit.mana),
                        (752, unit.control_cost),
                    ] {
                        put_tail_i32(&mut tail, offset, value);
                    }
                }
                TypeBody::Build {
                    object: fields,
                    build,
                } => {
                    let bytes = object.as_mut().unwrap();
                    for (offset, value) in [
                        (488, fields.attack),
                        (504, fields.min_range),
                        (508, fields.max_range),
                        (528, fields.hits),
                        (532, fields.armor),
                        (540, fields.los),
                        (544, fields.science_los),
                    ] {
                        put_i32(bytes, offset, 484, value);
                    }
                    for (offset, value) in [
                        (692, build.town_hits),
                        (708, build.most_shots),
                        (712, build.garrison_max),
                        (716, build.base_arrows),
                        (720, build.wonder_val),
                        (724, build.plunder_value),
                        (728, build.plunder_good),
                    ] {
                        put_tail_i32(&mut tail, offset, value);
                    }
                }
                TypeBody::Other => {}
            }
            let object_arrays = object.as_ref().map(|_| {
                [
                    U16ArraySource {
                        capacity: owner.is_list.len() as i32,
                        grow: 1,
                        flags: 0x40,
                        elements: owner.is_list.clone(),
                    },
                    U16ArraySource {
                        capacity: 0,
                        grow: 0,
                        flags: 0,
                        elements: Vec::new(),
                    },
                ]
            });
            TypeWalkSourceRow {
                slot,
                kind: Some(kind),
                type_base: Some(type_base),
                object,
                object_arrays,
                tail: Some(tail),
                strings: Some(TypeStringSource {
                    name: owner.name.clone(),
                    display_name: owner.common.display_name.clone(),
                    type_name: owner.type_name.clone(),
                }),
            }
        })
        .collect();
    let mut source = TypeWalkSource {
        provenance,
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
    let checkpoint = candidate_source_checkpoint(&source).unwrap().0;
    source.mode = TypeWalkSourceMode::ExactComposition {
        pristine_after_types: checkpoint,
    };
    source
}

fn script_runtime() -> ScriptRuntime {
    let index = 290;
    let args = vec![Value::str("Citizen"), Value::Int(-1)];
    let instructions: Vec<(u8, Vec<u32>)> = vec![
        (0x47, vec![0]),
        (0x26, vec![VarRef::Const(1).encode()]),
        (0x26, vec![VarRef::Const(0).encode()]),
        (0x38, vec![index]),
        (0x27, Vec::new()),
        (0x3e, Vec::new()),
    ];
    let borrowed: Vec<_> = instructions
        .iter()
        .map(|(opcode, operands)| (*opcode, operands.as_slice()))
        .collect();
    let decl = builtin(index).unwrap();
    assert_eq!(decl.params, &[ScriptTy::Str, ScriptTy::Int]);
    ScriptRuntime::new(
        Program::single(ScriptFile {
            code: asm(&borrowed),
            const_pool: args,
            scripts: vec![Script {
                name: "game_tick".into(),
                entry: 0,
                return_type: ScriptTy::Void.tag(),
                ..Default::default()
            }],
            ..Default::default()
        }),
        Some(ScriptBinding::new(0, "game_tick")),
        None,
    )
    .unwrap()
}

fn source_and_input() -> (TypeWalkSource, TypeBuiltinFactoryInput) {
    let input = factory_input();
    let produced = produce_type_builtin_state(input.clone()).unwrap();
    (source_for(produced.state(), produced.provenance()), input)
}

#[test]
fn session_projects_clean_and_mutated_owner_at_the_live_revision() {
    let (source, input) = source_and_input();
    let pristine_checkpoint = candidate_source_checkpoint(&source).unwrap().0;
    let mut session =
        BhsSession::new_with_channel13(Sim::new(0x13, 8), script_runtime(), input, source).unwrap();

    assert!(session.status().type_channel13_owned);
    assert_eq!(
        session.type_channel13_checkpoint().unwrap(),
        pristine_checkpoint
    );
    let pristine_owner = session.type_persistence_owner().unwrap();
    assert!(!pristine_owner.dirty());
    assert_eq!(pristine_owner.mutation_revision(), 0);
    assert_eq!(pristine_owner.provenance(), session.type_provenance());
    assert_eq!(pristine_owner.state().types.row(50).common.job_time, 50);
    assert!(session.partial_channel_digest().is_ok());
    assert!(matches!(session.save(), Err(SaveError::BhsTypes(_))));

    session.do_frame().unwrap();

    let mutated_checkpoint = session.type_channel13_checkpoint().unwrap();
    assert_ne!(mutated_checkpoint, pristine_checkpoint);
    let mutated_owner = session.type_persistence_owner().unwrap();
    assert!(mutated_owner.dirty());
    assert_eq!(mutated_owner.mutation_revision(), 1);
    assert_eq!(mutated_owner.projected_after_types(), mutated_checkpoint);
    assert_eq!(mutated_owner.state().types.row(50).common.job_time, 1);
    assert!(session.partial_channel_digest().is_ok());
    assert_eq!(
        session.save(),
        Err(SaveError::BhsTypes(
            TypeBuiltinBoundaryError::SaveOwnerUnowned {
                mutation_revision: 1,
                dirty: true,
            }
        ))
    );
}

#[test]
fn state_only_session_stays_checksum_red() {
    let session = BhsSession::new(Sim::new(0x14, 8), script_runtime(), factory_input()).unwrap();
    assert!(!session.status().type_channel13_owned);
    assert_eq!(
        session.type_channel13_checkpoint(),
        Err(TypeChannel13Error::SourceUnowned)
    );
    assert_eq!(
        session.partial_channel_digest(),
        Err(TypeBuiltinBoundaryError::Channel13ProjectionUnowned)
    );
}

#[test]
fn mismatched_source_is_rejected_before_session_publication() {
    let (mut source, input) = source_and_input();
    source.provenance.type_rows_sha256 = digest(9);
    assert!(matches!(
        BhsSession::new_with_channel13(Sim::new(0x15, 8), script_runtime(), input, source,),
        Err(BhsSessionSetupError::Channel13(
            TypeChannel13Error::ProvenanceMismatch
        ))
    ));
}
