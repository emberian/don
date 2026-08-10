use don_bhs::disasm::asm;
use don_bhs::{builtin, Program, Script, ScriptFile, ScriptTy, Value, VarRef};
use don_sim::bhs_session::{BhsSession, BhsSessionSetupError};
use don_sim::script_runtime::{ScriptBinding, ScriptRuntime};
use don_sim::systems::bhs_type_factory::*;
use don_sim::systems::bhs_type_runtime::TypeBuiltinBoundaryError;
use don_sim::systems::bhs_type_table::*;
use don_sim::systems::save_load::{save_sim, SaveError};
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
    let rows = (0..NUM_TYPES)
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
                .map(|_| Some(LeaderTypeMasks::default()))
                .collect(),
        },
    }
}

fn one_builtin_program(index: u32, args: Vec<Value>) -> Program {
    let decl = builtin(index).unwrap();
    assert_eq!(decl.arity as usize, args.len());
    let mut instructions: Vec<(u8, Vec<u32>)> = vec![(0x47, vec![0])];
    for slot in (0..args.len()).rev() {
        instructions.push((0x26, vec![VarRef::Const(slot as u32).encode()]));
    }
    instructions.push((0x38, vec![index]));
    instructions.push((0x27, Vec::new()));
    instructions.push((0x3e, Vec::new()));
    let borrowed: Vec<_> = instructions
        .iter()
        .map(|(opcode, operands)| (*opcode, operands.as_slice()))
        .collect();
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

fn script_runtime() -> ScriptRuntime {
    ScriptRuntime::new(
        one_builtin_program(290, vec![Value::str("Citizen"), Value::Int(-1)]),
        Some(ScriptBinding::new(0, "game_tick")),
        None,
    )
    .unwrap()
}

#[test]
fn setup_installs_provenance_before_first_frame_and_all_session_boundaries_stay_admitted() {
    let sim = Sim::new(0x815, 8);
    let mut session = BhsSession::new(sim, script_runtime(), factory_input()).unwrap();

    assert_eq!(
        session.type_provenance().composition,
        RulesCompositionId(digest(1))
    );
    assert_eq!(session.status().frame, 0);
    assert_eq!(session.status().type_mutation_revision, 0);
    assert!(!session.status().type_state_dirty);
    assert_eq!(
        session.save(),
        Err(SaveError::BhsTypes(
            TypeBuiltinBoundaryError::SaveOwnerUnowned {
                mutation_revision: 0,
                dirty: false,
            }
        ))
    );
    assert_eq!(
        session.partial_channel_digest(),
        Err(TypeBuiltinBoundaryError::Channel13ProjectionUnowned)
    );

    session.do_frame().unwrap();

    assert_eq!(session.status().frame, 1);
    assert_eq!(session.status().script_calls, 1);
    assert_eq!(session.status().type_mutation_revision, 1);
    assert!(session.status().type_state_dirty);
    assert_eq!(session.type_state().types.row(50).common.job_time, 1);
    assert_eq!(session.last_type_builtin_receipt().unwrap().index, 290);
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
fn setup_rejects_a_state_only_runtime_instead_of_pairing_it_with_new_provenance() {
    let produced = produce_type_builtin_state(factory_input()).unwrap();
    let (state, _) = produced.into_parts();
    let mut scripts = script_runtime();
    scripts.install_type_builtins(state).unwrap();

    assert!(matches!(
        BhsSession::new(Sim::new(1, 8), scripts, factory_input()),
        Err(BhsSessionSetupError::PreinstalledTypeOwner)
    ));
}

#[test]
fn setup_rejects_even_a_failed_prior_script_frame() {
    let mut sim = Sim::new(0x290, 8);
    let mut scripts = script_runtime();
    assert!(sim.do_frame_with_scripts(&mut scripts).is_err());
    assert!(scripts.has_started());

    assert!(matches!(
        BhsSession::new(sim, scripts, factory_input()),
        Err(BhsSessionSetupError::ScriptRuntimeAlreadyStarted)
    ));
}

#[test]
fn setup_runs_factory_admission_before_exposing_a_session() {
    let mut input = factory_input();
    input.leaders.witness.manifest_sha256 = digest(9);
    assert!(matches!(
        BhsSession::new(Sim::new(2, 8), script_runtime(), input),
        Err(BhsSessionSetupError::Factory(
            TypeBuiltinFactoryError::MixedManifest {
                role: TypeSourceRole::LeaderMasks,
            }
        ))
    ));
}

#[test]
fn consuming_sim_prevents_post_install_legacy_save_and_digest_handles() {
    let sim = Sim::new(3, 8);
    assert!(save_sim(&sim).is_ok());
    let _unadmitted_legacy_digest = sim.channel_digest();
    let session = BhsSession::new(sim, script_runtime(), factory_input()).unwrap();

    // BhsSession deliberately provides no Sim accessor, Deref, Clone, or into_parts path.
    // After the unique Sim is consumed, save and digest both have only admitted red boundaries.
    assert!(matches!(session.save(), Err(SaveError::BhsTypes(_))));
    assert_eq!(
        session.partial_channel_digest(),
        Err(TypeBuiltinBoundaryError::Channel13ProjectionUnowned)
    );
}
