//! The aggregate opcodes: dynamic arrays and struct field access.
//!
//! Every expectation here is the behaviour of a specific retail handler inside
//! `VirtualMachine::execute_next`, cited at the test. These are *behaviour* tests of
//! our reading of the machine, not evidence about retail.

use don_bhs::disasm::{asm, asm_len};
use don_bhs::host::NullHost;
use don_bhs::program::{Program, Script, ScriptFile};
use don_bhs::value::{ScriptTy, Value};
use don_bhs::vm::{VarRef, Vm};

const INT: u32 = 0x0005_7bad;
const STR: u32 = 0x0016_8174;
/// A `StructType`'s own tag is a `String::generate_hash` of its name, so it is not
/// one of the ten builtin tags. Any distinct value stands in for one here.
const VECTOR_STRUCT: u32 = 0x00ab_cdef;

fn run(code: Vec<u8>, consts: Vec<Value>) -> don_bhs::vm::RunOutcome {
    let mut prog = Program::single(ScriptFile {
        code,
        const_pool: consts,
        scripts: vec![Script {
            name: "t".into(),
            ..Default::default()
        }],
        ..Default::default()
    });
    let mut host = NullHost;
    let mut vm = Vm::new(&mut prog, &mut host);
    vm.run_script(0, "t").expect("no implementation gap")
}

/// `int[] a; a[2] = 7; return a[2];`
///
/// `OP_CREATE_ARRAY` (0x29, handler `0x009e0aa4`) pops the prototype element and
/// makes an empty array; `OP_CREATE_ARRAY_INDEX` (0x2e, `0x009e0cc3`) is the
/// *lvalue* subscript and calls `resize_array(idx + 1, grow_only)`;
/// `OP_PUSH_ARRAY_INDEX` (0x2d, `0x009e0b87`) is the *rvalue* subscript and never
/// grows. Assignment writes through the element slot the subscript pushed.
#[test]
fn a_subscripted_store_grows_the_array_and_a_load_reads_it_back() {
    let code = asm(&[
        (0x28, &[INT]),                       // OP_CREATE_SIMPLE int -> prototype
        (0x29, &[INT]),                       // OP_CREATE_ARRAY int
        (0x33, &[VarRef::Local(0).encode()]), // OP_INIT local[0] = the array
        (0x26, &[VarRef::Const(0).encode()]), // push 7          (value)
        (0x26, &[VarRef::Local(0).encode()]), // push a
        (0x26, &[VarRef::Const(1).encode()]), // push 2
        (0x2e, &[]),                          // OP_CREATE_ARRAY_INDEX -> &a[2]
        (0x00, &[]),                          // OP_ASSIGN (target is on TOP)
        (0x27, &[]),                          // OP_POP
        (0x26, &[VarRef::Local(0).encode()]),
        (0x26, &[VarRef::Const(1).encode()]),
        (0x2d, &[]), // OP_PUSH_ARRAY_INDEX
        (0x3e, &[]), // OP_RETURN
    ]);
    let out = run(code, vec![Value::Int(7), Value::Int(2)]);
    assert!(out.ok(), "{:?}", out.error);
    assert_eq!(out.returned, Some(Value::Int(7)));
}

/// `OP_PUSH_ARRAY_LENGTH` (0x30, `0x009e0e95`) pushes a fresh `ScriptInt` holding
/// `values.count`. Growing to index 2 must therefore report length 3, because
/// `resize_array` fills the gap with `blank_base` duplicates.
#[test]
fn growing_to_an_index_fills_the_gap_and_length_sees_it() {
    let code = asm(&[
        (0x28, &[INT]),
        (0x29, &[INT]),
        (0x33, &[VarRef::Local(0).encode()]),
        (0x26, &[VarRef::Const(0).encode()]), // 7
        (0x26, &[VarRef::Local(0).encode()]),
        (0x26, &[VarRef::Const(1).encode()]), // 2
        (0x2e, &[]),
        (0x00, &[]),
        (0x27, &[]),
        (0x26, &[VarRef::Local(0).encode()]),
        (0x30, &[]), // OP_PUSH_ARRAY_LENGTH
        (0x3e, &[]),
    ]);
    let out = run(code, vec![Value::Int(7), Value::Int(2)]);
    assert!(out.ok(), "{:?}", out.error);
    assert_eq!(out.returned, Some(Value::Int(3)));
}

