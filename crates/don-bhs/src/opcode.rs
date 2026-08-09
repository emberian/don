//! The BHS opcode set.
//!
//! The names and numeric values are the `OpCodeTypes` enum **recovered whole from
//! `ron-bin/sbl/rise.pdb`** (`LF_ENUM 0x3C85`, field list `0x3C84`): 73 enumerators,
//! `OP_ASSIGN = 0` .. `OP_ERROR_TOKEN = 72`, `NUM_OP_CODES = 73`. [measured]
//!
//! Operand counts come from the *dispatch table* of `VirtualMachine::execute_next`
//! (`0x009e0840`), read at the instruction level rather than from decompiled C:
//! a byte index table at `0x009e167c` maps opcode -> case group, and a dword target
//! table at `0x009e15e0` maps case group -> handler VA (39 distinct groups). Each
//! handler's `add dword ptr [esi + 0xc], 4` sequence gives its operand count, where
//! `esi + 0xc` is `VirtualMachine::bip`. [measured]
//!
//! Encoding: **one byte of opcode, followed by 0, 1 or 2 little-endian 32-bit
//! operands, unaligned.** `RunTimeEnv::exec` (`0x009c3600`) fetches exactly one byte
//! (`movzx` of `code[bip]`), increments `bip`, and dispatches; every operand read in
//! `execute_next` is a `dword ptr [code + bip]` followed by `bip += 4`.

/// Number of opcodes the engine defines (`NUM_OP_CODES` in the PDB enum).
pub const NUM_OP_CODES: usize = 73;

/// What an operand *means*, which determines how a disassembler should print it and
/// how the VM should decode it. Recovered from `OpArg::write_arg` (`0x009c27d0`) and
/// the individual `execute_next` handlers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperandKind {
    /// No operand.
    None,
    /// A *variable reference* — see [`crate::vm::VarRef`]. Tagged: `0x20000000` =
    /// constant pool, `0x40000000` = script static, otherwise a local frame slot.
    VarRef,
    /// An index into `ScriptFile::const_pool`, with the `0x20000000` tag masked off.
    ConstIndex,
    /// An absolute byte offset into the code array (a jump target).
    CodeOffset,
    /// An index into `ScriptFile::scripts` (a script-to-script call).
    ScriptIndex,
    /// An index into `ScriptFile::linked_files` (an `include`d file).
    FileIndex,
    /// An index into the engine's registered builtin table (`OP_CALL_GAME`).
    BuiltinIndex,
    /// A literal argument count (`OP_CALL_GAME_VARIED`).
    ArgCount,
    /// A trigger index within `Script::trigger_bits`.
    TriggerIndex,
    /// A `SymType` id (a type tag such as `0x57bad` = int).
    TypeTag,
    /// An opaque 32-bit immediate whose exact meaning we have not pinned down.
    Raw,
}

/// A recovered opcode definition.
#[derive(Debug, Clone, Copy)]
pub struct OpDef {
    pub code: u8,
    pub name: &'static str,
    pub operands: &'static [OperandKind],
    /// VA of this opcode's handler inside `VirtualMachine::execute_next`. Opcodes
    /// sharing a handler share semantics (e.g. all 19 binary operators).
    pub handler_va: u32,
}

use OperandKind::*;

