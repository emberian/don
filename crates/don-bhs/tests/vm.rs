//! VM behaviour tests.
//!
//! These test *our* implementation against the semantics recovered from the binary.
//! They are **not** evidence of fidelity to retail — that requires the differential
//! harness described in `docs/tracks/bhs-engine.md`. A green run here means the
//! crate does what we believe the engine does, not that the belief is right.

use don_bhs::builtins::UtilHost;
use don_bhs::disasm::{asm, asm_len, disassemble};
use don_bhs::host::{Coverage, Host, HostError, HostResult, NullHost};
use don_bhs::program::{
    Program, ProgramWalkMeta, Script, ScriptFile, ScriptFileWalkMeta, ScriptWalkMeta, ValueWalkMeta,
};
use don_bhs::value::Value;
use don_bhs::vm::{MissingBuiltinPolicy, VarRef, Vm, VmError};
use don_bhs::{builtin, find_builtin, BuiltinDecl, BUILTIN_COUNT};

fn prog(code: Vec<u8>, consts: Vec<Value>, statics: usize) -> Program {
    Program::single(ScriptFile {
        code,
        const_pool: consts,
        scripts: vec![Script {
            name: "tick".into(),
            statics: vec![None; statics],
            trigger_bits: vec![0u8; 4],
            trigger_count: 31,
            ..Default::default()
        }],
        ..Default::default()
    })
}

fn run_once(p: &mut Program) -> Option<Value> {
    let mut host = NullHost;
    let mut vm = Vm::new(p, &mut host);
    vm.run_script(0, "tick").unwrap().returned
}

#[test]
fn arithmetic_and_return() {
    // return 6 * 7;
    let code = asm(&[
        (0x26, &[VarRef::Const(0).encode()]),
        (0x26, &[VarRef::Const(1).encode()]),
        (0x0f, &[]), // OP_MUL
        (0x3e, &[]), // OP_RETURN
    ]);
    let mut p = prog(code, vec![Value::Int(6), Value::Int(7)], 0);
    assert_eq!(run_once(&mut p), Some(Value::Int(42)));
}

#[test]
fn static_growth_mirrors_retail_scalar_ownership_metadata() {
    // The shipped static_int compiler capture: const[0], OP_INIT_COPY static[0].
    // The source constant remains VM_CONST (2/0); its duplicate is promoted by
    // set_value to VM_VAR (3/0).
    let mut p = Program::single(ScriptFile {
        code: decode_hex("47000000004400000040180000002600000020320000004028ad7b05003e"),
        const_pool: vec![Value::Int(1)],
        scripts: vec![Script {
            name: "static_int".into(),
            return_type: don_bhs::ScriptTy::Int.tag(),
            static_var_names: vec!["value".into()],
            ..Default::default()
        }],
        ..Default::default()
    })
    .with_walk_meta(ProgramWalkMeta {
        files: vec![ScriptFileWalkMeta {
            script_meta: vec![ScriptWalkMeta::default()],
            const_pool: vec![Some(ValueWalkMeta::scalar(2, 0))],
            ..Default::default()
        }],
    });

    let mut host = NullHost;
    assert_eq!(
        Vm::new(&mut p, &mut host)
            .run_script(0, "static_int")
            .unwrap()
            .returned,
        Some(Value::Int(0))
    );
    assert_eq!(p.files[0].scripts[0].statics, [Some(Value::Int(1))]);
    assert_eq!(
        p.walk_meta().unwrap().files[0].script_meta[0].statics,
        [Some(ValueWalkMeta::scalar(3, 0))]
    );
    assert_eq!(
        p.walk_meta().unwrap().files[0].const_pool,
        [Some(ValueWalkMeta::scalar(2, 0))]
    );
}

