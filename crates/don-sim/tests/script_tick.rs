use don_bhs::chunk::load_program;
use don_bhs::disasm::{asm, asm_len};
use don_bhs::{find_builtin, Program, Script, ScriptFile, ScriptTy, Value, VarRef, VmError};
use don_bhs_cc::sema::{self, Severity};
use don_sim::rng::Random;
use don_sim::script_runtime::{
    ScriptBindError, ScriptBinding, ScriptFailure, ScriptOutput, ScriptRuntime, ScriptSlot,
};
use don_sim::tick::{Sim, StepRun};
use std::path::Path;

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

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_chunk_string(out: &mut Vec<u8>, value: &str) {
    let units = value.encode_utf16().collect::<Vec<_>>();
    push_u32(out, units.len() as u32);
    for unit in units {
        out.extend_from_slice(&unit.to_le_bytes());
    }
}

fn chunk(tag: u16, payload: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + payload.len());
    push_u32(&mut out, (8 + payload.len()) as u32);
    out.extend_from_slice(&tag.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&payload);
    out
}

fn loaded_one_builtin_program(name: &str, args: &[Value]) -> Program {
    let compiled = one_builtin_program(name, args);
    let file = &compiled.files[0];

    let mut script = Vec::new();
    push_u32(&mut script, 0);
    push_chunk_string(&mut script, "game_tick");
    push_u32(&mut script, 0); // entry
    push_u32(&mut script, 0); // script_type
    push_u32(&mut script, ScriptTy::Void.tag());
    push_u32(&mut script, 0); // trigger_count
    push_u32(&mut script, 0); // param_count

    let mut children = vec![chunk(2, script)];
    for value in &file.const_pool {
        let mut payload = Vec::new();
        match value {
            Value::Int(value) => {
                push_u32(&mut payload, ScriptTy::Int.tag());
                push_u32(&mut payload, *value as u32);
            }
            Value::Str(value) => {
                push_u32(&mut payload, ScriptTy::Str.tag());
                push_chunk_string(&mut payload, value);
            }
            _ => panic!("loaded fixture uses only shipped scalar chunk constants"),
        }
        children.push(chunk(3, payload));
    }
    children.push(chunk(4, file.code.clone()));

    let size = 8 + children.iter().map(Vec::len).sum::<usize>();
    let mut root = Vec::with_capacity(size);
    push_u32(&mut root, size as u32);
    root.extend_from_slice(&0u16.to_le_bytes());
    root.extend_from_slice(&(children.len() as u16).to_le_bytes());
    for child in children {
        root.extend_from_slice(&child);
    }
    load_program(&root, "loaded_scenario_runtime.bhs").unwrap()
}

fn game_runtime(program: Program) -> ScriptRuntime {
    ScriptRuntime::new(program, Some(ScriptBinding::new(0, "game_tick")), None).unwrap()
}

fn compile_source_fixture(name: &str) -> Program {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
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
fn ordinary_source_runtime_mutates_world_and_persists_retail_timers() {
    let program = compile_source_fixture("scenario_runtime.bhs");
    let mut scripts =
        ScriptRuntime::new(program, Some(ScriptBinding::new(0, "scenario_tick")), None).unwrap();
    let mut sim = Sim::new(0x1234, 8);
    sim.activate(0);
    sim.activate(1);
    sim.leaders[0].econ.age_alt = 4;
    // The first queried tile has retail's water mask and the second remains land.
    // A constant map answer changes the asserted Oil stockpile in either direction.
    sim.map.world.tdata[0] = 0x20;
    // `is_defeated` reads bit 6 from the exact Leader flags byte.
    sim.step8.leaders[0].flags |= 1 << 6;

    let first = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(first.steps[4], StepRun::Executed);
    assert!(first.work[4] > 0, "source-compiled script did no VM work");
    assert_eq!(
        sim.leaders[0].econ.stockpile,
        [15, 32, 2, 4, 101, 2],
        "map, player, leader, clock, timer, and resource handlers all feed live state"
    );
    assert_eq!(sim.leaders[0].gather_ctx.extra_income[2], 3 << 4);

    // The timer was set at Game::seconds 0 for one second. Step 23 reaches second 1
    // only after frame 14; the next step-4 call observes and consumes the timer.
    sim.run_with_scripts(&mut scripts, 14).unwrap();
    assert_eq!(sim.world.seconds, 1);
    assert_eq!(sim.leaders[0].econ.stockpile[0], 15);

    let expiry = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(expiry.steps[4], StepRun::Executed);
    assert_eq!(sim.leaders[0].econ.stockpile[0], 22);

    // `ScriptTimers::check` removes an expired entry. The following absent check
    // returns -1, so the source branch does not award the resource twice.
    sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(sim.leaders[0].econ.stockpile[0], 22);
    assert_eq!(
        scripts.program().files[0].scripts[0].statics,
        [Some(Value::Int(2))]
    );
}

#[test]
fn retail_chunk_loader_program_uses_the_same_mandatory_world_host() {
    let program = loaded_one_builtin_program(
        "give_good",
        &[Value::Int(1), Value::str("Food"), Value::Int(9)],
    );
    assert!(
        program.walk_meta().is_some(),
        "test must execute the normal chunk-loader producer, not a hand-built Program"
    );
    let mut scripts = game_runtime(program);
    let mut sim = Sim::new(0x5678, 8);
    sim.activate(0);

    let trace = sim.do_frame_with_scripts(&mut scripts).unwrap();
    assert_eq!(trace.steps[4], StepRun::Executed);
    assert!(trace.work[4] > 0);
    assert_eq!(sim.leaders[0].econ.stockpile[0], 9);
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
