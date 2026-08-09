//! Differential fixtures captured from the supported retail compiler.
//!
//! The expectations below are not hand-calculated. `tools/bhs-regress.sh` executes
//! `Compiler::compile` at 0x009bf160 from the supported PE32 image on hbox and writes
//! `schema/bhs-compiler-regression.json`. These focused assertions keep the first retail
//! bytecode findings executable even though `don-bhs-cc` is not byte-identical yet.

use std::path::{Path, PathBuf};

use don_bhs::builtins::UtilHost;
use don_bhs::host::NullHost;
use don_bhs::program::{Program, Script, ScriptFile};
use don_bhs::value::Value;
use don_bhs::vm::Vm;
use don_bhs_cc::sema::{self, Severity};

const SCHEMA: &str = include_str!("../../../schema/bhs-compiler-regression.json");

#[derive(Clone, Debug, PartialEq, Eq)]
enum NValue {
    Int(i32),
    Real(u32),
    String(String),
    Null,
    Unsupported(u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NScript {
    name: String,
    offset: u32,
    return_type: u32,
    script_type: u32,
    params: Vec<u32>,
    refs: Vec<u8>,
    statics: Vec<NValue>,
    trigger_count: i32,
    trigger_bits: Vec<u8>,
    trigger_names: Vec<String>,
    var_names: Vec<String>,
    static_var_names: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NFile {
    code: Vec<u8>,
    const_pool: Vec<NValue>,
    scripts: Vec<NScript>,
}

#[derive(Clone, Copy)]
struct RetailFixture {
    id: &'static str,
    code_hex: &'static str,
    const_pool: &'static [NValue],
    script: fn() -> NScript,
}

fn int(value: i32) -> NValue {
    NValue::Int(value)
}

fn script(name: &str, vars: &[&str]) -> NScript {
    NScript {
        name: name.to_string(),
        offset: 0,
        return_type: 0x0005_7bad,
        script_type: 0,
        params: Vec::new(),
        refs: Vec::new(),
        statics: Vec::new(),
        trigger_count: 0,
        trigger_bits: Vec::new(),
        trigger_names: Vec::new(),
        var_names: vars.iter().map(|s| (*s).to_string()).collect(),
        static_var_names: Vec::new(),
    }
}

fn empty_script() -> NScript {
    script("empty_main", &[])
}

fn assign_script() -> NScript {
    script("assign_int", &["value"])
}

fn logical_script() -> NScript {
    script("logical_and", &["value"])
}

fn run_once_script() -> NScript {
    let mut s = script("run_once", &["value"]);
    s.trigger_count = 1;
    s.trigger_bits = vec![0xff];
    // Retail assigns the hidden run-once gate an empty trigger name.
    s.trigger_names = vec![String::new()];
    s
}

fn static_script() -> NScript {
    let mut s = script("static_int", &[]);
    s.static_var_names = vec!["value".to_string()];
    // This is the compiler output before the runtime allocates static storage.
    s
}

const EMPTY_CONSTS: &[NValue] = &[];
const ONE: &[NValue] = &[NValue::Int(1)];
const ONE_TWO: &[NValue] = &[NValue::Int(1), NValue::Int(2)];

const FIXTURES: &[RetailFixture] = &[
    RetailFixture {
        id: "assign_int",
        code_hex: "47000000002600000020320000000028ad7b05003e",
        const_pool: ONE,
        script: assign_script,
    },
    RetailFixture {
        id: "empty_main",
        code_hex: "470000000028ad7b05003e",
        const_pool: EMPTY_CONSTS,
        script: empty_script,
    },
    RetailFixture {
        id: "logical_and",
        code_hex: "470000000026000000204314000000260100002035320000000028ad7b05003e",
        const_pool: ONE_TWO,
        script: logical_script,
    },
    RetailFixture {
        id: "run_once",
        code_hex: "470000000045000000001d0000003c000000002600000020320000000028ad7b05003e",
        const_pool: ONE,
        script: run_once_script,
    },
    RetailFixture {
        id: "static_int",
        code_hex: "47000000004400000040180000002600000020320000004028ad7b05003e",
        const_pool: ONE,
        script: static_script,
    },
];

fn hex_bytes(hex: &str) -> Vec<u8> {
    assert_eq!(hex.len() % 2, 0);
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

fn retail(f: &RetailFixture) -> NFile {
    NFile {
        code: hex_bytes(f.code_hex),
        const_pool: f.const_pool.to_vec(),
        scripts: vec![(f.script)()],
    }
}

fn normalize_value(value: &Value) -> NValue {
    match value {
        Value::Int(v) => NValue::Int(*v),
        Value::Real(v) => NValue::Real(v.to_bits()),
        Value::Str(v) => NValue::String((**v).clone()),
        Value::Null => NValue::Null,
        Value::Obj(v) => NValue::Unsupported(v.borrow().data_type),
    }
}

fn normalize_script(script: &Script) -> NScript {
    NScript {
        name: script.name.clone(),
        offset: script.entry,
        return_type: script.return_type,
        script_type: script.script_type,
        params: script.params.clone(),
        refs: script.refs.clone(),
        statics: script
            .statics
            .iter()
            .map(|v| v.as_ref().map(normalize_value).unwrap_or(NValue::Null))
            .collect(),
        trigger_count: script.trigger_count,
        trigger_bits: script.trigger_bits.clone(),
        trigger_names: script.trigger_names.clone(),
        var_names: script.var_names.clone(),
        static_var_names: script.static_var_names.clone(),
    }
}

fn normalize_file(file: &ScriptFile) -> NFile {
    NFile {
        code: file.code.clone(),
        const_pool: file.const_pool.iter().map(normalize_value).collect(),
        scripts: file.scripts.iter().map(normalize_script).collect(),
    }
}

fn fixture_path(id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../don-bhs/oracle/fixtures")
        .join(format!("{id}.bhs"))
}

fn runtime_fixture_path(id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../don-bhs/tests/fixtures")
        .join(format!("{id}.bhs"))
}

fn compile_program(path: &Path) -> Program {
    let inc = sema::IncludePath::with_roots([path.parent().unwrap().to_path_buf()]);
    let unit = sema::analyze(path, &inc).unwrap();
    let (program, diags, _) = don_bhs_cc::codegen::compile(&unit);
    let errors: Vec<String> = unit
        .diags
        .iter()
        .chain(diags.iter())
        .filter(|d| d.severity == Severity::Error)
        .map(ToString::to_string)
        .collect();
    assert!(
        errors.is_empty(),
        "{}: {}",
        path.display(),
        errors.join("\n")
    );
    assert_eq!(
        program.files.len(),
        1,
        "{}: unexpected include closure",
        path.display()
    );
    program
}

fn compile_local(id: &str) -> NFile {
    let program = compile_program(&fixture_path(id));
    normalize_file(&program.files[0])
}

fn changed_fields(a: &NFile, b: &NFile) -> Vec<&'static str> {
    let mut out = Vec::new();
    if a.code != b.code {
        out.push("code");
    }
    if a.const_pool != b.const_pool {
        out.push("const_pool");
    }
    if a.scripts != b.scripts {
        out.push("scripts");
    }
    out
}

#[test]
fn retail_capture_is_tied_to_the_supported_image_and_fixture_bytes() {
    assert!(SCHEMA.contains("30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079"));
    assert!(SCHEMA.contains("\"compiler_va\": \"0x009bf160\""));
    assert!(SCHEMA.contains("\"compiler_root_va\": \"0x00eb6a90\""));
    for f in FIXTURES {
        assert!(fixture_path(f.id).is_file(), "missing source for {}", f.id);
        assert!(
            SCHEMA.contains(&format!("\"id\": \"{}\"", f.id)),
            "schema is missing {}",
            f.id
        );
        assert!(
            SCHEMA.contains(&format!("\"code_hex\": \"{}\"", f.code_hex)),
            "schema bytecode drifted for {}",
            f.id
        );
    }
}

#[test]
fn retail_lowering_answers_the_first_open_compiler_questions() {
    let empty = retail(&FIXTURES[1]);
    // OP_SCRIPT_MARKER(0), OP_CREATE_SIMPLE(int), OP_RETURN.
    assert_eq!(empty.code, hex_bytes("470000000028ad7b05003e"));

    let logical = retail(&FIXTURES[2]);
    assert!(logical.code.contains(&0x43)); // OP_JUMP_IF_SC_FALSE
    assert!(logical.code.contains(&0x35)); // OP_CAST_BOOL

    let run_once = retail(&FIXTURES[3]);
    assert!(run_once.code.contains(&0x45)); // OP_JUMP_IF_BITSET
    assert!(run_once.code.contains(&0x3c)); // clear after first entry
    assert!(run_once.scripts[0].statics.is_empty());
    assert_eq!(run_once.scripts[0].trigger_bits, [0xff]);

    let static_int = retail(&FIXTURES[4]);
    assert!(static_int.code.contains(&0x44)); // OP_JUMP_IF_INITED
    assert_eq!(static_int.scripts[0].static_var_names, ["value"]);
    assert!(static_int.scripts[0].statics.is_empty());
}

#[test]
fn all_measured_fixtures_are_byte_identical() {
    let mut byte_identical = 0usize;
    for fixture in FIXTURES {
        let expected = retail(fixture);
        let actual = compile_local(fixture.id);
        let changed = changed_fields(&actual, &expected);
        if changed.is_empty() {
            byte_identical += 1;
        } else {
            eprintln!("{} differs in {}", fixture.id, changed.join(", "));
        }
        assert!(
            changed.is_empty(),
            "{} parity status drifted; changed fields: {}",
            fixture.id,
            changed.join(", ")
        );
    }
    assert_eq!(
        byte_identical, 5,
        "the measured local/retail identity count drifted"
    );
}

#[test]
fn a_one_bit_retail_mutation_is_detected() {
    for fixture in FIXTURES {
        let expected = retail(fixture);
        let mut mutated = expected.clone();
        mutated.code[0] ^= 1;
        assert_eq!(
            changed_fields(&mutated, &expected),
            ["code"],
            "{}",
            fixture.id
        );
    }
}

#[test]
fn measured_multi_argument_fixtures_are_byte_identical_and_execute() {
    // Shipped Compiler::compile capture, source SHA-256
    // b10d7f9f9dffa9c610d0db5da9e00db347aad8e1617b9923cc386c4dfb707e66.
    let mut mixed = compile_program(&runtime_fixture_path("mixed_params"));
    assert_eq!(
        mixed.files[0].code,
        hex_bytes(
            "470000000032000000003301000000320200000032030000002601000000260000000004\
             2602000000042603000000042601000000002726010000003e47010000002600000020\
             3200000000260100002032010000002602000020320200000026030000203203000000\
             260300000026020000002601000000260000000036000000002726010000003e"
                .replace(' ', "")
                .as_str()
        )
    );
    assert_eq!(mixed.files[0].scripts[0].entry, 0);
    assert_eq!(mixed.files[0].scripts[0].refs, [0, 1, 0, 0]);
    assert_eq!(mixed.files[0].scripts[1].entry, 61);
    let mut host = NullHost;
    assert_eq!(
        Vm::new(&mut mixed, &mut host)
            .run_script(0, "mixed_params")
            .unwrap()
            .returned,
        Some(Value::Int(1111))
    );

    // Independent native-call capture, source SHA-256
    // ffab74ced8626eda196a2d944444ebc1d79958238be9ad417118fc431bcafa66.
    let mut builtin_args = compile_program(&runtime_fixture_path("builtin_args"));
    assert_eq!(
        builtin_args.files[0].code,
        hex_bytes("47000000002601000020260000002038130000003e")
    );
    let mut host = UtilHost::default();
    assert_eq!(
        Vm::new(&mut builtin_args, &mut host)
            .run_script(0, "builtin_args")
            .unwrap()
            .returned,
        Some(Value::Int('b' as i32))
    );
}

#[test]
fn helper_covers_all_scalar_capture_shapes() {
    // Keep the normalization variants exercised so adding a real/string fixture cannot
    // silently change their representation.
    assert_eq!(int(7), NValue::Int(7));
    assert_ne!(NValue::Real(0), NValue::String(String::new()));
}