#[test]
fn direct_alias_static_install_invalidates_erased_ownership_metadata() {
    // OP_INIT (not COPY) receives a const-pool pointer here. Retail set_value changes
    // that same object's scope, so a Rust value clone cannot update both aliases
    // exactly. The live static grows, but its sidecar slot must remain absent.
    let code = asm(&[
        (0x26, &[VarRef::Const(0).encode()]),
        (0x33, &[VarRef::Static(2).encode()]),
        (0x28, &[don_bhs::ScriptTy::Int.tag()]),
        (0x3e, &[]),
    ]);
    let mut p = Program::single(ScriptFile {
        code,
        const_pool: vec![Value::Int(7)],
        scripts: vec![Script {
            name: "tick".into(),
            return_type: don_bhs::ScriptTy::Int.tag(),
            ..Default::default()
        }],
        ..Default::default()
    })
    .with_walk_meta(ProgramWalkMeta {
        files: vec![ScriptFileWalkMeta {
            script_meta: vec![ScriptWalkMeta::default()],
            const_pool: vec![Some(ValueWalkMeta::scalar(2, 0))],
            ..Default::default()
        }],
    });

    assert_eq!(run_once(&mut p), Some(Value::Int(0)));
    assert_eq!(
        p.files[0].scripts[0].statics,
        [None, None, Some(Value::Int(7))]
    );
    assert_eq!(
        p.walk_meta().unwrap().files[0].script_meta[0].statics,
        [None, None, None]
    );
}

fn decode_hex(s: &str) -> Vec<u8> {
    assert_eq!(s.len() % 2, 0);
    s.as_bytes()
        .chunks_exact(2)
        .map(|p| {
            let d = |b: u8| match b {
                b'0'..=b'9' => b - b'0',
                b'a'..=b'f' => b - b'a' + 10,
                _ => panic!("non-hex fixture byte"),
            };
            d(p[0]) << 4 | d(p[1])
        })
        .collect()
}

/// Exact pointer-free output captured from the supported shipped compiler
/// (`Compiler::compile`, 0x009bf160) on hbox. The two sources are identical except
/// for `ref`; their bytecode differs at exactly offset 5: OP_INIT (pointer alias)
/// versus OP_INIT_COPY (duplicate). This is the ownership transition the VM must
/// preserve, not a hand-selected calling convention.
fn retail_param_program(by_ref: bool) -> Program {
    let code_hex = if by_ref {
        "4700000000330000000026000000002600000020042600000000002726000000003e\
         470100000026010000203200000000260000000036000000002726000000003e"
    } else {
        "4700000000320000000026000000002600000020042600000000002726000000003e\
         470100000026010000203200000000260000000036000000002726000000003e"
    };
    let callee_name = if by_ref {
        "increment"
    } else {
        "increment_copy"
    };
    let entry_name = if by_ref { "ref_alias" } else { "value_param" };
    Program::single(ScriptFile {
        code: decode_hex(&code_hex.replace(' ', "")),
        const_pool: vec![Value::Int(1), Value::Int(41)],
        scripts: vec![
            Script {
                name: callee_name.into(),
                entry: 0,
                arity: 1,
                params: vec![don_bhs::ScriptTy::Int.tag()],
                refs: vec![u8::from(by_ref)],
                return_type: don_bhs::ScriptTy::Int.tag(),
                var_names: vec!["value".into()],
                ..Default::default()
            },
            Script {
                name: entry_name.into(),
                entry: 34,
                return_type: don_bhs::ScriptTy::Int.tag(),
                var_names: vec!["value".into()],
                ..Default::default()
            },
        ],
        ..Default::default()
    })
}

