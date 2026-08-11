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
//! - Arguments are already on the **shared run stack**, pushed right to left, so
//!   logical parameter 0 is on top. This order is captured in shipped-compiler output
//!   for both script calls and native `char_at("abc", 1)`.
//! - `nargs < 0` (which is how `OP_CALL_GAME` encodes "default") is replaced by
//!   `ScriptFunc::params.count`, i.e. the declared arity.
//! - The handler receives a `ScriptParamStack { Stack* stack; int num_params;
//!   int orig_stack_size; }` and consumes all `num_params` entries.
//! - The return value is pushed back **only if** `func->return_type != 0x84048`
//!   (void). The VM then asserts the stack ended at `orig - nargs + (ret?1:0)` and
//!   raises `run_time_error` if not.
//!
//! On a call rejected by the retail function set, the engine substitutes
//! `ScriptFuncSet::get_err_return` (`0x009d41d0`): `-1`, `-1.0`, an empty string,
//! null for void, or an empty object according to the declared return type.
//! That retail rejection path is **not** permission to conceal an incomplete DoN
//! host. [`Vm`](crate::vm::Vm) therefore fails on [`HostError::Unimplemented`] by
//! default; substitution exists only in an explicitly selected coverage-survey mode.

use crate::builtin_table::BuiltinDecl;
use crate::value::{ScriptTy, Value};
use std::collections::BTreeMap;

