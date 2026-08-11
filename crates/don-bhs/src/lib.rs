//! # `don-bhs` — the Big Huge Script engine
//!
//! A Rust reimplementation of the **bytecode virtual machine** that Rise of Nations:
//! Extended Edition uses to run `.bhs` scripts, derived from `ron-bin/riseofnations.exe`
//! and its matching private PDB.
//!
//! ## Why this crate exists
//!
//! `script_run_time` is one of the engine's sixteen lockstep checksum channels
//! (`CheckSums::check_script_run_time`; the object is `RunTimeEnv script_run_time`
//! at `0x00ebeeb0`). `RunTimeEnv::walk_data` and `Script::walk_data` put script
//! state on the same `DataWalk` interface as `CheckSum` / `SaveGame` / `LoadGame`,
//! so **script state is sim-critical state**: no scripted game can be validated
//! against a replay without a faithful interpreter. It is also the gate on running
//! the real shipped opening-book scripts rather than a hand transcription, and on
//! the entire Steam Workshop mod library.
//!
//! ## The decomposition
//!
//! BHS is **source -> bytecode -> stack VM**, not a tree-walking interpreter. That
//! splits the work three ways, with very different costs:
//!
//! 1. **The compiler** (`Compiler`, `Lexer`, `SyntaxNode`, `SymTable`, `OpCode`)
//!    runs once, offline, outside the tick. It never needs reimplementing — it can
//!    be *borrowed* by driving retail's own `Compiler::compile` under the oracle.
//! 2. **The VM** must be ours: it runs inside `Game::do_frame`, once per frame, and
//!    its state is checksummed. That is this crate.
//! 3. **The builtins** must be ours, but they are the boundary to our simulation
//!    anyway, so that work is owed regardless of who wrote the evaluator. See
//!    [`host`].
//!
//! ## Provenance and fidelity
//!
//! Everything here is static analysis. Layouts, the opcode enum, operand counts and
//! the builtin table are `[measured]` — read out of the PDB or off the instruction
//! stream, with the specific address cited at each site. The *arithmetic* in
//! [`ops`] is the weak point and is marked as such: it has never been run against
//! the retail evaluator. Nothing in this crate is verified, proven or a refinement.
//!
//! ## Quick start
//!
//! ```
//! use don_bhs::{disasm::{asm, asm_len}, program::*, value::Value, vm::{Vm, VarRef}, host::NullHost};
//!
//! // static int n = 0;  n = n + 1;  return n;
//! // The `static` initialiser is guarded by OP_JUMP_IF_INITED, whose second
//! // operand is the absolute byte offset just past the initialiser.
//! let prologue: &[(u8, &[u32])] = &[
//!     (0x44, &[VarRef::Static(0).encode(), 0]),  // OP_JUMP_IF_INITED (target patched)
//!     (0x26, &[VarRef::Const(0).encode()]),      // OP_PUSH const 0
//!     (0x33, &[VarRef::Static(0).encode()]),     // OP_INIT static[0]
//! ];
//! let body_at = asm_len(prologue) as u32;
//! let code = asm(&[
//!     (0x44, &[VarRef::Static(0).encode(), body_at]),
//!     (0x26, &[VarRef::Const(0).encode()]),
//!     (0x33, &[VarRef::Static(0).encode()]),
//!     (0x26, &[VarRef::Static(0).encode()]),     // OP_PUSH static[0]   <- body_at
//!     (0x26, &[VarRef::Const(1).encode()]),      // OP_PUSH const 1
//!     (0x04, &[]),                               // OP_ADD
//!     (0x33, &[VarRef::Static(0).encode()]),     // OP_INIT static[0]
//!     (0x26, &[VarRef::Static(0).encode()]),     // OP_PUSH static[0]
//!     (0x3e, &[]),                               // OP_RETURN
//! ]);
//! let mut prog = Program::single(ScriptFile {
//!     code,
//!     const_pool: vec![Value::Int(0), Value::Int(1)],
//!     scripts: vec![Script { name: "tick".into(), statics: vec![None; 1], ..Default::default() }],
//!     ..Default::default()
//! });
//! let mut host = NullHost;
//! let mut vm = Vm::new(&mut prog, &mut host);
//! // The static persists across calls, exactly as it does across frames in-game.
//! assert!(matches!(vm.run_script(0, "tick").unwrap().returned, Some(Value::Int(1))));
//! assert!(matches!(vm.run_script(0, "tick").unwrap().returned, Some(Value::Int(2))));
//! ```

pub mod builtin_table;
pub mod builtins;
pub mod chunk;
pub mod corpus;
pub mod disasm;
pub mod host;
pub mod opcode;
pub mod ops;
pub mod program;
pub mod scenario;
pub mod value;
pub mod vm;

pub use builtin_table::{builtin, find_builtin, BuiltinDecl, BUILTINS, BUILTIN_COUNT};
pub use builtins::{call_util, UtilHost};
pub use corpus::{scan_dir, Census};
pub use host::{Coverage, Host, HostError, HostResult, NullHost};
pub use program::{Program, Script, ScriptFile};
pub use scenario::{
    call_scenario, GameImage, ObjectImage, ObjectProbe, ScenarioHost, ScenarioWorldImage,
    ScriptTimers, Timer,
};
pub use value::{Obj, ScriptTy, Value};
pub use vm::{MissingBuiltinPolicy, RunOutcome, RuntimeError, VarRef, Vm, VmError};

/// The `.bhs` entry-point convention for the *game script* slot.
///
/// `SetupWin::set_script_path` (`0x005b7aa0`) stores the selected file's base name
/// with the extension stripped in `Game+0x500`, and `Game::do_frame` (`0x00591ef0`)
/// passes that same string to `run_script` as the *script name*. So the entry
/// function's name equals the file's base name and takes zero parameters.
pub fn entry_point_name(file_stem: &str) -> &str {
    file_stem
}