/// The complete opcode table. Order is the enum's numeric order, so
/// `OPCODES[n].code == n`.
pub static OPCODES: [OpDef; NUM_OP_CODES] = [
    // ---- assignment family: one handler, 0x009e10e9. pops rhs then lhs, calls
    // ScriptType::do_operator(op, rhs) on the lhs, pushes nothing. No operands.
    OpDef {
        code: 0x00,
        name: "OP_ASSIGN",
        operands: &[],
        handler_va: 0x009e10e9,
    },
    // ---- binary operators: one handler, 0x009e11e5. pops rhs then lhs,
    // pushes lhs.do_operator(op, rhs).
    OpDef {
        code: 0x01,
        name: "OP_EQ_OP",
        operands: &[],
        handler_va: 0x009e11e5,
    },
    OpDef {
        code: 0x02,
        name: "OP_NE_OP",
        operands: &[],
        handler_va: 0x009e11e5,
    },
    OpDef {
        code: 0x03,
        name: "OP_ADD_ASSIGN",
        operands: &[],
        handler_va: 0x009e10e9,
    },
    OpDef {
        code: 0x04,
        name: "OP_ADD",
        operands: &[],
        handler_va: 0x009e11e5,
    },
    OpDef {
        code: 0x05,
        name: "OP_LESS",
        operands: &[],
        handler_va: 0x009e11e5,
    },
    OpDef {
        code: 0x06,
        name: "OP_GREA",
        operands: &[],
        handler_va: 0x009e11e5,
    },
    OpDef {
        code: 0x07,
        name: "OP_LE_OP",
        operands: &[],
        handler_va: 0x009e11e5,
    },
    OpDef {
        code: 0x08,
        name: "OP_GE_OP",
        operands: &[],
        handler_va: 0x009e11e5,
    },
    // ---- unary operators: one handler, 0x009e11bd. pops one, pushes do_operator(op, null).
    OpDef {
        code: 0x09,
        name: "OP_UNA_NOT",
        operands: &[],
        handler_va: 0x009e11bd,
    },
    OpDef {
        code: 0x0a,
        name: "OP_AND_OP",
        operands: &[],
        handler_va: 0x009e11e5,
    },
    OpDef {
        code: 0x0b,
        name: "OP_OR_OP",
        operands: &[],
        handler_va: 0x009e11e5,
    },
    OpDef {
        code: 0x0c,
        name: "OP_MUL_ASSIGN",
        operands: &[],
        handler_va: 0x009e10e9,
    },
    OpDef {
        code: 0x0d,
        name: "OP_DIV_ASSIGN",
        operands: &[],
        handler_va: 0x009e10e9,
    },
    OpDef {
        code: 0x0e,
        name: "OP_SUB_ASSIGN",
        operands: &[],
        handler_va: 0x009e10e9,
    },
    OpDef {
        code: 0x0f,
        name: "OP_MUL",
        operands: &[],
        handler_va: 0x009e11e5,
    },
    OpDef {
        code: 0x10,
        name: "OP_DIV",
        operands: &[],
        handler_va: 0x009e11e5,
    },
    OpDef {
        code: 0x11,
        name: "OP_SUBT",
        operands: &[],
        handler_va: 0x009e11e5,
    },
    // ---- pre-inc/dec: handler 0x009e1127. peeks the *next opcode byte* and if it is
    // OP_POP (0x27) swallows it (statement-position `i++`).
    OpDef {
        code: 0x12,
        name: "OP_INC_OP",
        operands: &[],
        handler_va: 0x009e1127,
    },
    OpDef {
        code: 0x13,
        name: "OP_DEC_OP",
        operands: &[],
        handler_va: 0x009e1127,
    },
    // ---- post-inc/dec: handler 0x009e1159. same OP_POP peek, else duplicates first.
    OpDef {
        code: 0x14,
        name: "OP_INC_OP_POST",
        operands: &[],
        handler_va: 0x009e1159,
    },
    OpDef {
        code: 0x15,
        name: "OP_DEC_OP_POST",
        operands: &[],
        handler_va: 0x009e1159,
    },
    OpDef {
        code: 0x16,
        name: "OP_UNA_NEGA",
        operands: &[],
        handler_va: 0x009e11bd,
    },
    OpDef {
        code: 0x17,
        name: "OP_POW_OP",
        operands: &[],
        handler_va: 0x009e11e5,
    },
    OpDef {
        code: 0x18,
        name: "OP_POW_ASSIGN",
        operands: &[],
        handler_va: 0x009e10e9,
    },
    OpDef {
        code: 0x19,
        name: "OP_MOD_ASSIGN",
        operands: &[],
        handler_va: 0x009e10e9,
    },
    OpDef {
        code: 0x1a,
        name: "OP_LEFT_ASSIGN",
        operands: &[],
        handler_va: 0x009e10e9,
    },
    OpDef {
        code: 0x1b,
        name: "OP_RIGHT_ASSIGN",
        operands: &[],
        handler_va: 0x009e10e9,
    },
    OpDef {
        code: 0x1c,
        name: "OP_AND_ASSIGN",
        operands: &[],
        handler_va: 0x009e10e9,
    },
    OpDef {
        code: 0x1d,
        name: "OP_XOR_ASSIGN",
        operands: &[],
        handler_va: 0x009e10e9,
    },
    OpDef {
        code: 0x1e,
        name: "OP_OR_ASSIGN",
        operands: &[],
        handler_va: 0x009e10e9,
    },
    OpDef {
        code: 0x1f,
        name: "OP_MOD",
        operands: &[],
        handler_va: 0x009e11e5,
    },
    OpDef {
        code: 0x20,
        name: "OP_LEFT_OP",
        operands: &[],
        handler_va: 0x009e11e5,
    },
    OpDef {
        code: 0x21,
        name: "OP_RIGHT_OP",
        operands: &[],
        handler_va: 0x009e11e5,
    },
    OpDef {
        code: 0x22,
        name: "OP_AND_BIT",
        operands: &[],
        handler_va: 0x009e11e5,
    },
    OpDef {
        code: 0x23,
        name: "OP_XOR_BIT",
        operands: &[],
        handler_va: 0x009e11e5,
    },
    OpDef {
        code: 0x24,
        name: "OP_OR_BIT",
        operands: &[],
        handler_va: 0x009e11e5,
    },
    OpDef {
        code: 0x25,
        name: "OP_UNA_TILD",
        operands: &[],
        handler_va: 0x009e11bd,
    },
    // ---- stack / variable access
    OpDef {
        code: 0x26,
        name: "OP_PUSH",
        operands: &[VarRef],
        handler_va: 0x009e09e1,
    },
    OpDef {
        code: 0x27,
        name: "OP_POP",
        operands: &[],
        handler_va: 0x009e0a30,
    },
    // ---- construction
    OpDef {
        code: 0x28,
        name: "OP_CREATE_SIMPLE",
        operands: &[TypeTag],
        handler_va: 0x009e0a64,
    },
    OpDef {
        code: 0x29,
        name: "OP_CREATE_ARRAY",
        operands: &[TypeTag],
        handler_va: 0x009e0aa4,
    },
    OpDef {
        code: 0x2a,
        name: "OP_CREATE_ARRAY_DYN",
        operands: &[TypeTag],
        handler_va: 0x009e0ac2,
    },
    OpDef {
        code: 0x2b,
        name: "OP_CREATE_ARRAY_INITER",
        operands: &[Raw, Raw],
        handler_va: 0x009e0b47,
    },
    OpDef {
        code: 0x2c,
        name: "OP_CREATE_STRUCT",
        operands: &[Raw, Raw],
        handler_va: 0x009e0b67,
    },
    // ---- aggregate access
    OpDef {
        code: 0x2d,
        name: "OP_PUSH_ARRAY_INDEX",
        operands: &[],
        handler_va: 0x009e0b87,
    },
    OpDef {
        code: 0x2e,
        name: "OP_CREATE_ARRAY_INDEX",
        operands: &[],
        handler_va: 0x009e0cc3,
    },
    OpDef {
        code: 0x2f,
        name: "OP_PUSH_STRUCT_FIELD",
        operands: &[Raw],
        handler_va: 0x009e0dc5,
    },
    OpDef {
        code: 0x30,
        name: "OP_PUSH_ARRAY_LENGTH",
        operands: &[],
        handler_va: 0x009e0e95,
    },
    OpDef {
        code: 0x31,
        name: "OP_SET_ARRAY_LENGTH",
        operands: &[],
        handler_va: 0x009e0f0c,
    },
    // ---- initialisation: writes a variable slot via VirtualMachine::set_value
    OpDef {
        code: 0x32,
        name: "OP_INIT_COPY",
        operands: &[VarRef],
        handler_va: 0x009e0f62,
    },
    OpDef {
        code: 0x33,
        name: "OP_INIT",
        operands: &[VarRef],
        handler_va: 0x009e0f99,
    },
    OpDef {
        code: 0x34,
        name: "OP_CAST",
        operands: &[TypeTag],
        handler_va: 0x009e1597,
    },
    OpDef {
        code: 0x35,
        name: "OP_CAST_BOOL",
        operands: &[],
        handler_va: 0x009e14e8,
    },
    // ---- calls
    OpDef {
        code: 0x36,
        name: "OP_CALL",
        operands: &[ScriptIndex],
        handler_va: 0x009e12e3,
    },
    OpDef {
        code: 0x37,
        name: "OP_CALL_INCLUDE",
        operands: &[ScriptIndex, FileIndex],
        handler_va: 0x009e130f,
    },
    OpDef {
        code: 0x38,
        name: "OP_CALL_GAME",
        operands: &[BuiltinIndex],
        handler_va: 0x009e1343,
    },
    OpDef {
        code: 0x39,
        name: "OP_CALL_GAME_VARIED",
        operands: &[BuiltinIndex, ArgCount],
        handler_va: 0x009e136c,
    },
    // ---- switch/case, debugger breakpoint
    OpDef {
        code: 0x3a,
        name: "OP_CASE",
        operands: &[ConstIndex, CodeOffset],
        handler_va: 0x009e1259,
    },
    OpDef {
        code: 0x3b,
        name: "OP_BREAK",
        operands: &[],
        handler_va: 0x009e087e,
    },
    // ---- trigger enable bits. NOTE the machine semantics are the OPPOSITE of the
    // enum names: OP_BIT_SET uses `btr` (CLEARS), OP_BIT_UNSET uses `bts` (SETS).
    OpDef {
        code: 0x3c,
        name: "OP_BIT_SET",
        operands: &[TriggerIndex],
        handler_va: 0x009e1022,
    },
    OpDef {
        code: 0x3d,
        name: "OP_BIT_UNSET",
        operands: &[TriggerIndex],
        handler_va: 0x009e105f,
    },
    // ---- control flow
    OpDef {
        code: 0x3e,
        name: "OP_RETURN",
        operands: &[],
        handler_va: 0x009e1545,
    },
    OpDef {
        code: 0x3f,
        name: "OP_JUMP",
        operands: &[CodeOffset],
        handler_va: 0x009e14c9,
    },
    OpDef {
        code: 0x40,
        name: "OP_JUMP_IF",
        operands: &[CodeOffset],
        handler_va: 0x009e139d,
    },
    OpDef {
        code: 0x41,
        name: "OP_JUMP_IF_NOT",
        operands: &[CodeOffset],
        handler_va: 0x009e13e5,
    },
    OpDef {
        code: 0x42,
        name: "OP_JUMP_IF_SC_TRUE",
        operands: &[CodeOffset],
        handler_va: 0x009e142d,
    },
    OpDef {
        code: 0x43,
        name: "OP_JUMP_IF_SC_FALSE",
        operands: &[CodeOffset],
        handler_va: 0x009e1477,
    },
    OpDef {
        code: 0x44,
        name: "OP_JUMP_IF_INITED",
        operands: &[VarRef, CodeOffset],
        handler_va: 0x009e0fca,
    },
    OpDef {
        code: 0x45,
        name: "OP_JUMP_IF_BITSET",
        operands: &[TriggerIndex, CodeOffset],
        handler_va: 0x009e109c,
    },
    // ---- markers. OP_MARKER emits NO bytes at all (OpCode::count_code skips it);
    // OP_SCRIPT_MARKER emits one byte plus a 4-byte operand and is a runtime no-op.
    OpDef {
        code: 0x46,
        name: "OP_MARKER",
        operands: &[],
        handler_va: 0x009e0934,
    },
    OpDef {
        code: 0x47,
        name: "OP_SCRIPT_MARKER",
        operands: &[Raw],
        handler_va: 0x009e0f82,
    },
    // ---- never emitted: OpCode::write_op raises a compile error on this.
    OpDef {
        code: 0x48,
        name: "OP_ERROR_TOKEN",
        operands: &[],
        handler_va: 0,
    },
];

