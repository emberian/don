use don_bhs::disasm::{asm, asm_len};
use don_bhs::{find_builtin, Program, Script, ScriptFile, ScriptTy, Value, VarRef, VmError};
use don_sim::rng::Random;
use don_sim::script_runtime::{
    ScriptBindError, ScriptBinding, ScriptFailure, ScriptOutput, ScriptRuntime, ScriptSlot,
};
use don_sim::tick::{Sim, StepRun};

fn void_script(name: &str, entry: u32) -> Script {
    Script {
        name: name.to_string(),
        entry,
        return_type: ScriptTy::Void.tag(),
        ..Default::default()
    }
}

fn print_order_program() -> Program {
    let mut code = asm(&[
        (0x47, &[0]),
        (0x26, &[VarRef::Const(0).encode()]),
        (0x38, &[find_builtin("print_line").unwrap().index]),
        (0x3e, &[]),
    ]);
    let general_entry = code.len() as u32;
    code.extend_from_slice(&asm(&[
        (0x47, &[1]),
        (0x26, &[VarRef::Const(1).encode()]),
        (0x38, &[find_builtin("print_line").unwrap().index]),
        (0x3e, &[]),
    ]));
    Program::single(ScriptFile {
        code,
        const_pool: vec![Value::str("game"), Value::str("general")],
        scripts: vec![
            void_script("game_tick", 0),
            void_script("general_powers", general_entry),
        ],
        ..Default::default()
    })
}

fn one_builtin_program(name: &str, args: &[Value]) -> Program {
    let builtin = find_builtin(name).unwrap();
    assert_eq!(builtin.arity as usize, args.len());
    let mut instructions: Vec<(u8, Vec<u32>)> = vec![(0x47, vec![0])];
    // Retail call convention: logical arguments are emitted right-to-left.
    for i in (0..args.len()).rev() {
        instructions.push((0x26, vec![VarRef::Const(i as u32).encode()]));
    }
    instructions.push((0x38, vec![builtin.index]));
    if builtin.ret != ScriptTy::Void {
        instructions.push((0x27, Vec::new()));
    }
    instructions.push((0x3e, Vec::new()));
    let borrowed: Vec<(u8, &[u32])> = instructions
        .iter()
        .map(|(op, operands)| (*op, operands.as_slice()))
        .collect();
    Program::single(ScriptFile {
        code: asm(&borrowed),
        const_pool: args.to_vec(),
        scripts: vec![void_script("game_tick", 0)],
        ..Default::default()
    })
}

fn game_runtime(program: Program) -> ScriptRuntime {
    ScriptRuntime::new(program, Some(ScriptBinding::new(0, "game_tick")), None).unwrap()
}

fn static_counter_program() -> Program {
    let static_ref = VarRef::Static(0).encode();
    let guard = [
        (0x47u8, &[0][..]),
        (0x44, &[static_ref, 0][..]),
        (0x26, &[VarRef::Const(0).encode()][..]),
        (0x32, &[static_ref][..]),
    ];
    let body_at = asm_len(&guard) as u32;
    Program::single(ScriptFile {
        code: asm(&[
            (0x47, &[0]),
            (0x44, &[static_ref, body_at]),
            (0x26, &[VarRef::Const(0).encode()]),
            (0x32, &[static_ref]),
            (0x26, &[static_ref]),
            (0x26, &[VarRef::Const(1).encode()]),
            (0x04, &[]),
            (0x33, &[static_ref]),
            (0x3e, &[]),
        ]),
        const_pool: vec![Value::Int(0), Value::Int(1)],
        scripts: vec![Script {
            static_var_names: vec!["counter".to_string()],
            ..void_script("game_tick", 0)
        }],
        ..Default::default()
    })
}

