//! Bytecode disassembler.
//!
//! This exists for one specific reason beyond debugging: retail's own compiler can
//! emit a symbolic listing to `bhs.log` (`OpCode::write_code`, `0x009c2d90`, writes
//! `"%d - %d %s%s\n"` = source line, code offset, op name, arg text, whenever its
//! `String` directory argument is non-empty). **That listing is the reference our
//! disassembler is checked against** — same opcode names, same offsets — which
//! makes it the cheapest possible differential test of the decode layer, requiring
//! no simulation state at all.
//!
//! Also included is a tiny assembler ([`asm`]) so the VM can be tested before the
//! retail compiler is available.

use crate::opcode::{decode, OperandKind};
use crate::vm::VarRef;

/// One decoded instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Insn {
    pub offset: usize,
    pub op: u8,
    pub name: &'static str,
    pub operands: Vec<u32>,
    pub len: usize,
}

/// Decode a whole code array linearly.
///
/// Linear sweep is correct here because the compiler emits a contiguous stream and
/// all jump targets are absolute byte offsets into it; there is no interleaved data.
pub fn disassemble(code: &[u8]) -> Result<Vec<Insn>, String> {
    let mut out = Vec::new();
    let mut pc = 0usize;
    while pc < code.len() {
        let op = code[pc];
        let d = decode(op).ok_or_else(|| format!("bad opcode {op:#04x} at {pc}"))?;
        let n = d.operands.len();
        if pc + 1 + 4 * n > code.len() {
            return Err(format!("truncated operand for {} at {pc}", d.name));
        }
        let mut ops = Vec::with_capacity(n);
        for i in 0..n {
            let b = &code[pc + 1 + 4 * i..pc + 5 + 4 * i];
            ops.push(u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        }
        out.push(Insn {
            offset: pc,
            op,
            name: d.name,
            operands: ops,
            len: 1 + 4 * n,
        });
        pc += 1 + 4 * n;
    }
    Ok(out)
}

/// Render one instruction in a form close to retail's `bhs.log` line.
pub fn format_insn(i: &Insn) -> String {
    let d = decode(i.op).unwrap();
    let mut s = format!("{:>6}  {}", i.offset, i.name);
    for (k, v) in i.operands.iter().enumerate() {
        let kind = d.operands.get(k).copied().unwrap_or(OperandKind::Raw);
        s.push(' ');
        s.push_str(&match kind {
            OperandKind::VarRef => match VarRef::decode(*v) {
                VarRef::Const(n) => format!("const[{n}]"),
                VarRef::Static(n) => format!("static[{n}]"),
                VarRef::Local(n) => format!("local[{n}]"),
            },
            OperandKind::ConstIndex => format!("const[{}]", v & 0xdfff_ffff),
            OperandKind::CodeOffset => format!("-> {v}"),
            OperandKind::BuiltinIndex => match crate::builtin_table::builtin(*v) {
                Some(b) => format!("{}#{}", b.name, v),
                None => format!("builtin#{v}"),
            },
            OperandKind::ScriptIndex => format!("script[{v}]"),
            OperandKind::FileIndex => format!("file[{v}]"),
            OperandKind::ArgCount => format!("argc={v}"),
            OperandKind::TriggerIndex => format!("trigger[{v}]"),
            OperandKind::TypeTag => match crate::value::ScriptTy::from_tag(*v) {
                Some(t) => format!("{t:?}"),
                None => format!("type#{v:#x}"),
            },
            OperandKind::Raw | OperandKind::None => format!("{v:#x}"),
        });
    }
    s
}

pub fn format_all(code: &[u8]) -> Result<String, String> {
    Ok(disassemble(code)?
        .iter()
        .map(format_insn)
        .collect::<Vec<_>>()
        .join("\n"))
}

/// A minimal assembler: `(opcode, operands)` pairs to bytes.
///
/// Encoding is the engine's: one opcode byte then one little-endian `u32` per
/// operand, unaligned (`OpCode::count_code`, `0x009c2ed0`, adds exactly `1` for the
/// opcode and `4` per present `OpArg`).
pub fn asm(items: &[(u8, &[u32])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (op, ops) in items {
        out.push(*op);
        for o in *ops {
            out.extend_from_slice(&o.to_le_bytes());
        }
    }
    out
}

/// Byte length an [`asm`] item list will produce, so tests can compute jump targets.
pub fn asm_len(items: &[(u8, &[u32])]) -> usize {
    items.iter().map(|(_, o)| 1 + 4 * o.len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let code = asm(&[
            (0x26, &[VarRef::Local(0).encode()]),
            (0x26, &[VarRef::Const(2).encode()]),
            (0x04, &[]),
            (0x38, &[7]),
            (0x3e, &[]),
        ]);
        let d = disassemble(&code).unwrap();
        assert_eq!(d.len(), 5);
        assert_eq!(d[0].name, "OP_PUSH");
        assert_eq!(d[1].operands[0], 2 | 0x2000_0000);
        assert_eq!(d[2].name, "OP_ADD");
        assert_eq!(d[3].name, "OP_CALL_GAME");
        // 5 (push) + 5 (push) + 1 (add) + 5 (call_game) = 16
        assert_eq!(d[4].offset, 16);
        assert_eq!(d.iter().map(|i| i.len).sum::<usize>(), code.len());
    }

    #[test]
    fn rejects_truncated_operands() {
        assert!(disassemble(&[0x3f, 0x01, 0x02]).is_err());
    }
}
