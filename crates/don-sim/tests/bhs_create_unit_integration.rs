use std::path::Path;

use don_bhs::disasm::asm;
use don_bhs::{builtin, Program, Script, ScriptFile, ScriptTy, Value, VarRef};
use don_bhs_cc::sema::{self, Severity};
use don_sim::bhs_session::{BhsSession, BhsSessionSetupError};
use don_sim::script_runtime::{ScriptBinding, ScriptRuntime};
use don_sim::systems::bhs_create_unit_frontier::{
    CreateUnitPrefixError, CreateUnitRoute, CREATE_UNIT_CENSUS, CREATE_UNIT_COHORT_CALLS,
    SHIPPED_CORPUS_CALLS,
};
use don_sim::systems::bhs_create_unit_runtime::*;
use don_sim::systems::bhs_type_factory::*;
use don_sim::systems::bhs_type_runtime::TypeBuiltinBoundaryError;
use don_sim::systems::bhs_type_table::*;
use don_sim::systems::map_terrain::land;
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

fn factory_input(transport_relation: bool) -> TypeBuiltinFactoryInput {
    let rows = (0..NUM_TYPES)
        .map(|slot| {
            let mut row = TypeRow::empty(slot);
            row.name = format!("Internal {slot}");
            row.type_name = format!("Family {slot}");
            if slot == 50 {
                row.name = "Infantry".into();
                if transport_relation {
                    row.is_list.push(0x140);
                }
            }
            if slot == 51 {
                row.name = "Modern Infantry".into();
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
        .collect();
    TypeBuiltinFactoryInput {
        types: WitnessedTypeSource {
            witness: witness(TypeSourceRole::TypeRows, 3),
            value: rows,
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
                .map(|slot| {
                    Some(LeaderTypeMasks {
                        leader_flags: if slot == 0 { 3 } else { 0 },
                        ..LeaderTypeMasks::default()
                    })
                })
                .collect(),
        },
    }
}

fn create_input(include_modern_type: bool) -> CreateUnitRuntimeInput {
    let mut types = vec![None; NUM_TYPES];
    types[50] = Some(CreateUnitTypeProjection {
        type_index: 50,
        domain: 0,
        unit_flags: 0,
    });
    if include_modern_type {
        types[51] = Some(CreateUnitTypeProjection {
            type_index: 51,
            domain: 0,
            unit_flags: 0,
        });
    }

    let mut current_upgrade = vec![None; NUM_TYPES];
    current_upgrade[50] = Some(51);
    let mut graft = vec![None; NUM_TYPES];
    graft[50] = Some(50);
    graft[51] = Some(51);
    let mut leaders = vec![None; NUM_LEADERS];
    leaders[0] = Some(CreateUnitLeaderProjection {
        leader_slot: 0,
        current_upgrade,
        graft,
    });

    CreateUnitRuntimeInput {
        witness: CreateUnitProjectionWitness {
            composition: RulesCompositionId(digest(1)),
            manifest_sha256: digest(2),
            component_sha256: digest(6),
        },
        types,
        leaders,
        numeric_groups: vec![
            (
                0,
                vec![CreateUnitGroupMember {
                    owner: 0,
                    object_id: 7,
                }],
            ),
            (
                1,
                vec![CreateUnitGroupMember {
                    owner: 0,
                    object_id: 8,
                }],
            ),
        ],
    }
}

fn sim() -> Sim {
    let mut sim = Sim::new(0x508, 8);
    sim.step8.leaders[0].flags = 3;
    sim.map.world.wdata_mut(1, 1).land = land::FERTILE;
    sim
}

fn one_builtin_program(index: u32, who: i32, count: i32) -> Program {
    let decl = builtin(index).unwrap();
    assert_eq!(decl.arity, 5);
    let args = vec![
        Value::Int(who),
        Value::Int(4),
        Value::Int(4),
        Value::str("Infantry"),
        Value::Int(count),
    ];
    let mut instructions: Vec<(u8, Vec<u32>)> = vec![(0x47, vec![0])];
    for slot in (0..args.len()).rev() {
        instructions.push((0x26, vec![VarRef::Const(slot as u32).encode()]));
    }
    instructions.extend([(0x38, vec![index]), (0x27, Vec::new()), (0x3e, Vec::new())]);
    let borrowed = instructions
        .iter()
        .map(|(opcode, operands)| (*opcode, operands.as_slice()))
        .collect::<Vec<_>>();
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
    })
}

fn runtime(program: Program, binding: &str) -> ScriptRuntime {
    ScriptRuntime::new(program, Some(ScriptBinding::new(0, binding)), None).unwrap()
}

fn compile_source_fixture() -> Program {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/scenario_bhs_create_unit_zero.bhs");
    let includes = sema::IncludePath::with_roots([path.parent().unwrap().to_path_buf()]);
    let unit = sema::analyze(&path, &includes).unwrap();
    let (program, codegen_diags, _) = don_bhs_cc::codegen::compile(&unit);
    let errors = unit
        .diags
        .iter()
        .chain(codegen_diags.iter())
        .filter(|diag| diag.severity == Severity::Error)
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "{}", errors.join("\n"));
    program
}