#[test]
fn step_four_runs_both_slots_in_recovered_order_with_real_work() {
    let mut scripts = ScriptRuntime::new(
        print_order_program(),
        Some(ScriptBinding::new(0, "game_tick")),
        Some(ScriptBinding::new(0, "general_powers")),
    )
    .unwrap();
    let mut sim = Sim::new(7, 8);

    // Retail gates general powers on the pre-increment Game::frame being positive.
    let first = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(first.steps[4], StepRun::Executed);
    assert!(
        first.work[4] > 0,
        "a call count without bytecode is vacuous"
    );
    assert_eq!(
        scripts.output(),
        [ScriptOutput {
            text: "game".to_string(),
            newline: true,
        }]
    );
    assert_eq!(sim.world.frame, 1);

    let second = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(second.steps[4], StepRun::Executed);
    assert!(second.work[4] > first.work[4]);
    assert_eq!(
        scripts.output(),
        [
            ScriptOutput {
                text: "game".to_string(),
                newline: true,
            },
            ScriptOutput {
                text: "game".to_string(),
                newline: true,
            },
            ScriptOutput {
                text: "general".to_string(),
                newline: true,
            },
        ]
    );
    assert_eq!(scripts.calls(), 3);
    assert!(scripts.bytecodes() >= (first.work[4] + second.work[4]) as u64);
    assert_eq!(sim.world.frame, 2);
}

#[test]
fn script_statics_are_a_real_cross_frame_state_producer() {
    let mut scripts = game_runtime(static_counter_program());
    let mut sim = Sim::new(17, 8);
    let first = sim.do_frame_with_scripts(&mut scripts).unwrap();
    let second = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert!(first.work[4] > 0 && second.work[4] > 0);
    assert_eq!(
        scripts.program().files[0].scripts[0].statics,
        [Some(Value::Int(2))],
        "step 4 must retain Script::static_vars between frames"
    );
}

#[test]
fn utility_rng_uses_the_simulation_stream() {
    let seed = 0x1234_5678u64;
    let mut expected = Random::new(seed as i32);
    let _ = expected.get(10, 20);

    let mut scripts = game_runtime(one_builtin_program(
        "rand_int",
        &[Value::Int(10), Value::Int(20)],
    ));
    let mut sim = Sim::new(seed, 8);
    let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(trace.steps[4], StepRun::Executed);
    assert_eq!(sim.world.random.state(), expected.state());
}

#[test]
fn unsupported_scenario_builtin_stops_before_the_rest_of_the_tick() {
    let mut scripts = game_runtime(one_builtin_program("num_cities", &[Value::Int(1)]));
    let mut sim = Sim::new(99, 8);
    sim.activate(0);

    let error = sim.do_frame_with_scripts(&mut scripts).unwrap_err();
    assert_eq!(error.slot, ScriptSlot::Game);
    assert!(error.bytecodes_executed > 0);
    assert!(matches!(
        error.failure,
        ScriptFailure::Vm(VmError::UnimplementedBuiltin {
            name: "num_cities",
            ..
        })
    ));
    assert_eq!(sim.world.frame, 0, "step 20 must not run after BHS failure");
    assert_eq!(sim.cover.ticks, 0, "a failed partial tick is not coverage");
    assert_eq!(sim.cover.leader_gathers, 0, "step 8 must not run");
}

#[test]
fn tick_bindings_reject_parameterized_entries_before_execution() {
    let mut parameterized = void_script("needs_arg", 0);
    parameterized.arity = 1;
    parameterized.params = vec![ScriptTy::Int.tag()];
    let program = Program::single(ScriptFile {
        code: asm(&[(0x47, &[0]), (0x3e, &[])]),
        scripts: vec![parameterized],
        ..Default::default()
    });
    let error = match ScriptRuntime::new(program, Some(ScriptBinding::new(0, "needs_arg")), None) {
        Ok(_) => panic!("parameterized step-4 binding was accepted"),
        Err(error) => error,
    };
    assert_eq!(
        error,
        ScriptBindError::NonzeroArity {
            file: 0,
            name: "needs_arg".to_string(),
            arity: 1,
        }
    );
}