/// `OP_SET_ARRAY_LENGTH` (0x31, `0x009e0f0c`) takes the **array from the top of the
/// stack** and the length beneath it — the same operand order as `OP_ASSIGN` and the
/// opposite of a binary operator — calls `resize_array(n, 0)` (which may shrink) and
/// then pushes the length value back, which is why a statement-position
/// `a.length = n` is followed by `OP_POP`.
#[test]
fn setting_the_length_shrinks_and_leaves_the_length_on_the_stack() {
    let code = asm(&[
        (0x28, &[INT]),
        (0x26, &[VarRef::Const(1).encode()]), // 5 elements
        (0x2a, &[INT]),                       // OP_CREATE_ARRAY_DYN
        (0x33, &[VarRef::Local(0).encode()]),
        (0x26, &[VarRef::Const(0).encode()]), // push new length 2
        (0x26, &[VarRef::Local(0).encode()]), // push the array (TOP)
        (0x31, &[]),                          // OP_SET_ARRAY_LENGTH -> pushes 2 back
        (0x27, &[]),                          // OP_POP
        (0x26, &[VarRef::Local(0).encode()]),
        (0x30, &[]),
        (0x3e, &[]),
    ]);
    let out = run(code, vec![Value::Int(2), Value::Int(5)]);
    assert!(out.ok(), "{:?}", out.error);
    assert_eq!(out.returned, Some(Value::Int(2)));
}

/// `OP_CREATE_STRUCT` (0x2c, `0x009e0b67`) -> `ScriptObject::init_struct`
/// (`0x009d8410`): it pops `count` values off the run stack and appends
/// `duplicate()` of each **in pop order**, so `values[0]` is whatever was on top.
/// `OP_PUSH_STRUCT_FIELD` (0x2f, `0x009e0dc5`) then indexes that same array.
#[test]
fn struct_fields_are_indexed_slots_and_values_zero_is_the_top_of_stack() {
    // Push field 1 first, then field 0, so the struct reads {0: "a", 1: "b"}.
    let code = asm(&[
        (0x26, &[VarRef::Const(1).encode()]), // "b"
        (0x26, &[VarRef::Const(0).encode()]), // "a"
        (0x2c, &[2, VECTOR_STRUCT]),          // OP_CREATE_STRUCT count=2
        (0x33, &[VarRef::Local(0).encode()]),
        (0x26, &[VarRef::Local(0).encode()]),
        (0x2f, &[1]), // OP_PUSH_STRUCT_FIELD 1
        (0x3e, &[]),
    ]);
    let out = run(code, vec![Value::str("a"), Value::str("b")]);
    assert!(out.ok(), "{:?}", out.error);
    assert_eq!(out.returned, Some(Value::str("b")));
}

/// A struct field is an aliasable slot: assigning through
/// `OP_PUSH_STRUCT_FIELD` must be visible on the next read, because the engine
/// pushes `&obj->values[i]` rather than a copy.
#[test]
fn a_store_through_a_struct_field_aliases_the_struct() {
    let code = asm(&[
        (0x26, &[VarRef::Const(1).encode()]),
        (0x26, &[VarRef::Const(0).encode()]),
        (0x2c, &[2, VECTOR_STRUCT]),
        (0x33, &[VarRef::Local(0).encode()]),
        // s.field0 = "z"
        (0x26, &[VarRef::Const(2).encode()]), // "z" (value)
        (0x26, &[VarRef::Local(0).encode()]),
        (0x2f, &[0]), // &s.values[0]  (target, on TOP)
        (0x00, &[]),  // OP_ASSIGN
        (0x27, &[]),
        (0x26, &[VarRef::Local(0).encode()]),
        (0x2f, &[0]),
        (0x3e, &[]),
    ]);
    let out = run(
        code,
        vec![Value::str("a"), Value::str("b"), Value::str("z")],
    );
    assert!(out.ok(), "{:?}", out.error);
    assert_eq!(out.returned, Some(Value::str("z")));
}

