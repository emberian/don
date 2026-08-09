use don_bhs::disasm::asm;
use don_bhs::{builtin, BuiltinDecl, Program, Script, ScriptFile, ScriptTy, Value, VarRef};
use don_sim::script_runtime::{ScriptBinding, ScriptRuntime};
use don_sim::systems::bhs_type_runtime::{
    TypeBuiltinBoundaryError, TypeBuiltinOutcome, TypeBuiltinRuntime, TypeBuiltinRuntimeError,
};
use don_sim::systems::bhs_type_table::*;
use don_sim::systems::save_load::{save_sim_with_scripts, SaveError};
use don_sim::tick::Sim;

fn fixture() -> TypeBuiltinState {
    let mut rows: Vec<_> = (0..NUM_TYPES).map(TypeRow::empty).collect();
    rows[50].name = "Citizen".into();
    rows[50].type_name = "CitizenFamily".into();
    rows[50].common.job_time = 50;
    rows[50].common.tribe_mask = 0;
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
    let mut tribes: Vec<_> = (0..NUM_TRIBES)
        .map(|index| format!("Tribe {index}"))
        .collect();
    tribes[3] = "Romans".into();
    TypeBuiltinState::new(
        types,
        TribeRoster::new(tribes).unwrap(),
        std::array::from_fn(|_| LeaderTypeMasks::default()),
    )
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

#[test]
fn live_vm_routes_signed_minus_one_return_and_issues_a_mutation_receipt() {
    let program = one_builtin_program(290, vec![Value::str("Citizen"), Value::Int(-1)]);
    let mut scripts =
        ScriptRuntime::new(program, Some(ScriptBinding::new(0, "game_tick")), None).unwrap();
    scripts.install_type_builtins(fixture()).unwrap();
    let mut sim = Sim::new(0x284, 8);

    sim.do_frame_with_scripts(&mut scripts).unwrap();

    let receipt = scripts.last_type_builtin_receipt().unwrap();
    assert_eq!(receipt.index, 290);
    assert_eq!(receipt.outcome, TypeBuiltinOutcome::Returned(-1));
    assert!(receipt.mutated());
    assert_eq!(receipt.revision_after, 1);
    assert_eq!(
        scripts
            .type_builtins()
            .unwrap()
            .state()
            .types
            .row(50)
            .common
            .job_time,
        1
    );
}

#[test]
fn five_argument_typed_tail_failure_receipts_the_already_committed_prefix() {
    let mut runtime = TypeBuiltinRuntime::new(fixture());
    let decl = builtin(817).unwrap();
    let error = runtime
        .dispatch(
            decl,
            &[
                Value::str("Citizen"),
                Value::str("Romans"),
                Value::str("非 ASCII building"),
                Value::Int(9),
                Value::Int(7),
            ],
        )
        .unwrap_err();

    assert_eq!(
        error,
        TypeBuiltinRuntimeError::Owner(TypeTableError::NonAsciiRetailName)
    );
    let receipt = runtime.last_receipt().unwrap();
    assert_eq!(
        receipt.outcome,
        TypeBuiltinOutcome::OwnerFault(TypeTableError::NonAsciiRetailName)
    );
    assert!(receipt.mutated());
    assert_eq!(receipt.revision_after, 1);
    assert_eq!(
        runtime.state().types.row(50).common.tribe_mask & (1 << 3),
        1 << 3
    );
    assert_eq!(runtime.state().types.row(50).modified, 1);
}

#[test]
fn declaration_and_argument_admission_are_exact_and_non_mutating() {
    let mut runtime = TypeBuiltinRuntime::new(fixture());
    let shipped = builtin(284).unwrap();
    let wrong_generation = BuiltinDecl {
        handler_va: shipped.handler_va + 1,
        ..*shipped
    };
    assert_eq!(
        runtime.dispatch(&wrong_generation, &[Value::str("Citizen")]),
        Err(TypeBuiltinRuntimeError::DeclarationMismatch { index: 284 })
    );
    assert_eq!(
        runtime.dispatch(shipped, &[Value::Int(50)]),
        Err(TypeBuiltinRuntimeError::BadArguments { index: 284 })
    );
    assert_eq!(runtime.state().mutation_revision(), 0);
    assert!(!runtime.state().is_dirty());
}

#[test]
fn save_and_checksum_boundaries_refuse_omitted_live_type_state() {
    let program = one_builtin_program(284, vec![Value::str("Citizen")]);
    let mut scripts =
        ScriptRuntime::new(program, Some(ScriptBinding::new(0, "game_tick")), None).unwrap();
    scripts.install_type_builtins(fixture()).unwrap();
    let mut sim = Sim::new(0x815, 8);

    assert_eq!(
        save_sim_with_scripts(&sim, &scripts),
        Err(SaveError::BhsTypes(
            TypeBuiltinBoundaryError::SaveOwnerUnowned {
                mutation_revision: 0,
                dirty: false
            }
        ))
    );
    assert_eq!(
        scripts.admitted_sim_channel_digest(&sim),
        Err(TypeBuiltinBoundaryError::Channel13ProjectionUnowned)
    );

    sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(
        save_sim_with_scripts(&sim, &scripts),
        Err(SaveError::BhsTypes(
            TypeBuiltinBoundaryError::SaveOwnerUnowned {
                mutation_revision: 1,
                dirty: true
            }
        ))
    );

    let rejected_second_owner = fixture();
    assert!(scripts
        .install_type_builtins(rejected_second_owner)
        .is_err());
    assert_eq!(
        scripts.type_builtins().unwrap().state().mutation_revision(),
        1
    );
}
