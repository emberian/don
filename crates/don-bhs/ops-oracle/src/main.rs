//! Differentially test `don_bhs::ops::do_operator` against **retail's own**
//! `ScriptType::do_operator`, with no game and no world.
//!
//! # Why this is possible at all
//!
//! Every arithmetic, comparison, logical and assignment opcode in
//! `VirtualMachine::execute_next` funnels through one virtual call,
//! `ScriptType::do_operator(int op, ScriptType* rhs)` at vtable slot +44. There are
//! only five implementations, and the two that carry all the numeric semantics —
//! `ScriptInt::do_operator` (`0x009d7760`) and `ScriptFloat::do_operator`
//! (`0x009d7040`) — read exactly two fields of each operand (`data_type` at +4,
//! `value` at +16) and allocate results through a static recycler that falls back to
//! `malloc`. So the entire operator surface is reachable by mapping the image,
//! fabricating two 20-byte objects, and calling one address. No `Game`, no
//! `Constants`, no `StringTable`.
//!
//! # It compares against the shipped crate
//!
//! `don-bhs` is a path dependency here and the model side of every case is
//! `don_bhs::ops::do_operator` itself, not a copy typed into this file. That is the
//! specific mistake `README-LLM.md` records ("a differential test whose model is an
//! inline copy tests the copy"), and it is designed out rather than remembered.
//!
//! ```sh
//! # on hbox only — this is 32-bit x86 machine code
//! cd ~/don-bhs-ops-oracle/crates/don-bhs/ops-oracle
//! nice -n 15 taskset -c 0-3 cargo build --target i686-unknown-linux-musl -q
//! RON_EXE=../oracle/data/riseofnations.exe \
//!   ./target/i686-unknown-linux-musl/debug/bhsops sweep
//! ```

#![allow(static_mut_refs)]

// Re-use the compiler lane's PE mapper and Win32 shim without touching them.
#[path = "../../oracle/src/image.rs"]
mod image;
#[path = "../../oracle/src/win.rs"]
mod win;

use don_bhs::value::Value;
use image::{guard_for, Mapped};

// --- retail VAs [measured, rise.pdb] ------------------------------------------------
const VA_SCRIPTINT_VFTABLE: u32 = 0x00b5_ef28;
const VA_SCRIPTFLOAT_VFTABLE: u32 = 0x00b5_f01c;
const VA_INT_DO_OPERATOR: u32 = 0x009d_7760;
const VA_FLOAT_DO_OPERATOR: u32 = 0x009d_7040;

const TAG_INT: u32 = 0x0005_7bad;
const TAG_REAL: u32 = 0x0012_f35f;

/// `VarScope::VM_VAR` — a variable-owned value, so retail never tries to recycle our
/// stack-allocated operands.
const VM_VAR: u32 = 3;

/// `ScriptInt` / `ScriptFloat` are both `sizeof 20`: vptr, data_type, scope,
/// ref_count(u16)+pad, value.
#[repr(C, align(4))]
#[derive(Clone, Copy)]
struct ScriptScalar {
    vptr: u32,
    data_type: u32,
    scope: u32,
    ref_count: u32,
    value: u32,
}

impl ScriptScalar {
    fn int(m: &Mapped, v: i32) -> ScriptScalar {
        ScriptScalar {
            vptr: m.at(VA_SCRIPTINT_VFTABLE) as u32,
            data_type: TAG_INT,
            scope: VM_VAR,
            ref_count: 1,
            value: v as u32,
        }
    }
    fn real(m: &Mapped, v: f32) -> ScriptScalar {
        ScriptScalar {
            vptr: m.at(VA_SCRIPTFLOAT_VFTABLE) as u32,
            data_type: TAG_REAL,
            scope: VM_VAR,
            ref_count: 1,
            value: v.to_bits(),
        }
    }
}

#[inline(never)]
unsafe fn thiscall2(f: u32, this: u32, a: u32, b: u32) -> u32 {
    let r: u32;
    std::arch::asm!(
        "push {b:e}",
        "push {a:e}",
        "call {f:e}",
        a = in(reg) a, b = in(reg) b, f = in(reg) f, in("ecx") this,
        lateout("eax") r, clobber_abi("C"),
    );
    r
}