/// `OP_CREATE_ARRAY_INITER` (0x2b, `0x009e0b47`) -> `ScriptArray::init_array`
/// (`0x009d6170`): pops `count` values, and additionally derives `blank_base` from
/// `values[0]->duplicate()->clear()`, which is what a later grow duplicates.
#[test]
fn an_initialiser_list_builds_the_array_and_its_growth_prototype() {
    let code = asm(&[
        (0x26, &[VarRef::Const(2).encode()]), // 30
        (0x26, &[VarRef::Const(1).encode()]), // 20
        (0x26, &[VarRef::Const(0).encode()]), // 10
        (0x2b, &[3, INT]),                    // OP_CREATE_ARRAY_INITER count=3
        (0x33, &[VarRef::Local(0).encode()]),
        // grow to index 4; the new slots come from blank_base, i.e. 0
        (0x26, &[VarRef::Const(3).encode()]), // 99 (value)
        (0x26, &[VarRef::Local(0).encode()]),
        (0x26, &[VarRef::Const(4).encode()]), // 4
        (0x2e, &[]),
        (0x00, &[]),
        (0x27, &[]),
        (0x26, &[VarRef::Local(0).encode()]),
        (0x26, &[VarRef::Const(5).encode()]), // 3 — the gap slot
        (0x2d, &[]),
        (0x3e, &[]),
    ]);
    let out = run(
        code,
        vec![
            Value::Int(10),
            Value::Int(20),
            Value::Int(30),
            Value::Int(99),
            Value::Int(4),
            Value::Int(3),
        ],
    );
    assert!(out.ok(), "{:?}", out.error);
    // blank_base is `values[0]` cleared, so the gap is 0 and not 10.
    assert_eq!(out.returned, Some(Value::Int(0)));
}

/// The rvalue subscript raises a `run_time_error` past the end rather than growing:
/// `0x009e0bcf` compares against `values.count` and jumps to the error path. A
/// runtime error is retail behaviour, so it must surface as [`RunOutcome::error`],
/// **not** as a `VmError` — the two are different claims and this test pins the
/// distinction.
#[test]
fn reading_past_the_end_is_a_retail_runtime_error_not_an_implementation_gap() {
    let code = asm(&[
        (0x28, &[INT]),
        (0x29, &[INT]),
        (0x33, &[VarRef::Local(0).encode()]),
        (0x26, &[VarRef::Local(0).encode()]),
        (0x26, &[VarRef::Const(0).encode()]), // index 3 into an empty array
        (0x2d, &[]),
        (0x3e, &[]),
    ]);
    let out = run(code, vec![Value::Int(3)]);
    assert!(!out.ok());
    assert_eq!(out.err_count, 1);
    let e = out.error.unwrap();
    assert!(e.message.contains("out of range"), "{}", e.message);
    assert_eq!(e.op, 0x2d);
}

/// `resize_array` (`0x009d5cd0`) opens with `if (blank_base == NULL) run_time_error`,
/// so the lvalue subscript on a *struct* fails: a struct has no growth prototype.
/// `OP_CREATE_ARRAY_INDEX` also gates on `is_array()` before that.
#[test]
fn a_struct_cannot_be_subscripted_as_an_array() {
    let code = asm(&[
        (0x26, &[VarRef::Const(0).encode()]),
        (0x2c, &[1, VECTOR_STRUCT]),
        (0x33, &[VarRef::Local(0).encode()]),
        (0x26, &[VarRef::Local(0).encode()]),
        (0x26, &[VarRef::Const(1).encode()]),
        (0x2e, &[]),
        (0x3e, &[]),
    ]);
    let out = run(code, vec![Value::Int(1), Value::Int(0)]);
    assert!(!out.ok());
    assert!(out.error.unwrap().message.contains("not an array"));
}

/// Mixed-type arithmetic is a retail `run_time_error`, not a promotion, and it stops
/// the run (`err_count++; script_status = 3`).
#[test]
fn mixed_type_arithmetic_stops_the_run_the_way_retail_does() {
    let code = asm(&[
        (0x26, &[VarRef::Const(0).encode()]),
        (0x26, &[VarRef::Const(1).encode()]),
        (0x04, &[]), // OP_ADD int + real
        (0x3e, &[]),
    ]);
    let out = run(code, vec![Value::Int(1), Value::Real(2.0)]);
    assert!(!out.ok());
    assert_eq!(out.err_count, 1);
    assert!(out.error.unwrap().message.contains("type mismatch"));
}

