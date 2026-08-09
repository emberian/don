//! **don-bhs-cc** — a compiler for Big Huge Script, the scripting language shipped
//! with *Rise of Nations: Extended Edition*.
//!
//! Front end (lexer, parser, semantic analysis) plus a code generator that targets the
//! **engine's own opcode set**, recovered by the sibling engine lane and re-exported
//! here from [`don_bhs::opcode`]. There is no invented instruction set: the compiler
//! emits `OP_PUSH` / `OP_JUMP_IF_NOT` / `OP_CALL_GAME` with the byte values the retail
//! `VirtualMachine::execute_next` dispatch table defines.
//!
//! # Why the opcode set is not ours to choose
//!
//! `script_run_time` is channel 15 of the engine's 15 lockstep checksum channels.
//! `RunTimeEnv::walk_data` (`0x009c41a0`) was read at the instruction level for this
//! lane. The result, in two parts:
//!
//! * **No VM internals are hashed.** `RunTimeEnv::close` runs *first* and zeroes the
//!   frame stack, the operand stack, `cur_vm`, `bytecodes_executed` and
//!   `script_status`; `VirtualMachine` has no walker at all. Program counter, operand
//!   stack and locals never enter the checksum. **Execution order and instruction
//!   selection are therefore not observable through this channel.**
//! * **The compiled image *is* hashed.** `ScriptFile::walk_data` (`0x009c63b0`) walks
//!   `code` (the raw bytecode bytes), `const_pool` and `linked_files`, and
//!   `Script::walk_data` (`0x009c5f30`) walks `static_vars`, `trigger_bits`, `params`,
//!   `refs`, the three name tables, `name`, `offset`, `return_type` and `script_type`.
//!
//! So a *running* game is insensitive to how we compile, but the channel-15 word for a
//! scripted match is a function of the compiled bytes. Two honest positions follow, and
//! this crate is built for both: run our own bytecode and accept a constant offset in
//! channel 15, or drive retail's compiler for the image and use ours for everything
//! else. Byte-identity with retail is a **measurable** goal, not an assumed one — it
//! needs reference output from the retail compiler to diff against, which is the
//! sibling oracle lane's deliverable.
//!
//! # Status
//!
//! See `docs/tracks/bhs-compiler.md` for the parse rate over the 363-file shipped
//! corpus and the honest list of what semantic analysis does and does not check.

pub mod ast;
pub mod codegen;
pub mod lex;
pub mod parse;
pub mod sema;

pub use lex::{lex, LexError, Pos};
pub use parse::{parse_file, ParseError};

/// Read a `.bhs` file from disk and parse it.
///
/// Handles the one shipped file that is not valid UTF-8 (`leipzigsetup.bhs`) via the
/// Latin-1 fallback in [`lex::decode_source`].
pub fn parse_path(path: &std::path::Path) -> Result<ast::SourceFile, CompileError> {
    let bytes = std::fs::read(path)
        .map_err(|e| CompileError::Io(path.display().to_string(), e.to_string()))?;
    let src = lex::decode_source(&bytes);
    parse_file(&path.display().to_string(), &src).map_err(CompileError::Parse)
}

#[derive(Debug)]
pub enum CompileError {
    Io(String, String),
    Parse(ParseError),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileError::Io(p, e) => write!(f, "{p}: {e}"),
            CompileError::Parse(e) => write!(f, "{e}"),
        }
    }
}