/// Exact pointer-free output captured from the shipped compiler for a four-parameter
/// script with its second parameter passed by reference. The caller emits locals
/// 3,2,1,0; the callee binds COPY 0, REF 1, COPY 2, COPY 3.
fn retail_mixed_param_program() -> Program {
    let code = decode_hex(
        "470000000032000000003301000000320200000032030000002601000000260000000004\
         2602000000042603000000042601000000002726010000003e47010000002600000020\
         3200000000260100002032010000002602000020320200000026030000203203000000\
         260300000026020000002601000000260000000036000000002726010000003e"
            .replace(' ', "")
            .as_str(),
    );
    Program::single(ScriptFile {
        code,
        const_pool: vec![
            Value::Int(1),
            Value::Int(10),
            Value::Int(100),
            Value::Int(1000),
        ],
        scripts: vec![
            Script {
                name: "accumulate".into(),
                entry: 0,
                arity: 4,
                params: vec![don_bhs::ScriptTy::Int.tag(); 4],
                refs: vec![0, 1, 0, 0],
                return_type: don_bhs::ScriptTy::Int.tag(),
                var_names: vec!["first", "total", "third", "fourth"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                ..Default::default()
            },
            Script {
                name: "mixed_params".into(),
                entry: 61,
                return_type: don_bhs::ScriptTy::Int.tag(),
                var_names: vec!["first", "total", "third", "fourth"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                ..Default::default()
            },
        ],
        ..Default::default()
    })
}

fn retail_builtin_args_program() -> Program {
    Program::single(ScriptFile {
        code: decode_hex("47000000002601000020260000002038130000003e"),
        const_pool: vec![Value::str("abc"), Value::Int(1)],
        scripts: vec![Script {
            name: "builtin_args".into(),
            entry: 0,
            return_type: don_bhs::ScriptTy::Int.tag(),
            ..Default::default()
        }],
        ..Default::default()
    })
}

#[test]
fn shipped_compiler_ref_prologue_mutates_the_callers_scalar() {
    // Fixture SHA-256 cf4fb7fb00c03be5cdefa98349465a47a3ec5cda6a891f2700c97d9254fea08f.
    assert_eq!(
        include_str!("fixtures/ref_alias.bhs"),
        concat!(
            "int scenario increment(ref int value)\n",
            "{\n",
            "    value = value + 1;\n",
            "    return value;\n",
            "}\n",
            "\n",
            "int scenario ref_alias()\n",
            "{\n",
            "    int value = 41;\n",
            "    increment(value);\n",
            "    return value;\n",
            "}\n",
        )
    );
    let mut p = retail_param_program(true);
    let mut host = NullHost;
    let mut vm = Vm::new(&mut p, &mut host);
    let out = vm.run_script(0, "ref_alias").unwrap();
    assert_eq!(out.returned, Some(Value::Int(42)));
    assert!(out.error.is_none());

    // The same measured callee through the external script boundary must write
    // the ref argument back to the C++/simulation-side owner, which is how the
    // shipped economic/defensive scripts advance `ref int step`.
    let mut p = retail_param_program(true);
    let mut host = NullHost;
    let mut vm = Vm::new(&mut p, &mut host);
    let mut args = [Value::Int(9)];
    let out = vm.run_script_index_mut(0, 0, &mut args).unwrap();
    assert_eq!(out.returned, Some(Value::Int(10)));
    assert_eq!(args, [Value::Int(10)]);
}

#[test]
fn shipped_compiler_value_prologue_copies_and_the_mutation_gate_bites() {
    // Fixture SHA-256 e2ed9f9830421c02c8fa3a61f65e2ad309d0cb0a9a00f97bfac2487a7acf69b1.
    assert_eq!(
        include_str!("fixtures/value_param.bhs"),
        concat!(
            "int scenario increment_copy(int value)\n",
            "{\n",
            "    value = value + 1;\n",
            "    return value;\n",
            "}\n",
            "\n",
            "int scenario value_param()\n",
            "{\n",
            "    int value = 41;\n",
            "    increment_copy(value);\n",
            "    return value;\n",
            "}\n",
        )
    );
    let mut by_value = retail_param_program(false);
    let mut host = NullHost;
    let out = Vm::new(&mut by_value, &mut host)
        .run_script(0, "value_param")
        .unwrap();
    assert_eq!(out.returned, Some(Value::Int(41)));

    let mut by_value = retail_param_program(false);
    let mut host = NullHost;
    let mut vm = Vm::new(&mut by_value, &mut host);
    let mut args = [Value::Int(9)];
    let out = vm.run_script_index_mut(0, 0, &mut args).unwrap();
    assert_eq!(out.returned, Some(Value::Int(10)));
    assert_eq!(args, [Value::Int(9)], "by-value must not write back");

    // One-byte mutant: replacing the captured ref OP_INIT with OP_INIT_COPY must
    // stop the callee's write from reaching the caller. If alias propagation is
    // vacuous, this assertion cannot distinguish the implementations.
    let mut mutant = retail_param_program(true);
    assert_eq!(mutant.files[0].code[5], 0x33);
    mutant.files[0].code[5] = 0x32;
    let mut host = NullHost;
    let out = Vm::new(&mut mutant, &mut host)
        .run_script(0, "ref_alias")
        .unwrap();
    assert_eq!(out.returned, Some(Value::Int(41)));
}

#[test]
fn shipped_compiler_mixed_parameters_bind_forward_and_write_back_only_ref() {
    // Fixture SHA-256 b10d7f9f9dffa9c610d0db5da9e00db347aad8e1617b9923cc386c4dfb707e66.
    assert_eq!(
        include_str!("fixtures/mixed_params.bhs"),
        concat!(
            "int scenario accumulate(int first, ref int total, int third, int fourth)\n",
            "{\n",
            "    total = total + first + third + fourth;\n",
            "    return total;\n",
            "}\n",
            "\n",
            "int scenario mixed_params()\n",
            "{\n",
            "    int first = 1;\n",
            "    int total = 10;\n",
            "    int third = 100;\n",
            "    int fourth = 1000;\n",
            "    accumulate(first, total, third, fourth);\n",
            "    return total;\n",
            "}\n",
        )
    );

    // Internal script call: the returned value is discarded, so 1111 proves the
    // second local was actually aliased back into the caller.
    let mut p = retail_mixed_param_program();
    let mut host = NullHost;
    let out = Vm::new(&mut p, &mut host)
        .run_script(0, "mixed_params")
        .unwrap();
    assert_eq!(out.returned, Some(Value::Int(1111)));

    // External boundary: API arguments arrive in logical order, are stacked in
    // retail order, and only the measured `ref` slot writes through.
    let mut p = retail_mixed_param_program();
    let mut host = NullHost;
    let mut args = [
        Value::Int(1),
        Value::Int(10),
        Value::Int(100),
        Value::Int(1000),
    ];
    let out = Vm::new(&mut p, &mut host)
        .run_script_index_mut(0, 0, &mut args)
        .unwrap();
    assert_eq!(out.returned, Some(Value::Int(1111)));
    assert_eq!(
        args,
        [
            Value::Int(1),
            Value::Int(1111),
            Value::Int(100),
            Value::Int(1000),
        ]
    );
}

#[test]
fn shipped_compiler_builtin_arguments_are_consumed_top_first() {
    // Fixture SHA-256 ffab74ced8626eda196a2d944444ebc1d79958238be9ad417118fc431bcafa66.
    assert_eq!(
        include_str!("fixtures/builtin_args.bhs"),
        concat!(
            "int scenario builtin_args()\n",
            "{\n",
            "    return char_at(\"abc\", 1);\n",
            "}\n",
        )
    );
    let mut p = retail_builtin_args_program();
    let mut host = UtilHost::default();
    let out = Vm::new(&mut p, &mut host)
        .run_script(0, "builtin_args")
        .unwrap();
    // Reversing the popped arguments (the previous VM behavior) returns '1' (49),
    // so this is mutation-sensitive to the exact native-call stack convention.
    assert_eq!(out.returned, Some(Value::Int('b' as i32)));
}

/// The defining property of a per-frame script: `Game::do_frame` calls it once per
/// frame with zero arguments, so all memory is in `Script::static_vars`, guarded by
/// `OP_JUMP_IF_INITED`.
#[test]
fn statics_persist_across_invocations() {
    let init = [
        (0x44u8, &[VarRef::Static(0).encode(), 0][..]), // patched below
        (0x26, &[VarRef::Const(0).encode()][..]),
        (0x33, &[VarRef::Static(0).encode()][..]),
    ];
    let after_init = asm_len(&init) as u32;
    let code = asm(&[
        (0x44, &[VarRef::Static(0).encode(), after_init]),
        (0x26, &[VarRef::Const(0).encode()]),
        (0x33, &[VarRef::Static(0).encode()]),
        // n = n + 1
        (0x26, &[VarRef::Static(0).encode()]),
        (0x26, &[VarRef::Const(1).encode()]),
        (0x04, &[]),
        (0x33, &[VarRef::Static(0).encode()]),
        (0x26, &[VarRef::Static(0).encode()]),
        (0x3e, &[]),
    ]);
    let mut p = prog(code, vec![Value::Int(100), Value::Int(1)], 1);
    assert_eq!(run_once(&mut p), Some(Value::Int(101)));
    assert_eq!(run_once(&mut p), Some(Value::Int(102)));
    assert_eq!(run_once(&mut p), Some(Value::Int(103)));
    // The initialiser ran exactly once.
    assert_eq!(p.files[0].scripts[0].statics[0], Some(Value::Int(103)));
}

#[test]
fn conditional_jumps_take_the_measured_polarity() {
    // if (0) return 1; else return 2;   via OP_JUMP_IF_NOT
    let head = asm(&[(0x26, &[VarRef::Const(0).encode()]), (0x41, &[0])]);
    let then_part = asm(&[(0x26, &[VarRef::Const(1).encode()]), (0x3e, &[])]);
    let else_at = (head.len() + then_part.len()) as u32;
    let mut code = asm(&[(0x26, &[VarRef::Const(0).encode()]), (0x41, &[else_at])]);
    code.extend_from_slice(&then_part);
    code.extend_from_slice(&asm(&[(0x26, &[VarRef::Const(2).encode()]), (0x3e, &[])]));
    let mut p = prog(code, vec![Value::Int(0), Value::Int(1), Value::Int(2)], 0);
    // condition is false -> OP_JUMP_IF_NOT jumps -> else branch
    assert_eq!(run_once(&mut p), Some(Value::Int(2)));
}

/// `OP_BIT_SET` uses `btr` and `OP_BIT_UNSET` uses `bts` — the enum names are
/// inverted relative to the machine. This test locks in the machine behaviour, so
/// that "fixing" it to match the names breaks a test.
#[test]
fn trigger_bit_opcodes_follow_the_machine_not_the_enum_names() {
    let code = asm(&[
        (0x3d, &[3]), // OP_BIT_UNSET -> bts -> SETS bit 3
        (0x3c, &[5]), // OP_BIT_SET   -> btr -> CLEARS bit 5
        (0x3e, &[]),
    ]);
    let mut p = prog(code, vec![], 0);
    p.files[0].scripts[0].trigger_bits = vec![0xff, 0, 0, 0];
    p.files[0].scripts[0].return_type = don_bhs::ScriptTy::Void.tag();
    let mut host = NullHost;
    let mut vm = Vm::new(&mut p, &mut host);
    vm.run_script(0, "tick").unwrap();
    let s = &p.files[0].scripts[0];
    assert!(s.is_trigger_enabled(3), "OP_BIT_UNSET must SET the bit");
    assert!(!s.is_trigger_enabled(5), "OP_BIT_SET must CLEAR the bit");
}

/// A host that answers exactly one builtin, so we can see both halves of coverage.
struct PartialHost;
impl Host for PartialHost {
    fn call(&mut self, decl: &BuiltinDecl, _args: &[Value]) -> HostResult {
        match decl.name {
            "num_cities" => Ok(Value::Int(3)),
            _ => Err(HostError::Unimplemented),
        }
    }
}

fn missing_builtin_program() -> (Program, &'static BuiltinDecl, &'static BuiltinDecl) {
    let num_cities = find_builtin("num_cities").unwrap();
    let population = find_builtin("population").unwrap();
    let code = asm(&[
        (0x26, &[VarRef::Const(0).encode()]), // who
        (0x38, &[num_cities.index]),
        (0x27, &[]),                          // discard
        (0x26, &[VarRef::Const(0).encode()]), // who
        (0x38, &[population.index]),
        (0x3e, &[]),
    ]);
    (prog(code, vec![Value::Int(1)], 0), num_cities, population)
}

#[test]
fn unimplemented_builtins_fail_by_default_and_are_recorded() {
    let (mut p, num_cities, population) = missing_builtin_program();
    let mut host = PartialHost;
    let mut vm = Vm::new(&mut p, &mut host);
    assert_eq!(
        vm.run_script(0, "tick"),
        Err(VmError::UnimplementedBuiltin {
            index: population.index,
            name: "population",
        })
    );
    assert_eq!(
        vm.coverage.unimplemented().collect::<Vec<_>>(),
        vec![(population.index, "population", 1)]
    );
    assert_eq!(
        vm.coverage.implemented().collect::<Vec<_>>(),
        vec![(num_cities.index, 1)]
    );
}

#[test]
fn coverage_survey_is_explicitly_lossy() {
    let (mut p, num_cities, _population) = missing_builtin_program();
    let mut host = PartialHost;
    let mut vm =
        Vm::new(&mut p, &mut host).with_missing_builtin_policy(MissingBuiltinPolicy::Survey);
    let out = vm.run_script(0, "tick").unwrap();
    // This is ScriptFuncSet::get_err_return, but continuing is a survey device,
    // not a retail-faithful response to an incomplete DoN host.
    assert_eq!(out.returned, Some(Value::Int(-1)));
    let missing: Vec<_> = vm
        .coverage
        .unimplemented()
        .map(|(_, n, c)| (n, c))
        .collect();
    assert_eq!(missing, vec![("population", 1)]);
    let done: Vec<_> = vm.coverage.implemented().collect();
    assert_eq!(done, vec![(num_cities.index, 1)]);
    assert!(!vm.coverage.is_complete());
    assert!(vm.coverage.report().contains("population"));
}

#[test]
fn bad_opcode_is_rejected_at_the_engines_bound() {
    // execute_next does `cmp edi, 0x47 / ja -> run_time_error`.
    let mut p = prog(vec![0x48], vec![], 0);
    let mut host = NullHost;
    let mut vm = Vm::new(&mut p, &mut host);
    assert_eq!(vm.run_script(0, "tick"), Err(VmError::BadOpcode(0x48)));
}

#[test]
fn builtin_table_matches_the_binary() {
    assert_eq!(
        BUILTIN_COUNT, 873,
        "873 add_new_func sites across five FuncSets"
    );
    // Registration order is Math, Trigger, String, Array, Scenario — the order
    // ScriptGameInterface::init (0x009e1a20) constructs them.
    assert_eq!(builtin(0).unwrap().name, "sin");
    assert_eq!(builtin(872).unwrap().name, "clear_attrition_free_points");
    // The two functions that turn the game from an observatory into an experiment,
    // both absent from ron-data/scriptfunctions.xml.
    assert_eq!(
        find_builtin("set_object_type_attack").unwrap().handler_va,
        0x009f6170
    );
    assert_eq!(
        find_builtin("set_object_type_armor").unwrap().handler_va,
        0x009f5fb0
    );
    // Name resolution is case-insensitive, as ScriptGameInterfaceBase::find_func is.
    assert!(find_builtin("NUM_CITIES").is_some());
    // Every declared arity equals the number of add_param calls.
    for b in don_bhs::BUILTINS.iter() {
        assert_eq!(b.arity as usize, b.params.len(), "{}", b.name);
    }
}

/// Every builtin the three shipped scripts in `ron-data/ai-scripts/` call must be
/// present in the table with a matching name. This is the list `don-sim` owes.
#[test]
fn every_shipped_script_builtin_resolves() {
    const SHIPPED: &[&str] = &[
        "age",
        "at_least_type",
        "building_started",
        "can_pay_cost",
        "citizen_repair_order",
        "destroy_building",
        "find_build",
        "find_build_at_city",
        "find_city_id",
        "find_city_with_num",
        "find_idle_citizen",
        "find_inactive_build",
        "find_nation",
        "find_num_idle_unit",
        "find_unit",
        "get_is_no_nation_powers",
        "get_mapstyle",
        "get_rush_rules",
        "get_starting_resources",
        "get_starting_town_size",
        "get_techs_per_age",
        "have_tech",
        "is_conquest_scenario",
        "is_victory_economic",
        "is_victory_musical_chairs",
        "is_victory_score",
        "is_victory_tech_race",
        "is_victory_territory",
        "is_victory_wonder",
        "max_workers_at_building",
        "num_cities",
        "num_city_buildings",
        "num_rare_resources_seen",
        "num_type",
        "num_type_queued",
        "num_type_with_queued",
        "num_workers_at_building",
        "object_position_x",
        "object_position_y",
        "place_building_upgrade_with_cost",
        "place_building_with_cost",
        "place_city_with_cost",
        "place_orphan_building_with_cost",
        "population",
        "rand_int",
        "research_tech_with_cost",
        "researching_tech",
        "set_timer",
        "stop_timer",
        "timer_expired",
        "train_unit_at_with_cost",
        "train_unit_with_cost",
        "unit_move_order",
        "was_city_attacked",
        "was_city_raided",
    ];
    assert_eq!(SHIPPED.len(), 55);
    for n in SHIPPED {
        assert!(
            find_builtin(n).is_some(),
            "shipped script calls missing builtin {n}"
        );
    }
}

#[test]
fn disassembler_round_trips_a_realistic_body() {
    let code = asm(&[
        (0x44, &[VarRef::Static(0).encode(), 30]),
        (0x26, &[VarRef::Const(0).encode()]),
        (0x33, &[VarRef::Static(0).encode()]),
        (0x26, &[VarRef::Local(1).encode()]),
        (0x38, &[find_builtin("num_cities").unwrap().index]),
        (0x3e, &[]),
    ]);
    let d = disassemble(&code).unwrap();
    assert_eq!(
        d.iter().map(|i| i.name).collect::<Vec<_>>(),
        vec![
            "OP_JUMP_IF_INITED",
            "OP_PUSH",
            "OP_INIT",
            "OP_PUSH",
            "OP_CALL_GAME",
            "OP_RETURN"
        ]
    );
    let text = don_bhs::disasm::format_all(&code).unwrap();
    assert!(text.contains("static[0]"));
    assert!(text.contains("num_cities#"));
}

#[test]
fn coverage_starts_empty_and_is_reportable() {
    let c = Coverage::default();
    assert!(c.is_complete());
    assert_eq!(c.distinct_called(), 0);
}
