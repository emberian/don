//! The host boundary: everything a script can do *to* the simulation.
//!
//! `OP_CALL_GAME` / `OP_CALL_GAME_VARIED` are the only opcodes that leave the VM.
//! The engine's path is
//! `VirtualMachine::call_func(nargs, index)` (`0x009e0550`)
//! -> `ScriptFuncSet::call_func(func->index, ScriptParamStack&)` (`0x009d5500`)
//! -> `func->address()` (the native handler at `ScriptFunc+0x4c`).
//!
//! Call convention, [measured] from `0x009e0550`:
//!
//! - Arguments are already on the **shared run stack**, pushed left to right, so the
//!   last argument is on top.
//! - `nargs < 0` (which is how `OP_CALL_GAME` encodes "default") is replaced by
//!   `ScriptFunc::params.count`, i.e. the declared arity.
//! - The handler receives a `ScriptParamStack { Stack* stack; int num_params;
//!   int orig_stack_size; }` and consumes all `num_params` entries.
//! - The return value is pushed back **only if** `func->return_type != 0x84048`
//!   (void). The VM then asserts the stack ended at `orig - nargs + (ret?1:0)` and
//!   raises `run_time_error` if not.
//!
//! On a rejected call the engine substitutes `ScriptFuncSet::get_err_return`
//! (`0x009d41d0`). We have **not** read that function; the shipped
//! `ron-data/paramtypes.xml` documents the pervasive `int_return` type as
//! "1 if true or success, 0 if false, -1 if failed", so we return `-1` for int
//! returns. `[inferred]`, and flagged as such in [`HostError::Unimplemented`]
//! handling so a differential run will catch it if it is wrong.

use crate::builtin_table::BuiltinDecl;
use crate::value::{ScriptTy, Value};
use std::collections::BTreeMap;

/// Why a builtin call did not produce a real value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    /// The host does not implement this builtin. The VM records it in
    /// [`Coverage`] and substitutes the engine's error return, so a script that
    /// calls an unimplemented builtin keeps running rather than aborting the frame
    /// — which is what makes coverage *measurable* over a whole replay instead of
    /// stopping at the first gap.
    Unimplemented,
    /// The host implements the builtin but rejected these arguments.
    BadArgs(&'static str),
}

pub type HostResult = Result<Value, HostError>;

/// The simulation-facing half of the script engine.
///
/// This is deliberately one method rather than 873. The arity and types are already
/// checked by the VM against [`BuiltinDecl`] before the call, so an implementation
/// can match on `decl.name` (stable) or `decl.index` (stable for this build) and
/// trust `args.len() == decl.arity`.
pub trait Host {
    fn call(&mut self, decl: &BuiltinDecl, args: &[Value]) -> HostResult;

    /// `rand_int(min, max)` — `Random::get(int,int)` (`0x00a39d70`) on the object at
    /// `[0x00c06184]`, which is `GameAccess::game_random`, **the main simulation
    /// stream** (307 call sites: map generation, units, animals, AI leaders, the
    /// pathfinder). A script that rolls dice perturbs exactly the sequence the rest
    /// of the project is trying to reproduce. Implementations must route this to the
    /// real stream, never to a private one.
    fn game_random(&mut self, _lo: i32, _hi: i32) -> i32 {
        0
    }

    /// Advance that same stream one step and return the new seed word.
    /// `rand_real` (`0x009e18b0`) inlines `s = s*1664525 + 1013904223` against it and
    /// then builds a float from the low 23 bits, so it must share the state with
    /// [`Host::game_random`] or the stream splits.
    fn game_random_step(&mut self) -> u32 {
        0
    }

    /// `rand_get_seed()` (`0x009e1930`) — a bare `mov eax, [[0x00c06184]]`.
    fn game_random_seed(&self) -> u32 {
        0
    }

    /// `rand_seed(n)` for `n >= 0` (`0x009e1900`).
    fn set_game_random_seed(&mut self, _s: u32) {}

    /// `rand_seed(n)` for `n < 0`: the engine calls the CRT import at
    /// `[0x00ac5460]` and installs whatever it returns.
    fn reseed_from_clock(&mut self) {}

    /// `print` / `print_line` (`0x00a048d0` / `0x00a04950`) — the script log.
    /// Capturing it is what makes a whole-VM differential trace against the live
    /// game possible at all.
    fn script_print(&mut self, _s: &str, _newline: bool) {}
}

/// Per-builtin call accounting, so "which builtins do we still owe?" is a
/// measurement rather than an estimate.
#[derive(Debug, Clone, Default)]
pub struct Coverage {
    implemented: BTreeMap<u32, u64>,
    unimplemented: BTreeMap<u32, (&'static str, u64)>,
}

impl Coverage {
    pub fn record_implemented(&mut self, index: u32) {
        *self.implemented.entry(index).or_insert(0) += 1;
    }

    pub fn record_unimplemented(&mut self, index: u32, name: &'static str) {
        let e = self.unimplemented.entry(index).or_insert((name, 0));
        e.1 += 1;
    }

    /// Builtins that were called and handled, with call counts.
    pub fn implemented(&self) -> impl Iterator<Item = (u32, u64)> + '_ {
        self.implemented.iter().map(|(k, v)| (*k, *v))
    }

    /// Builtins that were called and **not** handled, with names and call counts.
    /// This is the debt list; it is exact for whatever workload was run.
    pub fn unimplemented(&self) -> impl Iterator<Item = (u32, &'static str, u64)> + '_ {
        self.unimplemented.iter().map(|(k, (n, c))| (*k, *n, *c))
    }

    pub fn distinct_called(&self) -> usize {
        self.implemented.len() + self.unimplemented.len()
    }

    pub fn is_complete(&self) -> bool {
        self.unimplemented.is_empty()
    }

    /// A stable, sorted report suitable for pasting into a coverage doc.
    pub fn report(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let _ = writeln!(
            s,
            "builtins called: {} ({} implemented, {} missing)",
            self.distinct_called(),
            self.implemented.len(),
            self.unimplemented.len()
        );
        for (i, n, c) in self.unimplemented() {
            let _ = writeln!(s, "  MISSING  {i:>4}  {n:<40} x{c}");
        }
        s
    }
}

/// The engine's substitute value for a call that could not be made.
/// See the module docs for the provenance of `-1`.
pub fn err_return(ret: ScriptTy) -> Value {
    match ret {
        ScriptTy::Void => Value::Null,
        ScriptTy::Real => Value::Real(-1.0),
        ScriptTy::Str => Value::str(""),
        _ => Value::Int(-1),
    }
}

/// A host that implements nothing. Useful on its own: run a script against it and
/// [`Coverage`] tells you precisely which builtins that workload needs.
#[derive(Debug, Default)]
pub struct NullHost;

impl Host for NullHost {
    fn call(&mut self, _decl: &BuiltinDecl, _args: &[Value]) -> HostResult {
        Err(HostError::Unimplemented)
    }
}