fn load() -> Mapped {
    let path = std::env::var("RON_EXE").unwrap_or_else(|_| "data/riseofnations.exe".into());
    let bytes = std::fs::read(&path).unwrap_or_else(|e| {
        eprintln!("cannot read {path}: {e}");
        std::process::exit(3);
    });
    Mapped::load(&bytes).unwrap_or_else(|e| {
        eprintln!("map failed: {e}");
        std::process::exit(3);
    })
}

fn setup(m: &Mapped) {
    image::install_fault_handler();
    if let Err(e) = image::install_fake_teb(Some(m)) {
        eprintln!("fake TEB failed: {e}");
        std::process::exit(3);
    }
    let env = win::install(m);
    eprintln!(
        "[ops] IAT: {} imports, {} bound, {} trapped",
        env.imports.len(),
        env.implemented,
        env.trapped
    );
}

/// One retail evaluation. `None` means the call faulted or timed out.
struct Retail {
    data_type: u32,
    raw: u32,
    /// `this->value` after the call — mutating opcodes write through it.
    this_after: u32,
}

fn call_retail(m: &Mapped, func: u32, lhs: &mut ScriptScalar, op: u32, rhs: *const ScriptScalar) -> Option<Retail> {
    let f = m.at(func) as u32;
    let mut out: u32 = 0;
    let this = lhs as *mut ScriptScalar as u32;
    let r = guard_for(5, || unsafe {
        out = thiscall2(f, this, op, rhs as u32);
    });
    if r.is_err() {
        return None;
    }
    if out < 0x1000 {
        return None;
    }
    unsafe {
        Some(Retail {
            data_type: std::ptr::read_unaligned((out + 4) as *const u32),
            raw: std::ptr::read_unaligned((out + 16) as *const u32),
            this_after: lhs.value,
        })
    }
}

/// Turn a retail result into the `Value` our model would have produced.
fn as_value(r: &Retail) -> Option<Value> {
    match r.data_type {
        TAG_INT => Some(Value::Int(r.raw as i32)),
        TAG_REAL => Some(Value::Real(f32::from_bits(r.raw))),
        _ => None,
    }
}

/// Opcodes whose retail handler mutates `this` and returns it; for those the model's
/// returned value is compared against `this->value` after the call.
fn mutates(op: u32) -> bool {
    matches!(op, 0x00 | 0x03 | 0x0c | 0x0d | 0x0e | 0x12..=0x15 | 0x18..=0x1e)
}

fn unary(op: u32) -> bool {
    matches!(op, 0x09 | 0x12 | 0x13 | 0x14 | 0x15 | 0x16 | 0x25)
}

struct Stats {
    cases: u64,
    agree: u64,
    disagree: Vec<String>,
    skipped: Vec<String>,
}

fn sweep_int(m: &Mapped, s: &mut Stats) {
    // Operand grid: signs, zero, small magnitudes, and the extremes that expose
    // shift, division and wrap behaviour.
    let vals: [i32; 13] = [0, 1, 2, 3, 5, 7, -1, -2, -7, 31, 32, i32::MIN, i32::MAX];
    // Every opcode ScriptInt::do_operator has a case for. Division and modulo by
    // zero are excluded here and probed separately, because their handler formats a
    // message out of the StringTable.
    let ops: [u32; 33] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
        0x20, 0x21, 0x22,
    ];
    for &op in ops.iter().chain([0x23u32, 0x24, 0x25].iter()) {
        for &a in vals.iter() {
            for &b in vals.iter() {
                // idiv traps on INT_MIN / -1 in retail exactly as it would here.
                if matches!(op, 0x0d | 0x10 | 0x19 | 0x1f) && (b == 0 || (a == i32::MIN && b == -1))
                {
                    continue;
                }
                let mut l = ScriptScalar::int(m, a);
                let r = ScriptScalar::int(m, b);
                let rhs = if unary(op) {
                    std::ptr::null()
                } else {
                    &r as *const ScriptScalar
                };
                let got = match call_retail(m, VA_INT_DO_OPERATOR, &mut l, op, rhs) {
                    Some(g) => g,
                    None => {
                        s.skipped.push(format!("int op {op:#04x} a={a} b={b}: fault/timeout"));
                        continue;
                    }
                };
                let rv = if unary(op) { None } else { Some(Value::Int(b)) };
                let model = don_bhs::ops::do_operator(&Value::Int(a), op as u8, rv.as_ref());
                s.cases += 1;
                let retail_v = if mutates(op) {
                    Value::Int(got.this_after as i32)
                } else {
                    match as_value(&got) {
                        Some(v) => v,
                        None => {
                            s.skipped.push(format!(
                                "int op {op:#04x} a={a} b={b}: unknown result tag {:#x}",
                                got.data_type
                            ));
                            continue;
                        }
                    }
                };
                match model {
                    Ok(mv) if mv == retail_v => s.agree += 1,
                    Ok(mv) => s.disagree.push(format!(
                        "int op {op:#04x} a={a} b={b}: retail {retail_v:?} model {mv:?}"
                    )),
                    Err(e) => s.disagree.push(format!(
                        "int op {op:#04x} a={a} b={b}: retail {retail_v:?} model Err({e:?})"
                    )),
                }
                if unary(op) {
                    break;
                }
            }
        }
    }
}