/// `OP_INIT_COPY` (0x32) calls `duplicate()` before storing and `OP_INIT` (0x33)
/// stores the pointer. On an aggregate that is the difference between two arrays and
/// two names for one array.
#[test]
fn init_copy_duplicates_an_aggregate_while_init_aliases_it() {
    let build = |init_op: u8| {
        asm(&[
            (0x28, &[INT]),
            (0x26, &[VarRef::Const(0).encode()]), // 1 element
            (0x2a, &[INT]),                       // OP_CREATE_ARRAY_DYN
            (0x33, &[VarRef::Local(0).encode()]),
            (0x26, &[VarRef::Local(0).encode()]),
            (init_op, &[VarRef::Local(1).encode()]), // local1 = local0 (copy or alias)
            // local0[0] = 42
            (0x26, &[VarRef::Const(1).encode()]),
            (0x26, &[VarRef::Local(0).encode()]),
            (0x26, &[VarRef::Const(2).encode()]),
            (0x2e, &[]),
            (0x00, &[]),
            (0x27, &[]),
            // read local1[0]
            (0x26, &[VarRef::Local(1).encode()]),
            (0x26, &[VarRef::Const(2).encode()]),
            (0x2d, &[]),
            (0x3e, &[]),
        ])
    };
    let consts = vec![Value::Int(1), Value::Int(42), Value::Int(0)];
    let aliased = run(build(0x33), consts.clone());
    assert_eq!(aliased.returned, Some(Value::Int(42)));
    let copied = run(build(0x32), consts);
    assert_eq!(copied.returned, Some(Value::Int(0)));
}

/// The trigger-utility builtins are answered by the VM against the running script,
/// because their handlers read `[0x00ebeed0]`. `enable_all_triggers` sets the bits;
/// `is_trigger_enabled(name)` resolves through `Script::trigger_names` and returns
/// `-1` for an unknown name.
#[test]
fn trigger_utility_builtins_act_on_the_running_script() {
    let code = asm(&[
        (0x38, &[16]), // disable_all_triggers
        (0x38, &[15]), // enable_all_triggers
        (0x26, &[VarRef::Const(0).encode()]),
        (0x39, &[17, 1]), // is_trigger_enabled("t0")
        (0x3e, &[]),
    ]);
    let mut prog = Program::single(ScriptFile {
        code,
        const_pool: vec![Value::str("t0")],
        scripts: vec![Script {
            name: "t".into(),
            trigger_names: vec!["t0".into(), "t1".into()],
            trigger_bits: vec![0u8],
            trigger_count: 2,
            ..Default::default()
        }],
        ..Default::default()
    });
    let mut host = NullHost;
    let mut vm = Vm::new(&mut prog, &mut host);
    let out = vm.run_script(0, "t").unwrap();
    assert!(out.ok(), "{:?}", out.error);
    assert_eq!(out.returned, Some(Value::Int(1)));
    // Answered by the VM, so it counts as implemented coverage, not debt.
    assert!(vm.coverage.is_complete());
}

/// A script-typed sanity check that the assembler's offsets and the VM's `bip`
/// arithmetic still agree once two-operand aggregate opcodes are in the stream.
#[test]
fn two_operand_aggregate_opcodes_keep_the_stream_aligned() {
    let prologue: &[(u8, &[u32])] = &[(0x2b, &[0, INT])];
    assert_eq!(asm_len(prologue), 9);
    let code = asm(&[
        (0x26, &[VarRef::Const(0).encode()]),
        (0x2b, &[1, STR]),
        (0x27, &[]),
        (0x26, &[VarRef::Const(0).encode()]),
        (0x3e, &[]),
    ]);
    let insns = don_bhs::disasm::disassemble(&code).unwrap();
    assert_eq!(insns[1].len, 9);
    assert_eq!(insns[2].offset, 5 + 9);
    let out = run(code, vec![Value::str("q")]);
    assert!(out.ok(), "{:?}", out.error);
    assert_eq!(out.returned, Some(Value::str("q")));
    assert_eq!(ScriptTy::Str.tag(), STR);
}