#[test]
fn compiled_source_runs_all_three_zero_count_calls_through_one_opaque_session() {
    let scripts = runtime(compile_source_fixture(), "bhs_create_unit_zero_tick");
    let mut session =
        BhsSession::new_with_create_units(sim(), scripts, factory_input(false), create_input(true))
            .unwrap();

    session.do_frame().unwrap();

    let status = session.status();
    assert!(status.create_unit_owned);
    assert_eq!(status.create_unit_completed_calls, 3);
    assert_eq!(status.create_unit_faulted_calls, 0);
    assert_eq!(session.create_unit_numeric_group(0), Some([].as_slice()));
    assert_eq!(
        session.create_unit_numeric_group(1),
        Some(
            [CreateUnitGroupMember {
                owner: 0,
                object_id: 8,
            }]
            .as_slice()
        )
    );
    let receipt = session.last_create_unit_receipt().unwrap();
    assert_eq!(receipt.registration, 510);
    assert_eq!(receipt.pre_group.as_ref().unwrap().effective_type, 51);
    assert_eq!(receipt.cleared_group_key, None);
    assert_eq!(receipt.outcome, CreateUnitOutcome::Returned(-1));
    assert_eq!(receipt.allocation_results, []);

    assert_eq!(
        session.save(),
        Err(SaveError::BhsTypes(
            TypeBuiltinBoundaryError::CreateUnitOwnerUnowned {
                completed_calls: 3,
                faulted_calls: 0,
            }
        ))
    );
    assert_eq!(
        session.partial_channel_digest(),
        Err(TypeBuiltinBoundaryError::CreateUnitOwnerUnowned {
            completed_calls: 3,
            faulted_calls: 0,
        })
    );
}

#[test]
fn reset_and_append_forms_keep_the_native_group_key_asymmetry() {
    for (index, expected_clear) in [(508, Some(0)), (509, Some(0)), (510, None)] {
        let scripts = runtime(one_builtin_program(index, 1, 0), "game_tick");
        let mut session = BhsSession::new_with_create_units(
            sim(),
            scripts,
            factory_input(false),
            create_input(true),
        )
        .unwrap();
        session.do_frame().unwrap();
        let receipt = session.last_create_unit_receipt().unwrap();
        assert_eq!(receipt.cleared_group_key, expected_clear);
        assert_eq!(receipt.pre_group.as_ref().unwrap().group_append_key, 1);
        assert_eq!(
            session.create_unit_numeric_group(0).unwrap().is_empty(),
            expected_clear.is_some()
        );
        assert_eq!(session.create_unit_numeric_group(1).unwrap().len(), 1);
    }
}