/// Look up an opcode definition. Returns `None` for bytes above `OP_ERROR_TOKEN`,
/// which is exactly what `execute_next` treats as a fatal `run_time_error`
/// (`cmp edi, 0x47 / ja` at `0x009e0863`).
#[inline]
pub fn decode(byte: u8) -> Option<&'static OpDef> {
    OPCODES.get(byte as usize)
}

/// Total encoded size of an instruction: 1 opcode byte + 4 bytes per operand.
#[inline]
pub fn encoded_len(byte: u8) -> Option<usize> {
    decode(byte).map(|d| 1 + 4 * d.operands.len())
}

// Named constants for the opcodes the VM special-cases.
pub const OP_POP: u8 = 0x27;
pub const OP_EQ_OP: u8 = 0x01;
pub const OP_ERROR_TOKEN: u8 = 0x48;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_dense_and_ordered() {
        for (i, d) in OPCODES.iter().enumerate() {
            assert_eq!(d.code as usize, i, "opcode table out of order at {i}");
        }
        assert_eq!(OPCODES.len(), NUM_OP_CODES);
    }

    #[test]
    fn operand_counts_match_recovered_handlers() {
        // Spot-checks against the instruction-level reading of execute_next.
        assert_eq!(encoded_len(0x38), Some(5)); // OP_CALL_GAME: index only
        assert_eq!(encoded_len(0x39), Some(9)); // OP_CALL_GAME_VARIED: index + argc
        assert_eq!(encoded_len(0x44), Some(9)); // OP_JUMP_IF_INITED: ref + target
        assert_eq!(encoded_len(0x3f), Some(5)); // OP_JUMP
        assert_eq!(encoded_len(0x00), Some(1)); // OP_ASSIGN
        assert_eq!(encoded_len(0x46), Some(1)); // OP_MARKER (never emitted anyway)
    }

    #[test]
    fn all_binary_operators_share_one_handler() {
        let h = OPCODES[0x04].handler_va;
        for op in [0x01u8, 0x02, 0x05, 0x06, 0x0a, 0x0f, 0x10, 0x24] {
            assert_eq!(OPCODES[op as usize].handler_va, h);
        }
    }
}