fn sweep_float(m: &Mapped, s: &mut Stats) {
    let vals: [f32; 11] = [0.0, 1.0, 2.0, 0.5, -0.5, -1.0, 3.25, -3.25, 1e9, -1e9, 1e-8];
    let ops: [u32; 19] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f, 0x10, 0x11, 0x16,
    ];
    for &op in ops.iter() {
        for &a in vals.iter() {
            for &b in vals.iter() {
                if matches!(op, 0x0d | 0x10) && b == 0.0 {
                    continue;
                }
                let mut l = ScriptScalar::real(m, a);
                let r = ScriptScalar::real(m, b);
                let rhs = if unary(op) {
                    std::ptr::null()
                } else {
                    &r as *const ScriptScalar
                };
                let got = match call_retail(m, VA_FLOAT_DO_OPERATOR, &mut l, op, rhs) {
                    Some(g) => g,
                    None => {
                        s.skipped.push(format!("real op {op:#04x} a={a} b={b}: fault/timeout"));
                        continue;
                    }
                };
                let rv = if unary(op) { None } else { Some(Value::Real(b)) };
                let model = don_bhs::ops::do_operator(&Value::Real(a), op as u8, rv.as_ref());
                s.cases += 1;
                let retail_v = if mutates(op) {
                    Value::Real(f32::from_bits(got.this_after))
                } else {
                    match as_value(&got) {
                        Some(v) => v,
                        None => {
                            s.skipped
                                .push(format!("real op {op:#04x}: unknown tag {:#x}", got.data_type));
                            continue;
                        }
                    }
                };
                match model {
                    Ok(mv) if mv == retail_v => s.agree += 1,
                    Ok(mv) => s.disagree.push(format!(
                        "real op {op:#04x} a={a} b={b}: retail {retail_v:?} model {mv:?}"
                    )),
                    Err(e) => s.disagree.push(format!(
                        "real op {op:#04x} a={a} b={b}: retail {retail_v:?} model Err({e:?})"
                    )),
                }
                if unary(op) {
                    break;
                }
            }
        }
    }
}

fn main() {
    let cmd = std::env::args().nth(1).unwrap_or_else(|| "sweep".into());
    let m = load();
    setup(&m);
    match cmd.as_str() {
        "probe" => {
            // The cheapest possible smoke test: 2 + 3.
            let mut l = ScriptScalar::int(&m, 2);
            let r = ScriptScalar::int(&m, 3);
            match call_retail(&m, VA_INT_DO_OPERATOR, &mut l, 0x04, &r) {
                Some(g) => println!(
                    "retail 2 + 3 -> tag {:#x} value {}",
                    g.data_type, g.raw as i32
                ),
                None => {
                    println!("FAULT on the smoke test; the environment is not usable");
                    std::process::exit(1);
                }
            }
        }
        "sweep" => {
            let mut s = Stats {
                cases: 0,
                agree: 0,
                disagree: Vec::new(),
                skipped: Vec::new(),
            };
            sweep_int(&m, &mut s);
            sweep_float(&m, &mut s);
            println!("cases {} agree {} disagree {}", s.cases, s.agree, s.disagree.len());
            for d in s.disagree.iter().take(200) {
                println!("  MISMATCH {d}");
            }
            if !s.skipped.is_empty() {
                println!("skipped {} (first 20):", s.skipped.len());
                for d in s.skipped.iter().take(20) {
                    println!("  SKIP {d}");
                }
            }
            // A sweep that could not run is SKIPPED, never green.
            if s.cases == 0 {
                std::process::exit(2);
            }
            std::process::exit(if s.disagree.is_empty() { 0 } else { 1 });
        }
        _ => eprintln!("usage: bhsops <probe|sweep>   env: RON_EXE=path/to/riseofnations.exe"),
    }
}