#[test]
fn post_clear_missing_authority_is_retained_and_positive_allocation_stays_red() {
    let scripts = runtime(one_builtin_program(509, 1, 0), "game_tick");
    let mut missing_domain = BhsSession::new_with_create_units(
        sim(),
        scripts,
        factory_input(false),
        create_input(false),
    )
    .unwrap();
    assert!(missing_domain.do_frame().is_err());
    assert!(missing_domain
        .create_unit_numeric_group(0)
        .unwrap()
        .is_empty());
    let receipt = missing_domain.last_create_unit_receipt().unwrap();
    assert_eq!(receipt.cleared_group_key, Some(0));
    assert_eq!(
        receipt.outcome,
        CreateUnitOutcome::OwnerFault(CreateUnitOwnerFault::Prefix(
            CreateUnitPrefixError::DomainFactsMissing
        ))
    );

    let scripts = runtime(one_builtin_program(508, 1, 2), "game_tick");
    let mut allocation =
        BhsSession::new_with_create_units(sim(), scripts, factory_input(false), create_input(true))
            .unwrap();
    assert!(allocation.do_frame().is_err());
    let receipt = allocation.last_create_unit_receipt().unwrap();
    assert_eq!(receipt.route, Some(CreateUnitRoute::DirectGround));
    assert_eq!(receipt.allocation_results, []);
    assert_eq!(
        receipt.outcome,
        CreateUnitOutcome::OwnerFault(CreateUnitOwnerFault::PositiveAllocationAuthority)
    );
}

#[test]
fn native_prefix_and_post_policy_rejections_return_without_claiming_allocation() {
    let scripts = runtime(one_builtin_program(509, 9, 0), "game_tick");
    let mut bad_player =
        BhsSession::new_with_create_units(sim(), scripts, factory_input(false), create_input(true))
            .unwrap();
    bad_player.do_frame().unwrap();
    assert_eq!(
        bad_player.last_create_unit_receipt().unwrap().outcome,
        CreateUnitOutcome::Returned(-1)
    );
    assert_eq!(bad_player.create_unit_numeric_group(0).unwrap().len(), 1);

    let scripts = runtime(one_builtin_program(508, 1, 7), "game_tick");
    let mut transport =
        BhsSession::new_with_create_units(sim(), scripts, factory_input(true), create_input(true))
            .unwrap();
    transport.do_frame().unwrap();
    let receipt = transport.last_create_unit_receipt().unwrap();
    assert_eq!(receipt.route, Some(CreateUnitRoute::RejectAfterGroupPolicy));
    assert_eq!(receipt.outcome, CreateUnitOutcome::Returned(-1));
    assert!(transport.create_unit_numeric_group(0).unwrap().is_empty());
}

#[test]
fn allocation_loop_keeps_partial_successes_and_returns_the_last_attempt() {
    for (answers, expected_published, expected_return) in
        [(vec![41, -1], vec![41], -1), (vec![-1, 42], vec![42], 42)]
    {
        let mut source = answers.clone().into_iter();
        let mut published = Vec::new();
        let (observed, returned) =
            execute_non_atomic_allocations(2, || source.next().unwrap(), |id| published.push(id));
        assert_eq!(observed, answers);
        assert_eq!(published, expected_published);
        assert_eq!(returned, expected_return);
    }
}

#[test]
fn setup_binds_projection_to_the_type_composition_and_corpus_coverage_stays_honest() {
    let scripts = runtime(one_builtin_program(508, 1, 0), "game_tick");
    let mut mismatched = create_input(true);
    mismatched.witness.manifest_sha256 = digest(9);
    assert!(matches!(
        BhsSession::new_with_create_units(sim(), scripts, factory_input(false), mismatched),
        Err(BhsSessionSetupError::CreateUnits(
            CreateUnitRuntimeSetupError::ManifestMismatch
        ))
    ));

    assert_eq!(
        CREATE_UNIT_CENSUS.iter().map(|row| row.calls).sum::<u32>(),
        CREATE_UNIT_COHORT_CALLS
    );
    assert_eq!(CREATE_UNIT_COHORT_CALLS, 5_272);
    assert_eq!(SHIPPED_CORPUS_CALLS, 39_957);
    // The shipped corpus has no zero, negative, >2000, or detectable Transport-Barge call.
    // Until the positive allocation authority lands, full handled-call coverage must stay zero.
    assert_eq!(STATICALLY_COMPLETE_SHIPPED_CALLS, 0);
    assert_eq!(FULLY_HANDLED_SHIPPED_CALL_DELTA, 0);
}