/// Why a builtin call did not produce a real value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    /// The host does not implement this builtin. The VM records it in
    /// [`Coverage`]. Strict execution then returns a `VmError`; the explicitly
    /// lossy survey policy may substitute the retail rejected-call value so a
    /// workload can discover more than its first missing builtin.
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
    fn game_random(&mut self, _lo: i32, _hi: i32) -> Result<i32, HostError> {
        Err(HostError::Unimplemented)
    }

    /// Advance that same stream one step and return the new seed word.
    /// `rand_real` (`0x009e18b0`) inlines `s = s*1664525 + 1013904223` against it and
    /// then builds a float from the low 23 bits, so it must share the state with
    /// [`Host::game_random`] or the stream splits.
    fn game_random_step(&mut self) -> Result<u32, HostError> {
        Err(HostError::Unimplemented)
    }

    /// `rand_get_seed()` (`0x009e1930`) — a bare `mov eax, [[0x00c06184]]`.
    fn game_random_seed(&self) -> Result<u32, HostError> {
        Err(HostError::Unimplemented)
    }

    /// `rand_seed(n)` for `n >= 0` (`0x009e1900`).
    fn set_game_random_seed(&mut self, _s: u32) -> Result<(), HostError> {
        Err(HostError::Unimplemented)
    }

    /// `rand_seed(n)` for `n < 0`: the engine calls the CRT import at
    /// `[0x00ac5460]` and installs whatever it returns.
    fn reseed_from_clock(&mut self) -> Result<(), HostError> {
        Err(HostError::Unimplemented)
    }

    /// `print` / `print_line` (`0x00a048d0` / `0x00a04950`) — the script log.
    /// Capturing it is what makes a whole-VM differential trace against the live
    /// game possible at all.
    fn script_print(&mut self, _s: &str, _newline: bool) -> Result<(), HostError> {
        Err(HostError::Unimplemented)
    }

    // -----------------------------------------------------------------------
    // `ScenarioFuncSet` reads. See [`crate::scenario`] for the handlers that use
    // them; every one names the exact retail address and field it stands for, and
    // every default refuses rather than answering.
    // -----------------------------------------------------------------------

    /// `Game::tick` (`Game+0x560`), which counts **game seconds**. `time_sec`
    /// (`0x009ead40`) is twelve bytes that return it verbatim, and `set_timer`
    /// (`0x009e4bc0`) adds its `seconds` argument to it.
    fn game_seconds(&mut self) -> Result<i32, HostError> {
        Err(HostError::Unimplemented)
    }

    /// `GameInfo::flags` (`Game+0x20`, i.e. `GameInfo+0x14`).
    /// `get_is_no_nation_powers` (`0x009e5230`) reads bit 2 of its low byte.
    fn game_info_flags(&mut self) -> Result<u32, HostError> {
        Err(HostError::Unimplemented)
    }

    /// `GameInfo::rush_rules` (`Game+0x32`), a `RushRulesIndex`.
    fn game_info_rush_rules(&mut self) -> Result<u8, HostError> {
        Err(HostError::Unimplemented)
    }

    /// `GameInfo::victory` (`Game+0x38`), a `VictoryIndex`. All ten `is_victory_*`
    /// builtins are a `cmp`/`sete` against a constant on this one byte.
    fn game_info_victory(&mut self) -> Result<u8, HostError> {
        Err(HostError::Unimplemented)
    }

    /// `Game::semaphore` (`BitMask<256>` at `Game+0x814`; the bit bytes start at
    /// `Game+0x820`).
    fn game_semaphore_bit(&mut self, _bit: u32) -> Result<bool, HostError> {
        Err(HostError::Unimplemented)
    }

    /// `ScenarioData::timers` (`0x00ed6650`) — script-engine state, so the container
    /// itself is implemented in [`crate::scenario::ScriptTimers`] and a host only has
    /// to own one.
    fn script_timers(&mut self) -> Result<&mut crate::scenario::ScriptTimers, HostError> {
        Err(HostError::Unimplemented)
    }

    /// `[0x00cc2214]`, the file-static object cursor `find_unit` (`0x009ebe10`) reads,
    /// clamps at zero and writes back on every hit.
    fn find_unit_cursor(&mut self) -> Result<i32, HostError> {
        Err(HostError::Unimplemented)
    }

    fn set_find_unit_cursor(&mut self, _v: i32) -> Result<(), HostError> {
        Err(HostError::Unimplemented)
    }

    /// The first dword of `Leaders[who0]` (`0x00e3a390`, stride `0x6eec`).
    /// See [`crate::scenario::leader_flag`].
    fn leader_flags(&mut self, _who0: i32) -> Result<u32, HostError> {
        Err(HostError::Unimplemented)
    }

    /// `ScenarioFuncSet::get_type_index(name, 0)` (`0x00a03480`): the index of the first
    /// of the 806 type records whose `TypeData+0x60` name matches, or `-1`.
    fn type_index_by_name(&mut self, _name: &str) -> Result<i32, HostError> {
        Err(HostError::Unimplemented)
    }

    /// `TypeData::is_unit_type`, which `find_unit` reaches through the type vtable at
    /// `+0x0c` and which the compiler devirtualises at `0x009ebe5d` into
    /// `0x32 <= TypeData+4 < 0x19e`.
    fn type_is_unit_type(&mut self, _type_index: i32) -> Result<bool, HostError> {
        Err(HostError::Unimplemented)
    }

    /// `Objects+0x15c+who0*4` — the high-water count of the owner's `Units::lists` band
    /// (`0x00c0aec0`), which is what `0x009e2850` wraps against.
    fn unit_band_count(&mut self, _who0: i32) -> Result<i32, HostError> {
        Err(HostError::Unimplemented)
    }

    /// One slot of `Units::lists[who0]`, by band index rather than by object handle.
    fn unit_band_probe(
        &mut self,
        _who0: i32,
        _idx: i32,
    ) -> Result<crate::scenario::ObjectProbe, HostError> {
        Err(HostError::Unimplemented)
    }

    /// `SubObject::is(type_index, 0)` (vtable `+0xb8`) on a `Units::lists` slot.
    fn unit_band_is_type(
        &mut self,
        _who0: i32,
        _idx: i32,
        _type_index: i32,
    ) -> Result<bool, HostError> {
        Err(HostError::Unimplemented)
    }

    /// One slot of `Objects::lists[who0]` (`Objects+0x14`, stride `0x1c`), by handle.
    fn object_band_probe(
        &mut self,
        _who0: i32,
        _o: i32,
    ) -> Result<crate::scenario::ObjectProbe, HostError> {
        Err(HostError::Unimplemented)
    }

    /// `SubObject::is(type_index, 0)` on an `Objects::lists` handle.
    fn object_band_is_type(
        &mut self,
        _who0: i32,
        _o: i32,
        _type_index: i32,
    ) -> Result<bool, HostError> {
        Err(HostError::Unimplemented)
    }

    /// `SubObjectData::x_internal` / `y_internal` **as stored**, i.e. still XORed with
    /// `0x00063637`. `bubble_text_obj` (`0x009ff58b`) un-XORs them itself.
    fn object_band_position_internal(
        &mut self,
        _who0: i32,
        _o: i32,
    ) -> Result<(i32, i32), HostError> {
        Err(HostError::Unimplemented)
    }

    /// `GroupData+0x4a` of the local console's select group
    /// (`[0x00e8d444] + Console+0x2a0 * 0x9f0`), zero-based.
    fn selection_group_owner(&mut self) -> Result<i32, HostError> {
        Err(HostError::Unimplemented)
    }

    /// The `short[]` member handles at `GroupData+0x8cc`, `GroupData+0xc` of them.
    fn selection_group_members(&mut self) -> Result<Vec<i32>, HostError> {
        Err(HostError::Unimplemented)
    }

    /// `Console+0x298`, the local display player.
    fn local_display_player(&mut self) -> Result<i32, HostError> {
        Err(HostError::Unimplemented)
    }

    /// `MessageWin::add_bubble_message` (`0x007e7e20`). Presentation: a receipt, not
    /// simulation state.
    fn add_bubble_message(
        &mut self,
        _text: &str,
        _x: i32,
        _y: i32,
        _who: i32,
    ) -> Result<(), HostError> {
        Err(HostError::Unimplemented)
    }
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

/// The engine's substitute value for a call that retail rejected.
///
/// Private to the VM's opt-in coverage survey: it must never make an incomplete
/// host look like a successful fidelity run.
pub(crate) fn survey_err_return(ret: ScriptTy) -> Value {
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
