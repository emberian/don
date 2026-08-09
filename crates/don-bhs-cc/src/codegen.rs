//! Code generation: AST -> the **engine's own bytecode**.
//!
//! There is no invented instruction set here. Every byte emitted is an `OpCodeTypes`
//! value from `ron-bin/sbl/rise.pdb` (`LF_ENUM 0x3c85`, 73 enumerators) and the encoding
//! is the one `RunTimeEnv::exec` (`0x009c3600`) fetches: **one opcode byte followed by
//! 0, 1 or 2 unaligned little-endian 32-bit operands**. The tables live in
//! [`don_bhs::opcode`]; the runtime meaning of each is in [`don_bhs::vm`].
//!
//! # Why we may choose our own instruction *sequence*
//!
//! `RunTimeEnv::walk_data` (`0x009c41a0`) is checksum channel 15. Read at the
//! instruction level: it calls `RunTimeEnv::close` first, which frees the frame stack
//! and the operand stack and zeroes `cur_vm`, `bytecodes_executed` and `script_status`;
//! it then walks only `ScriptFile::script_files`. `VirtualMachine` has **no walker at
//! all**. So the program counter, the operand stack, the locals array and the number of
//! bytecodes executed are invisible to the checksum, and any instruction sequence
//! reaching the same script-visible state hashes identically.
//!
//! What *is* hashed is the compiled image: `ScriptFile::code`, `const_pool`,
//! `linked_files`, and per script `static_vars`, `trigger_bits`, `params`, `refs`, the
//! three name tables, `name`, `offset`, `return_type` and `script_type`. Matching
//! retail's channel-15 word from source therefore requires byte-identical output, which
//! is a *measurable* goal needing reference bytecode from the retail compiler to diff
//! against. Nothing here claims to have met it.
//!
//! # Fidelity of the individual lowerings
//!
//! Each lowering below is marked. `[measured]` means the opcode's runtime behaviour was
//! read out of `VirtualMachine::execute_next` (by the engine lane) and the lowering is
//! the only sequence consistent with it. `[inferred]` means the opcode's behaviour is
//! known but the *compiler's* choice among several consistent sequences is not, so a
//! retail disassembly could still contradict us. The `[inferred]` ones are collected in
//! `docs/tracks/bhs-compiler.md` so they can be checked in one pass when reference
//! bytecode exists.

use std::collections::HashMap;

use don_bhs::program::{Program, Script, ScriptFile};
use don_bhs::value::{ScriptTy, Value};

use crate::ast::*;
use crate::lex::{Pos, P};
use crate::sema::{resolve_builtin, Diag, Severity, StructInfo, Ty, Unit};

// ---------------------------------------------------------------- opcode names

#[allow(dead_code)]
mod op {
    pub const ASSIGN: u8 = 0x00;
    pub const EQ: u8 = 0x01;
    pub const NE: u8 = 0x02;
    pub const ADD_ASSIGN: u8 = 0x03;
    pub const ADD: u8 = 0x04;
    pub const LESS: u8 = 0x05;
    pub const GREA: u8 = 0x06;
    pub const LE: u8 = 0x07;
    pub const GE: u8 = 0x08;
    pub const UNA_NOT: u8 = 0x09;
    pub const MUL_ASSIGN: u8 = 0x0c;
    pub const DIV_ASSIGN: u8 = 0x0d;
    pub const SUB_ASSIGN: u8 = 0x0e;
    pub const MUL: u8 = 0x0f;
    pub const DIV: u8 = 0x10;
    pub const SUBT: u8 = 0x11;
    pub const INC: u8 = 0x12;
    pub const DEC: u8 = 0x13;
    pub const INC_POST: u8 = 0x14;
    pub const DEC_POST: u8 = 0x15;
    pub const UNA_NEGA: u8 = 0x16;
    pub const POW: u8 = 0x17;
    pub const POW_ASSIGN: u8 = 0x18;
    pub const MOD_ASSIGN: u8 = 0x19;
    pub const LEFT_ASSIGN: u8 = 0x1a;
    pub const RIGHT_ASSIGN: u8 = 0x1b;
    pub const AND_ASSIGN: u8 = 0x1c;
    pub const XOR_ASSIGN: u8 = 0x1d;
    pub const OR_ASSIGN: u8 = 0x1e;
    pub const MOD: u8 = 0x1f;
    pub const SHL: u8 = 0x20;
    pub const SHR: u8 = 0x21;
    pub const AND_BIT: u8 = 0x22;
    pub const XOR_BIT: u8 = 0x23;
    pub const OR_BIT: u8 = 0x24;
    pub const UNA_TILD: u8 = 0x25;
    pub const PUSH: u8 = 0x26;
    pub const POP: u8 = 0x27;
    pub const CREATE_SIMPLE: u8 = 0x28;
    pub const CREATE_ARRAY: u8 = 0x29;
    pub const CREATE_ARRAY_DYN: u8 = 0x2a;
    pub const CREATE_ARRAY_INITER: u8 = 0x2b;
    pub const CREATE_STRUCT: u8 = 0x2c;
    pub const PUSH_ARRAY_INDEX: u8 = 0x2d;
    pub const CREATE_ARRAY_INDEX: u8 = 0x2e;
    pub const PUSH_STRUCT_FIELD: u8 = 0x2f;
    pub const PUSH_ARRAY_LENGTH: u8 = 0x30;
    pub const SET_ARRAY_LENGTH: u8 = 0x31;
    pub const INIT_COPY: u8 = 0x32;
    pub const INIT: u8 = 0x33;
    pub const CAST: u8 = 0x34;
    pub const CALL: u8 = 0x36;
    pub const CALL_INCLUDE: u8 = 0x37;
    pub const CALL_GAME: u8 = 0x38;
    pub const CASE: u8 = 0x3a;
    pub const BIT_CLEAR: u8 = 0x3c; // enum says OP_BIT_SET; the machine uses `btr`.
    pub const BIT_SET: u8 = 0x3d; // enum says OP_BIT_UNSET; the machine uses `bts`.
    pub const RETURN: u8 = 0x3e;
    pub const JUMP: u8 = 0x3f;
    pub const JUMP_IF: u8 = 0x40;
    pub const JUMP_IF_NOT: u8 = 0x41;
    pub const JUMP_IF_SC_TRUE: u8 = 0x42;
    pub const JUMP_IF_SC_FALSE: u8 = 0x43;
    pub const JUMP_IF_INITED: u8 = 0x44;
    pub const JUMP_IF_BITSET: u8 = 0x45;
    pub const ERROR_TOKEN: u8 = 0x48;
}

/// **The one convention this compiler shares with the VM and neither has confirmed.**
///
/// The assignment handler (`0x009e10e9`) pops two slots and calls `do_operator` on the
/// first. `don-bhs`'s VM reads the first-popped as the *target*; this compiler therefore
/// emits value-then-target. If a retail disassembly shows the opposite, exactly two
/// places change: this constant's users here and the matching arm in `don_bhs::vm`.
/// Flipping only one silently produces a compiler and a VM that agree with each other
/// and with nothing else.
pub const ASSIGN_EMITS_VALUE_THEN_TARGET: bool = true;

// ------------------------------------------------------------------ var refs

/// A resolved storage location, mirroring `VirtualMachine::get_value`'s three-way tag
/// test at `0x004d1010`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Const(u32),
    Static(u32),
    Local(u32),
}

impl Slot {
    fn encode(self) -> u32 {
        match self {
            Slot::Const(i) => i | 0x2000_0000,
            Slot::Static(i) => i | 0x4000_0000,
            Slot::Local(i) => i,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ConstKey {
    I(i32),
    F(u32),
    S(String),
}

// ------------------------------------------------------------------ per-file

struct FileGen<'a> {
    unit: &'a Unit,
    file: usize,
    code: Vec<u8>,
    consts: Vec<Value>,
    const_index: HashMap<ConstKey, u32>,
    /// Const-pool indices that came from a `$S("…")` literal rather than a plain string.
    loc_consts: Vec<u32>,
    /// Source line for each emitted opcode start, in `ScriptFile::line_to_op` shape.
    line_to_op: Vec<i32>,
    scripts: Vec<Script>,
    diags: Vec<Diag>,
    /// Count of implicit `OP_CAST`s inserted; reported so the number is visible rather
    /// than silent.
    auto_casts: usize,
}

/// Per-script compilation state.
struct ScriptGen {
    locals: Vec<String>,
    statics: Vec<String>,
    /// Declared type of each local/static, for member and index lowering.
    local_ty: Vec<Ty>,
    static_ty: Vec<Ty>,
    /// Lexical scope stack: name -> slot. Locals are a *flat* array in the engine
    /// (`Script::var_names`), so an inner block's shadow gets a fresh slot rather than
    /// reusing one.
    scopes: Vec<HashMap<String, Slot>>,
    labels: HashMap<String, i64>,
    triggers: Vec<String>,
    trigger_bits: Vec<u8>,
    ret: Ty,
    /// Targets to patch: (offset of the 4-byte operand, label id).
    fixups: Vec<(usize, usize)>,
    /// label id -> resolved code offset.
    label_pos: Vec<Option<u32>>,
    /// Innermost loop/switch exits and loop continue targets.
    breaks: Vec<usize>,
    continues: Vec<Option<usize>>,
}

impl ScriptGen {
    fn new_label(&mut self) -> usize {
        self.label_pos.push(None);
        self.label_pos.len() - 1
    }

    fn lookup(&self, name: &str) -> Option<Slot> {
        for s in self.scopes.iter().rev() {
            if let Some(v) = s.get(&name.to_ascii_lowercase()) {
                return Some(*v);
            }
        }
        None
    }

    fn slot_ty(&self, s: Slot) -> Ty {
        match s {
            Slot::Local(i) => self
                .local_ty
                .get(i as usize)
                .cloned()
                .unwrap_or(Ty::Untyped),
            Slot::Static(i) => self
                .static_ty
                .get(i as usize)
                .cloned()
                .unwrap_or(Ty::Untyped),
            Slot::Const(_) => Ty::Untyped,
        }
    }

    fn declare_local(&mut self, name: &str, ty: Ty) -> Slot {
        let i = self.locals.len() as u32;
        self.locals.push(name.to_string());
        self.local_ty.push(ty);
        let s = Slot::Local(i);
        self.scopes
            .last_mut()
            .unwrap()
            .insert(name.to_ascii_lowercase(), s);
        s
    }

    fn declare_static(&mut self, name: &str, ty: Ty) -> Slot {
        let i = self.statics.len() as u32;
        self.statics.push(name.to_string());
        self.static_ty.push(ty);
        let s = Slot::Static(i);
        // A `static` is visible from its declaration to the end of the script, exactly
        // like a local; only its *storage* differs.
        self.scopes
            .last_mut()
            .unwrap()
            .insert(name.to_ascii_lowercase(), s);
        s
    }

    fn trigger_index(&mut self, name: &str) -> u32 {
        if let Some(i) = self
            .triggers
            .iter()
            .position(|t| t.eq_ignore_ascii_case(name))
        {
            return i as u32;
        }
        self.triggers.push(name.to_string());
        let i = self.triggers.len() - 1;
        if self.trigger_bits.len() * 8 <= i {
            self.trigger_bits.push(0);
        }
        // Triggers are ENABLED at load: the shipped scripts call `disable_trigger` to
        // turn one off, never `enable_trigger` before first use. `OP_JUMP_IF_BITSET`
        // falls into the body when the bit is set.
        self.trigger_bits[i / 8] |= 1 << (i % 8);
        i as u32
    }
}

/// Counters worth surfacing rather than hiding.
#[derive(Debug, Clone, Copy, Default)]
pub struct Stats {
    /// Implicit `OP_CAST`s inserted because the engine's `do_operator` performs no
    /// runtime coercion. See [`FileGen::binary`].
    pub auto_casts: usize,
}

/// Compile a whole analysed unit.
///
/// The returned program is deliberately non-executable when semantic analysis or
/// lowering reports any error: each file's code is replaced by `OP_ERROR_TOKEN`.
/// This keeps diagnostic collection useful without letting a caller that forgets to
/// inspect diagnostics execute a guessed lowering.
pub fn compile(unit: &Unit) -> (Program, Vec<Diag>, Stats) {
    let mut prog = Program::default();
    let mut diags = Vec::new();
    let mut stats = Stats::default();
    for fi in 0..unit.files.len() {
        let mut g = FileGen {
            unit,
            file: fi,
            code: Vec::new(),
            consts: Vec::new(),
            const_index: HashMap::new(),
            loc_consts: Vec::new(),
            line_to_op: Vec::new(),
            scripts: Vec::new(),
            diags: Vec::new(),
            auto_casts: 0,
        };
        let sf = g.run();
        diags.append(&mut g.diags);
        stats.auto_casts += g.auto_casts;
        prog.files.push(sf);
    }
    let has_error = unit
        .diags
        .iter()
        .chain(diags.iter())
        .any(|d| d.severity == Severity::Error);
    if has_error {
        for file in &mut prog.files {
            file.code.clear();
            file.code.push(op::ERROR_TOKEN);
            file.line_to_op.clear();
            for script in &mut file.scripts {
                script.entry = 0;
            }
        }
    }
    (prog, diags, stats)
}

impl<'a> FileGen<'a> {
    fn path(&self) -> String {
        self.unit.files[self.file].path.display().to_string()
    }

    fn diag(&mut self, sev: Severity, pos: Pos, msg: impl Into<String>) {
        let file = self.path();
        self.diags.push(Diag {
            severity: sev,
            file,
            pos,
            msg: msg.into(),
        });
    }

    fn run(&mut self) -> ScriptFile {
        // One `Script` per name in the file's declaration order — that ordering is the
        // `OP_CALL` operand space, so it is fixed before any code is emitted.
        let names = self.unit.files[self.file].script_names.clone();
        self.scripts = names
            .iter()
            .map(|n| Script {
                name: n.clone(),
                ..Default::default()
            })
            .collect();

        let items = self.unit.files[self.file].ast.items.clone();
        for it in &items {
            match it {
                Item::Script(s) => {
                    if let Some(body) = &s.body {
                        self.script(&s.sig.name, &s.sig, body);
                    }
                }
                Item::Main(m) => {
                    let name = crate::sema::entry_name(&self.unit.files[self.file].path);
                    let sig = ScriptSig {
                        ret: m.ret.clone(),
                        script_type: Some(m.script_type.clone()),
                        name: name.clone(),
                        params: Vec::new(),
                        pos: m.pos,
                    };
                    self.script(&name, &sig, &m.body);
                }
                _ => {}
            }
        }

        ScriptFile {
            source_file: self.path(),
            code: std::mem::take(&mut self.code),
            scripts: std::mem::take(&mut self.scripts),
            const_pool: std::mem::take(&mut self.consts),
            linked_file_names: self.unit.files[self.file]
                .includes
                .iter()
                .map(|&i| self.unit.files[i].path.display().to_string())
                .collect(),
            line_to_op: std::mem::take(&mut self.line_to_op),
        }
    }

    // ---------------------------------------------------------- emission

    fn here(&self) -> u32 {
        self.code.len() as u32
    }

    fn emit(&mut self, b: u8, pos: Pos) {
        self.code.push(b);
        self.line_to_op.push(pos.line as i32);
    }

    fn emit_u32(&mut self, v: u32) {
        self.code.extend_from_slice(&v.to_le_bytes());
        // `line_to_op` is indexed by opcode, not by byte; operands add no entry.
    }

    fn emit1(&mut self, b: u8, a: u32, pos: Pos) {
        self.emit(b, pos);
        self.emit_u32(a);
    }

    fn emit2(&mut self, b: u8, a: u32, c: u32, pos: Pos) {
        self.emit(b, pos);
        self.emit_u32(a);
        self.emit_u32(c);
    }

    /// Emit an opcode whose 4-byte operand is a forward jump target.
    fn emit_jump(&mut self, g: &mut ScriptGen, b: u8, label: usize, pos: Pos) {
        self.emit(b, pos);
        g.fixups.push((self.code.len(), label));
        self.emit_u32(0);
    }

    /// Emit a two-operand conditional jump: `[first][target]`.
    fn emit_jump2(&mut self, g: &mut ScriptGen, b: u8, first: u32, label: usize, pos: Pos) {
        self.emit(b, pos);
        self.emit_u32(first);
        g.fixups.push((self.code.len(), label));
        self.emit_u32(0);
    }

    fn place(&mut self, g: &mut ScriptGen, label: usize) {
        g.label_pos[label] = Some(self.here());
    }

    fn intern(&mut self, v: Value) -> u32 {
        let key = match &v {
            Value::Int(i) => ConstKey::I(*i),
            Value::Real(f) => ConstKey::F(f.to_bits()),
            Value::Str(s) => ConstKey::S((**s).clone()),
            _ => ConstKey::I(0),
        };
        if let Some(i) = self.const_index.get(&key) {
            return *i;
        }
        let i = self.consts.len() as u32;
        self.consts.push(v);
        self.const_index.insert(key, i);
        i
    }

    // ---------------------------------------------------------- one script

    fn script(&mut self, name: &str, sig: &ScriptSig, body: &Block) {
        let si = match self
            .scripts
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(name))
        {
            Some(i) => i,
            None => return,
        };
        let entry = self.here();

        let ret = self.unit.resolve_type(sig.ret.as_ref());
        let mut g = ScriptGen {
            locals: Vec::new(),
            statics: Vec::new(),
            local_ty: Vec::new(),
            static_ty: Vec::new(),
            scopes: vec![HashMap::new()],
            labels: self.unit.labels.clone(),
            triggers: Vec::new(),
            trigger_bits: Vec::new(),
            ret: ret.clone(),
            fixups: Vec::new(),
            label_pos: Vec::new(),
            breaks: Vec::new(),
            continues: Vec::new(),
        };

        // Parameters occupy local slots 0..arity, in order. `RunTimeEnv::call_script`
        // leaves the arguments on the shared run stack and
        // `expected_stack_size -= params.count`, so the callee's slots line up with the
        // caller's pushes without any copying instruction. [measured]
        for p in &sig.params {
            let ty = self.unit.resolve_type(p.ty.as_ref());
            g.declare_local(&p.name, ty);
        }

        // A `labels` block anywhere in the body is script-scoped and visible throughout,
        // so hoist them before emitting: shipped scripts use a label above its block.
        self.hoist_labels(&mut g, &body.stmts);

        for s in &body.stmts {
            self.stmt(&mut g, s);
        }

        // Implicit tail return. `RunTimeEnv::close_frame` asserts the stack landed on
        // `expected_stack_size`, which is +1 for a non-void script, so a non-void script
        // that falls off the end must still leave a value.
        if !matches!(ret, Ty::Scalar(ScriptTy::Void)) {
            let c = self.intern(Value::Int(0));
            self.emit1(op::PUSH, Slot::Const(c).encode(), body.pos);
        }
        self.emit(op::RETURN, body.pos);

        // Patch jumps.
        for (at, label) in std::mem::take(&mut g.fixups) {
            let target = g.label_pos[label].unwrap_or_else(|| {
                // A label that was never placed is a compiler bug, not a source error.
                self.code.len() as u32
            });
            self.code[at..at + 4].copy_from_slice(&target.to_le_bytes());
        }

        let return_type = self.resolved_type_tag(&ret);
        let s = &mut self.scripts[si];
        s.arity = sig.params.len();
        s.entry = entry;
        s.return_type = return_type;
        s.script_type = script_type_tag(sig.script_type.as_deref());
        s.var_names = g.locals;
        s.static_var_names = g.statics.clone();
        s.statics = vec![None; g.statics.len()];
        s.trigger_names = g.triggers;
        s.trigger_count = s.trigger_names.len() as i32;
        s.trigger_bits = g.trigger_bits;
    }

    fn hoist_labels(&mut self, g: &mut ScriptGen, stmts: &[Stmt]) {
        for s in stmts {
            match s {
                Stmt::Labels { defs, .. } => {
                    let path = self.path();
                    let mut diags = std::mem::take(&mut self.diags);
                    crate::sema::eval_labels(defs, &mut g.labels, &mut diags, &path);
                    self.diags = diags;
                }
                Stmt::Block(b) => self.hoist_labels(g, &b.stmts),
                Stmt::RunOnce { body, .. } => self.hoist_labels(g, &body.stmts),
                _ => {}
            }
        }
    }

    // ---------------------------------------------------------- statements

    fn stmt(&mut self, g: &mut ScriptGen, s: &Stmt) {
        match s {
            Stmt::Empty(_) | Stmt::Labels { .. } | Stmt::Struct(_) => {}
            Stmt::Expr(e) => self.expr_stmt(g, e),
            Stmt::Decl(d) => self.decl(g, d),
            Stmt::Block(b) => {
                g.scopes.push(HashMap::new());
                for s in &b.stmts {
                    self.stmt(g, s);
                }
                g.scopes.pop();
            }
            Stmt::If {
                cond,
                then,
                els,
                pos,
            } => {
                let l_else = g.new_label();
                self.expr(g, cond);
                self.emit_jump(g, op::JUMP_IF_NOT, l_else, *pos);
                self.stmt(g, then);
                match els {
                    None => self.place(g, l_else),
                    Some(e) => {
                        let l_end = g.new_label();
                        self.emit_jump(g, op::JUMP, l_end, *pos);
                        self.place(g, l_else);
                        self.stmt(g, e);
                        self.place(g, l_end);
                    }
                }
            }
            Stmt::While { cond, body, pos } => {
                let l_top = g.new_label();
                let l_end = g.new_label();
                self.place(g, l_top);
                self.expr(g, cond);
                self.emit_jump(g, op::JUMP_IF_NOT, l_end, *pos);
                g.breaks.push(l_end);
                g.continues.push(Some(l_top));
                self.stmt(g, body);
                g.breaks.pop();
                g.continues.pop();
                self.emit_jump(g, op::JUMP, l_top, *pos);
                self.place(g, l_end);
            }
            Stmt::DoWhile { body, cond, pos } => {
                let l_top = g.new_label();
                let l_cont = g.new_label();
                let l_end = g.new_label();
                self.place(g, l_top);
                g.breaks.push(l_end);
                g.continues.push(Some(l_cont));
                self.stmt(g, body);
                g.breaks.pop();
                g.continues.pop();
                self.place(g, l_cont);
                self.expr(g, cond);
                self.emit_jump(g, op::JUMP_IF, l_top, *pos);
                self.place(g, l_end);
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
                pos,
            } => {
                // The init declaration scopes to the loop.
                g.scopes.push(HashMap::new());
                if let Some(i) = init {
                    self.stmt(g, i);
                }
                let l_top = g.new_label();
                let l_step = g.new_label();
                let l_end = g.new_label();
                self.place(g, l_top);
                if let Some(c) = cond {
                    self.expr(g, c);
                    self.emit_jump(g, op::JUMP_IF_NOT, l_end, *pos);
                }
                g.breaks.push(l_end);
                g.continues.push(Some(l_step));
                self.stmt(g, body);
                g.breaks.pop();
                g.continues.pop();
                self.place(g, l_step);
                if let Some(st) = step {
                    self.expr_stmt(g, st);
                }
                self.emit_jump(g, op::JUMP, l_top, *pos);
                self.place(g, l_end);
                g.scopes.pop();
            }
            Stmt::Switch { subject, arms, pos } => self.switch(g, subject, arms, *pos),
            Stmt::Break(pos) => match g.breaks.last().copied() {
                Some(l) => self.emit_jump(g, op::JUMP, l, *pos),
                None => self.diag(Severity::Error, *pos, "`break` outside a loop or switch"),
            },
            Stmt::Continue(pos) => match g.continues.last().copied().flatten() {
                Some(l) => self.emit_jump(g, op::JUMP, l, *pos),
                None => self.diag(Severity::Error, *pos, "`continue` outside a loop"),
            },
            Stmt::Return { value, pos } => {
                let is_void = matches!(g.ret, Ty::Scalar(ScriptTy::Void));
                match value {
                    Some(e) => {
                        self.expr(g, e);
                        if is_void {
                            // A value returned from a void script would unbalance the
                            // frame; drop it and say so.
                            self.diag(
                                Severity::Error,
                                *pos,
                                "a `void` script cannot return a value",
                            );
                            self.emit(op::POP, *pos);
                        }
                    }
                    None => {
                        if !is_void {
                            let c = self.intern(Value::Int(0));
                            self.emit1(op::PUSH, Slot::Const(c).encode(), *pos);
                        }
                    }
                }
                self.emit(op::RETURN, *pos);
            }
            Stmt::Trigger {
                name,
                cond,
                body,
                pos,
            } => {
                // `OP_JUMP_IF_BITSET` falls INTO the body when the bit is set and jumps
                // PAST it when clear, so it is the guard, not the test. [measured]
                let tname = name
                    .clone()
                    .unwrap_or_else(|| format!("trigger@{}", pos.line));
                let idx = g.trigger_index(&tname);
                let l_end = g.new_label();
                self.emit_jump2(g, op::JUMP_IF_BITSET, idx, l_end, *pos);
                if let Some(c) = cond {
                    self.expr(g, c);
                    self.emit_jump(g, op::JUMP_IF_NOT, l_end, *pos);
                }
                g.scopes.push(HashMap::new());
                self.stmt(g, body);
                g.scopes.pop();
                self.place(g, l_end);
            }
            Stmt::RunOnce { body, pos } => {
                // Lowered with the same mechanism `static` initialisers use: a hidden
                // static slot plus `OP_JUMP_IF_INITED`. [inferred] — the opcode
                // semantics are measured, retail's choice of lowering for `run_once` is
                // not.
                let slot =
                    g.declare_static(&format!("run_once@{}", pos.line), Ty::Scalar(ScriptTy::Int));
                let l_end = g.new_label();
                self.emit_jump2(g, op::JUMP_IF_INITED, slot.encode(), l_end, *pos);
                let c = self.intern(Value::Int(1));
                self.emit1(op::PUSH, Slot::Const(c).encode(), *pos);
                self.emit1(op::INIT, slot.encode(), *pos);
                g.scopes.push(HashMap::new());
                for s in &body.stmts {
                    self.stmt(g, s);
                }
                g.scopes.pop();
                self.place(g, l_end);
            }
        }
    }

    /// `switch` lowers to a linear `OP_CASE` dispatch chain, which is exactly what the
    /// opcode is for: it peeks the subject, compares it against `const_pool[k]` with
    /// `OP_EQ_OP`, and on a match **pops the subject** and jumps. On fall-through the
    /// subject is still on the stack, so the default path must pop it explicitly.
    /// [measured, from handler `0x009e1259`]
    fn switch(&mut self, g: &mut ScriptGen, subject: &Expr, arms: &[SwitchArm], pos: Pos) {
        self.expr(g, subject);
        let l_end = g.new_label();
        let arm_labels: Vec<usize> = arms.iter().map(|_| g.new_label()).collect();
        let mut default_arm: Option<usize> = None;

        for (ai, arm) in arms.iter().enumerate() {
            for l in &arm.labels {
                match &l.value {
                    Some(e) => match self.const_value(g, e) {
                        Some(v) => {
                            let k = self.intern(v);
                            self.emit_jump2(g, op::CASE, k | 0x2000_0000, arm_labels[ai], l.pos);
                        }
                        None => self.diag(
                            Severity::Error,
                            l.pos,
                            "`case` value is not a compile-time constant",
                        ),
                    },
                    None => default_arm = Some(ai),
                }
            }
        }
        // Fall-through: the subject is still on the stack.
        self.emit(op::POP, pos);
        match default_arm {
            Some(ai) => self.emit_jump(g, op::JUMP, arm_labels[ai], pos),
            None => self.emit_jump(g, op::JUMP, l_end, pos),
        }

        g.breaks.push(l_end);
        // `continue` inside a switch belongs to the enclosing loop, so the switch pushes
        // a transparent entry rather than shadowing it.
        let outer_continue = g.continues.last().copied().flatten();
        g.continues.push(outer_continue);
        for (ai, arm) in arms.iter().enumerate() {
            self.place(g, arm_labels[ai]);
            g.scopes.push(HashMap::new());
            for s in &arm.body {
                self.stmt(g, s);
            }
            g.scopes.pop();
        }
        g.continues.pop();
        g.breaks.pop();
        self.place(g, l_end);
    }

    fn const_value(&mut self, g: &ScriptGen, e: &Expr) -> Option<Value> {
        match e {
            Expr::Str(s, _) | Expr::LocStr(s, _) => Some(Value::str(s.clone())),
            Expr::Real(f, _) => Some(Value::Real(*f)),
            _ => crate::sema::const_int(e, &g.labels).map(|v| Value::Int(v as i32)),
        }
    }

    // -------------------------------------------------------- declarations

    fn decl(&mut self, g: &mut ScriptGen, d: &VarDecl) {
        for decl in &d.decls {
            let base = self.unit.resolve_type(d.ty.as_ref());
            let ty = if decl.array.is_some() {
                Ty::Array(Box::new(base.clone()))
            } else {
                base
            };
            let slot = if d.is_static {
                g.declare_static(&decl.name, ty.clone())
            } else {
                g.declare_local(&decl.name, ty.clone())
            };

            // A `static` initialiser runs exactly once, ever. `OP_JUMP_IF_INITED` tests
            // the slot for null and skips the initialiser when it holds a value; that is
            // the entire mechanism by which a per-frame script keeps state across frames.
            // [measured, handler 0x009e0fca]
            let skip = if d.is_static {
                let l = g.new_label();
                self.emit_jump2(g, op::JUMP_IF_INITED, slot.encode(), l, decl.pos);
                Some(l)
            } else {
                None
            };

            let copy = self.init_value(g, &ty, decl);
            let opc = if copy { op::INIT_COPY } else { op::INIT };
            self.emit1(opc, slot.encode(), decl.pos);

            if let Some(l) = skip {
                self.place(g, l);
            }
        }
    }

    /// Emit the value a declaration initialises with. Returns true when `OP_INIT_COPY`
    /// should be used instead of `OP_INIT`.
    ///
    /// The two opcodes share `set_value`; the difference is whether the stored value
    /// keeps aliasing the source. We use `OP_INIT_COPY` when the initialiser is a bare
    /// variable reference (which `OP_PUSH` leaves on the stack as an alias) and `OP_INIT`
    /// for a freshly computed temporary. [inferred]
    fn init_value(&mut self, g: &mut ScriptGen, ty: &Ty, d: &Declarator) -> bool {
        match (&d.init, &d.array) {
            (Some(Expr::ArrayLit(items, pos)), _) => {
                // Retail's constructor pops into values[0..], so source element 0
                // must be on top: emit the source list in reverse.
                for it in items.iter().rev() {
                    self.expr(g, it);
                }
                if items.is_empty() {
                    self.diag(
                        Severity::Error,
                        *pos,
                        "an array initializer cannot be empty",
                    );
                }
                self.emit2(
                    op::CREATE_ARRAY_INITER,
                    items.len() as u32,
                    self.array_element_tag(ty),
                    *pos,
                );
                false
            }
            (Some(e), _) => {
                self.expr(g, e);
                matches!(e, Expr::Name(..))
            }
            (None, Some(ArraySuffix::Sized(n))) => {
                self.emit_default_value(g, array_inner(ty), d.pos);
                self.expr(g, n);
                self.emit1(op::CREATE_ARRAY_DYN, self.array_element_tag(ty), d.pos);
                false
            }
            (None, Some(ArraySuffix::Dynamic)) => {
                self.emit_default_value(g, array_inner(ty), d.pos);
                self.emit1(op::CREATE_ARRAY, self.array_element_tag(ty), d.pos);
                false
            }
            (None, None) => {
                self.emit_default_value(g, ty, d.pos);
                false
            }
        }
    }

    /// Emit a real initialized value, including the prototypes and member values
    /// consumed by the aggregate constructors. The earlier recovered compiler
    /// emitted aggregate opcodes onto an empty stack; it decoded but could never run.
    fn emit_default_value(&mut self, g: &mut ScriptGen, ty: &Ty, pos: Pos) {
        match ty {
            Ty::Struct(si) => {
                let fields = self.unit.structs[*si].fields.clone();
                for field in fields.iter().rev() {
                    if let Some(n) = field.fixed_len {
                        self.emit_default_value(g, &field.ty, pos);
                        let c = self.intern(Value::Int(n as i32));
                        self.emit1(op::PUSH, Slot::Const(c).encode(), pos);
                        self.emit1(op::CREATE_ARRAY_DYN, self.resolved_type_tag(&field.ty), pos);
                    } else {
                        self.emit_default_value(g, &field.ty, pos);
                    }
                }
                // Both operands are measured: [member count][struct type tag].
                // SymTable assigns user types String::generate_hash's
                // case-insensitive word, the same scheme as the ten builtin tags.
                let tag = bhs_type_hash(&self.unit.structs[*si].name);
                self.emit2(op::CREATE_STRUCT, fields.len() as u32, tag, pos);
            }
            Ty::Array(inner) => {
                self.emit_default_value(g, inner, pos);
                self.emit1(op::CREATE_ARRAY, self.resolved_type_tag(inner), pos);
            }
            _ => self.emit1(op::CREATE_SIMPLE, self.resolved_type_tag(ty), pos),
        }
    }

    fn resolved_type_tag(&self, ty: &Ty) -> u32 {
        match ty {
            Ty::Struct(si) => bhs_type_hash(&self.unit.structs[*si].name),
            _ => ty.tag(),
        }
    }

    fn array_element_tag(&self, ty: &Ty) -> u32 {
        self.resolved_type_tag(array_inner(ty))
    }

    // --------------------------------------------------------- expressions

    /// An expression in statement position. Everything that leaves a value on the stack
    /// gets an `OP_POP`; the inc/dec handlers *swallow* a following `OP_POP`, which is
    /// why `i++;` costs no stack traffic in retail either. [measured]
    fn expr_stmt(&mut self, g: &mut ScriptGen, e: &Expr) {
        let pos = e.pos();
        self.expr(g, e);
        if self.leaves_value(g, e) {
            self.emit(op::POP, pos);
        }
    }

    fn leaves_value(&self, g: &ScriptGen, e: &Expr) -> bool {
        match e {
            // A void builtin or void script pushes nothing.
            Expr::Call { name, args, .. } => self.call_leaves_value(g, name, args.len()),
            Expr::MethodCall { name, args, .. } => self.call_leaves_value(g, name, args.len() + 1),
            _ => true,
        }
    }

    fn call_leaves_value(&self, _g: &ScriptGen, name: &str, argc: usize) -> bool {
        if name.eq_ignore_ascii_case("enable_trigger")
            || name.eq_ignore_ascii_case("disable_trigger")
        {
            return false; // an intrinsic that lowers to one bit opcode
        }
        if let Some(s) = self.unit.script_by_name(name) {
            return !matches!(s.ret, Ty::Scalar(ScriptTy::Void));
        }
        match resolve_builtin(name, argc) {
            Some(d) => d.ret != ScriptTy::Void,
            None => true,
        }
    }

    /// Compile an expression so that it leaves exactly one slot on the run stack
    /// (except a void call, which leaves none — see [`Self::leaves_value`]).
    fn expr(&mut self, g: &mut ScriptGen, e: &Expr) {
        let pos = e.pos();
        match e {
            Expr::Int(v, _) => {
                let c = self.intern(Value::Int(*v as i32));
                self.emit1(op::PUSH, Slot::Const(c).encode(), pos);
            }
            Expr::Real(v, _) => {
                let c = self.intern(Value::Real(*v));
                self.emit1(op::PUSH, Slot::Const(c).encode(), pos);
            }
            Expr::Str(s, _) => {
                let c = self.intern(Value::str(s.clone()));
                self.emit1(op::PUSH, Slot::Const(c).encode(), pos);
            }
            Expr::LocStr(s, _) => {
                // `$S("…")` currently compiles to a plain string constant, with the
                // const-pool index recorded so a localisation hook can be attached in one
                // place if the retail compiler turns out to mark these. [inferred]
                let c = self.intern(Value::str(s.clone()));
                if !self.loc_consts.contains(&c) {
                    self.loc_consts.push(c);
                }
                self.emit1(op::PUSH, Slot::Const(c).encode(), pos);
            }
            Expr::Name(n, _) => {
                let slot = self.name_slot(g, n, pos);
                self.emit1(op::PUSH, slot.encode(), pos);
            }
            Expr::ArrayLit(items, _) => {
                for it in items.iter().rev() {
                    self.expr(g, it);
                }
                let elem_tag = items
                    .first()
                    .and_then(|e| self.static_ty(g, e))
                    .map(|t| self.resolved_type_tag(&t));
                if items.is_empty() {
                    self.diag(
                        Severity::Error,
                        pos,
                        "an untyped array literal cannot be empty",
                    );
                }
                self.emit2(
                    op::CREATE_ARRAY_INITER,
                    items.len() as u32,
                    elem_tag.unwrap_or_else(|| ScriptTy::Any.tag()),
                    pos,
                );
            }
            Expr::Cast { ty, expr, .. } => {
                self.expr(g, expr);
                let t = self.unit.resolve_type(Some(ty));
                self.emit1(op::CAST, self.resolved_type_tag(&t), pos);
            }
            Expr::Unary { op: p, expr, .. } => {
                self.expr(g, expr);
                let b = match p {
                    P::Not => op::UNA_NOT,
                    P::Minus => op::UNA_NEGA,
                    P::Tilde => op::UNA_TILD,
                    _ => {
                        self.diag(
                            Severity::Error,
                            pos,
                            format!("unary `{}` has no opcode", p.as_str()),
                        );
                        return;
                    }
                };
                self.emit(b, pos);
            }
            Expr::PreIncDec { inc, expr, .. } => {
                self.expr(g, expr);
                self.emit(if *inc { op::INC } else { op::DEC }, pos);
            }
            Expr::PostIncDec { inc, expr, .. } => {
                self.expr(g, expr);
                self.emit(if *inc { op::INC_POST } else { op::DEC_POST }, pos);
            }
            Expr::Binary {
                op: p, lhs, rhs, ..
            } => self.binary(g, *p, lhs, rhs, pos),
            Expr::Assign {
                op: p,
                target,
                value,
                ..
            } => self.assign(g, *p, target, value, pos),
            Expr::Call { name, args, .. } => self.call(g, name, None, args, pos),
            Expr::MethodCall {
                recv, name, args, ..
            } => self.call(g, name, Some(recv), args, pos),
            Expr::Index { base, index, .. } => {
                self.expr(g, base);
                self.expr(g, index);
                self.emit(op::PUSH_ARRAY_INDEX, pos);
            }
            Expr::Member { base, name, .. } => self.member(g, base, name, pos),
        }
    }

    fn member(&mut self, g: &mut ScriptGen, base: &Expr, name: &str, pos: Pos) {
        // `a.length` is not a field: it is `OP_PUSH_ARRAY_LENGTH`. [measured]
        if name.eq_ignore_ascii_case("length") && !self.is_struct_with_field(g, base, name) {
            self.expr(g, base);
            self.emit(op::PUSH_ARRAY_LENGTH, pos);
            return;
        }
        self.expr(g, base);
        match self.field_index(g, base, name) {
            Some(i) => self.emit1(op::PUSH_STRUCT_FIELD, i as u32, pos),
            None => {
                self.diag(
                    Severity::Error,
                    pos,
                    format!("cannot resolve field `.{name}`; refusing an invented field index"),
                );
            }
        }
    }

    fn struct_of(&self, g: &ScriptGen, e: &Expr) -> Option<&'a StructInfo> {
        let ty = self.static_ty(g, e)?;
        match ty {
            Ty::Struct(i) => self.unit.structs.get(i),
            Ty::Array(inner) => match *inner {
                Ty::Struct(i) => self.unit.structs.get(i),
                _ => None,
            },
            _ => None,
        }
    }

    /// The declared type of an expression, where one is knowable. BHS does not check
    /// types, so this is only used to pick a struct field index and to decide `.length`.
    fn static_ty(&self, g: &ScriptGen, e: &Expr) -> Option<Ty> {
        match e {
            Expr::Str(..) | Expr::LocStr(..) => Some(Ty::Scalar(ScriptTy::Str)),
            Expr::Int(..) => Some(Ty::Scalar(ScriptTy::Int)),
            Expr::Real(..) => Some(Ty::Scalar(ScriptTy::Real)),
            Expr::Cast { ty, .. } => Some(self.unit.resolve_type(Some(ty))),
            Expr::Binary {
                op: P::Plus,
                lhs,
                rhs,
                ..
            } => match (self.static_ty(g, lhs), self.static_ty(g, rhs)) {
                (Some(Ty::Scalar(ScriptTy::Str)), _) | (_, Some(Ty::Scalar(ScriptTy::Str))) => {
                    Some(Ty::Scalar(ScriptTy::Str))
                }
                (a, _) => a,
            },
            Expr::Name(n, _) => g.lookup(n).map(|s| g.slot_ty(s)),
            Expr::Index { base, .. } => match self.static_ty(g, base)? {
                Ty::Array(inner) => Some(*inner),
                _ => None,
            },
            Expr::Member { base, name, .. } => {
                let s = self.struct_of(g, base)?;
                let i = s.field_index(name)?;
                Some(s.fields[i].ty.clone())
            }
            Expr::Call { name, args, .. } => self
                .unit
                .script_by_name(name)
                .map(|s| s.ret.clone())
                .or_else(|| resolve_builtin(name, args.len()).map(|d| Ty::Scalar(d.ret))),
            _ => None,
        }
    }

    fn is_struct_with_field(&self, g: &ScriptGen, base: &Expr, name: &str) -> bool {
        self.struct_of(g, base)
            .and_then(|s| s.field_index(name))
            .is_some()
    }

    fn field_index(&self, g: &ScriptGen, base: &Expr, name: &str) -> Option<usize> {
        if let Some(s) = self.struct_of(g, base) {
            return s.field_index(name);
        }
        // The base's type is unknown (an untyped or implicitly declared variable). Fall
        // back to a unique field name across all structs in the unit; ambiguity is a
        // diagnostic, not a silent pick.
        let mut hit = None;
        for s in &self.unit.structs {
            if let Some(i) = s.field_index(name) {
                if hit.is_some() {
                    return None;
                }
                hit = Some(i);
            }
        }
        hit
    }

    fn binary(&mut self, g: &mut ScriptGen, p: P, lhs: &Expr, rhs: &Expr, pos: Pos) {
        // `&&` and `||` short-circuit through the dedicated jumps: the handler pops the
        // operand, and on the deciding value pushes it BACK and jumps, so the result of
        // `a && b` is `a` when `a` is false and `b` otherwise — not a normalised 0/1.
        // Every conditional opcode calls `is_false`, so the difference only shows if the
        // result is stored. [measured, handlers 0x009e142d / 0x009e1477]
        if p == P::AndAnd || p == P::OrOr {
            let l_end = g.new_label();
            self.expr(g, lhs);
            let b = if p == P::AndAnd {
                op::JUMP_IF_SC_FALSE
            } else {
                op::JUMP_IF_SC_TRUE
            };
            self.emit_jump(g, b, l_end, pos);
            self.expr(g, rhs);
            self.place(g, l_end);
            return;
        }
        self.expr(g, lhs);
        self.expr(g, rhs);
        // **There is no runtime coercion.** `ScriptInt::do_operator` and
        // `ScriptString::do_operator` both begin `if (rhs && rhs->type != this->type)
        // run_time_error(…); return this;` — a mismatched pair produces an error and
        // leaves the left operand *unchanged*. The retail compiler is expected to have
        // inserted the conversion (`SyntaxNode::auto_cast` `0x009dfee0`), whose failure
        // message is "Can't convert \"$TYPE0\" to \"$TYPE1\". Explicit cast required".
        // [measured]
        //
        // We reproduce only the case the shipped corpus actually needs and that is
        // unambiguous: a string on the left. `auto_pause.bhs` writes
        // `"Game will pause for: " + (pause_duration / 1000) + " seconds"`, which cannot
        // run at all without it. The int-vs-real rules are NOT reproduced — the corpus
        // writes those casts by hand, which is itself evidence that `auto_cast` declines
        // them. See `docs/tracks/bhs-compiler.md` §"Open".
        if matches!(self.static_ty(g, lhs), Some(Ty::Scalar(ScriptTy::Str))) {
            let rt = self.static_ty(g, rhs);
            if !matches!(rt, Some(Ty::Scalar(ScriptTy::Str)) | None) {
                self.emit1(op::CAST, ScriptTy::Str.tag(), pos);
                self.auto_casts += 1;
            }
        }
        let b = match p {
            P::Eq => op::EQ,
            P::Ne => op::NE,
            P::Lt => op::LESS,
            P::Gt => op::GREA,
            P::Le => op::LE,
            P::Ge => op::GE,
            P::Plus => op::ADD,
            P::Minus => op::SUBT,
            P::Star => op::MUL,
            P::Slash => op::DIV,
            P::Percent => op::MOD,
            P::Pow => op::POW,
            P::Amp => op::AND_BIT,
            P::Pipe => op::OR_BIT,
            P::Caret => op::XOR_BIT,
            P::Shl => op::SHL,
            P::Shr => op::SHR,
            _ => {
                self.diag(
                    Severity::Error,
                    pos,
                    format!("binary `{}` has no opcode", p.as_str()),
                );
                return;
            }
        };
        self.emit(b, pos);
    }

    fn assign(&mut self, g: &mut ScriptGen, p: P, target: &Expr, value: &Expr, pos: Pos) {
        let b = match p {
            P::Assign => op::ASSIGN,
            P::AddAssign => op::ADD_ASSIGN,
            P::SubAssign => op::SUB_ASSIGN,
            P::MulAssign => op::MUL_ASSIGN,
            P::DivAssign => op::DIV_ASSIGN,
            P::ModAssign => op::MOD_ASSIGN,
            P::PowAssign => op::POW_ASSIGN,
            P::LeftAssign => op::LEFT_ASSIGN,
            P::RightAssign => op::RIGHT_ASSIGN,
            P::AndAssign => op::AND_ASSIGN,
            P::XorAssign => op::XOR_ASSIGN,
            P::OrAssign => op::OR_ASSIGN,
            _ => {
                self.diag(Severity::Error, pos, "not an assignment operator");
                return;
            }
        };
        // `a.length = n` is the one assignment that is not `OP_ASSIGN`.
        if let Expr::Member { base, name, .. } = target {
            if p == P::Assign
                && name.eq_ignore_ascii_case("length")
                && !self.is_struct_with_field(g, base, name)
            {
                self.expr(g, value);
                self.expr(g, base);
                self.emit(op::SET_ARRAY_LENGTH, pos);
                return;
            }
        }
        if ASSIGN_EMITS_VALUE_THEN_TARGET {
            self.expr(g, value);
            self.lvalue(g, target);
        } else {
            self.lvalue(g, target);
            self.expr(g, value);
        }
        self.emit(b, pos);
    }

    /// Push a *reference* to the assignment target. `OP_PUSH` of a variable already
    /// pushes the variable's own storage rather than a copy, which is how assignment
    /// mutates it; the aggregate forms have their own opcodes. [measured]
    fn lvalue(&mut self, g: &mut ScriptGen, e: &Expr) {
        match e {
            Expr::Name(..) | Expr::Member { .. } => self.expr(g, e),
            Expr::Index { base, index, .. } => {
                self.expr(g, base);
                self.expr(g, index);
                // The measured write form grows through blank_base when needed.
                self.emit(op::CREATE_ARRAY_INDEX, e.pos());
            }
            other => {
                self.diag(
                    Severity::Error,
                    other.pos(),
                    "assignment target is not a variable",
                );
                self.expr(g, other);
            }
        }
    }

    fn call(
        &mut self,
        g: &mut ScriptGen,
        name: &str,
        recv: Option<&Expr>,
        args: &[Expr],
        pos: Pos,
    ) {
        // `enable_trigger` / `disable_trigger` are **compiler intrinsics, not builtins**.
        //
        // The shipped corpus calls them 2081 times, and they appear in neither the 873
        // engine registrations nor `ron-data/scriptfunctions.xml`. The only opcodes that
        // touch `Script::trigger_bits` are `OP_BIT_UNSET` (0x3d, `bts` — sets the bit)
        // and `OP_BIT_SET` (0x3c, `btr` — clears it), and both take a *literal trigger
        // index*, which nothing but the compiler can derive from a name. Meanwhile
        // `is_trigger_enabled(String)` **is** a registered builtin (index 17) because a
        // by-name lookup at runtime needs `Script::trigger_names`. That asymmetry is the
        // tell. [measured that they are absent; [inferred] that this is the lowering]
        if recv.is_none() && args.len() == 1 {
            let enable = name.eq_ignore_ascii_case("enable_trigger");
            let disable = name.eq_ignore_ascii_case("disable_trigger");
            if enable || disable {
                // The argument is a *trigger name*, spelled either as a string literal
                // (the usual form) or as a bare identifier. The bare form is decisive
                // evidence that this is an intrinsic and not a host call:
                // `scenario/Scripts/Auto_Pause/auto_pause.bhs` writes
                // `disable_trigger(pause_time_up)` where `pause_time_up` is a trigger
                // declared later in the same script and is not a variable anywhere.
                match &args[0] {
                    Expr::Str(s, _) | Expr::LocStr(s, _) => {
                        let idx = g.trigger_index(s);
                        let b = if enable { op::BIT_SET } else { op::BIT_CLEAR };
                        self.emit1(b, idx, pos);
                    }
                    Expr::Name(n, _) => {
                        let idx = g.trigger_index(n);
                        let b = if enable { op::BIT_SET } else { op::BIT_CLEAR };
                        self.emit1(b, idx, pos);
                    }
                    other => self.diag(
                        Severity::Error,
                        other.pos(),
                        format!(
                            "`{name}` needs a trigger name (a string literal or a bare \
                             identifier); it resolves to a bit index at compile time"
                        ),
                    ),
                }
                return;
            }
        }

        // Arguments are pushed **left to right**: `VirtualMachine::call_func` pops `argc`
        // values and reverses them. This ordering is observable wherever an argument
        // draws RNG (`rand_int` is builtin 9 and the shipped scripts call it 566 times),
        // so it is not a free choice. [measured]
        let argc = args.len() + usize::from(recv.is_some());
        if let Some(r) = recv {
            self.expr(g, r);
        }
        for a in args {
            self.expr(g, a);
        }

        // A script in this unit wins over a builtin of the same name: the compiler
        // resolves user scripts first, which is how a script may shadow an engine
        // function. [inferred]
        if let Some(s) = self.unit.script_by_name(name) {
            if s.arity != argc {
                self.diag(
                    Severity::Error,
                    pos,
                    format!(
                        "script `{}` takes {} argument(s), called with {argc}",
                        s.name, s.arity
                    ),
                );
            }
            if s.file == self.file {
                self.emit1(op::CALL, s.index_in_file as u32, pos);
            } else {
                match self.unit.files[self.file]
                    .includes
                    .iter()
                    .position(|&i| i == s.file)
                {
                    Some(li) => {
                        self.emit2(op::CALL_INCLUDE, s.index_in_file as u32, li as u32, pos)
                    }
                    None => self.diag(
                        Severity::Error,
                        pos,
                        format!(
                            "script `{}` is not reachable from this file's includes",
                            s.name
                        ),
                    ),
                }
            }
            return;
        }

        match resolve_builtin(name, argc) {
            Some(d) => {
                if d.arity as usize == argc {
                    self.emit1(op::CALL_GAME, d.index, pos);
                } else {
                    // Varargs: `parse(fmt, …)`. `OP_CALL_GAME_VARIED` carries the count.
                    self.emit2(0x39, d.index, argc as u32, pos);
                }
            }
            None => {
                let known = !crate::sema::builtin_overloads(name).is_empty();
                let msg = if known {
                    let arities: Vec<String> = crate::sema::builtin_overloads(name)
                        .iter()
                        .map(|d| d.arity.to_string())
                        .collect();
                    format!(
                        "builtin `{name}` called with {argc} argument(s); registered arities are {}",
                        arities.join("/")
                    )
                } else {
                    format!("unknown function `{name}`")
                };
                self.diag(Severity::Error, pos, msg);
            }
        }
    }

    /// Resolve a bare name to storage.
    ///
    /// Order: lexical scope, then a `labels` constant, then an **implicit declaration**.
    ///
    /// Implicit declaration is not a convenience we invented, and it is not a recovery
    /// path either: it is a *grammar rule*. The retail compiler is flex+bison, and bison
    /// rule 92 is an **empty type-specifier** whose action loads the built-in `int`
    /// `SymType` from `[0x00EBE408]+0x00` into `script_compiler.current_data_type`; rule
    /// 93 then calls `SymTable::create_var_type`. Driving the shipped LALR tables on
    /// `scenario { run_once { NAME = INT ; } }` reduces 92 then 93 and ACCEPTs. So
    /// `j = 1;` with no declaration of `j` **declares `j` as an `int`**. [measured]
    ///
    /// `VarType::init` (`0x009da870`) then picks the storage: inside a script body it
    /// takes a frame slot (a **local**, fresh every frame); at file scope it takes a
    /// file global, or a file static when written `static`. Every shipped implicit
    /// declaration is inside a body, so they are all locals. [measured]
    ///
    /// Each one is still reported as a note so the count stays visible.
    fn name_slot(&mut self, g: &mut ScriptGen, n: &str, pos: Pos) -> Slot {
        if let Some(s) = g.lookup(n) {
            return s;
        }
        if let Some(v) = g.labels.get(n).copied() {
            let c = self.intern(Value::Int(v as i32));
            return Slot::Const(c);
        }
        self.diag(
            Severity::Note,
            pos,
            format!("`{n}` is used with no declaration; compiled as a fresh local"),
        );
        g.declare_local(n, Ty::Scalar(ScriptTy::Int))
    }
}

fn array_inner(ty: &Ty) -> &Ty {
    match ty {
        Ty::Array(inner) => inner,
        other => other,
    }
}

/// `String::generate_hash` (`0x00a1b6b0`) case-insensitive result, which
/// `SymTable::add_data_type` stores as a BHS type tag.
///
/// BHS identifiers are ASCII; applying ASCII lowercase here matches retail's
/// `towlower` for the language's admitted names.
fn bhs_type_hash(name: &str) -> u32 {
    const T: [u32; 50] = [
        127, 811, 1597, 2131, 2749, 4759, 5527, 5953, 8117, 9539, 10273, 10753, 11159, 12301,
        13217, 14207, 15413, 17681, 18661, 19013, 21089, 22051, 25111, 25801, 27457, 28057, 29581,
        30809, 32611, 34469, 36067, 37511, 38723, 40093, 41983, 43321, 45083, 47431, 49667, 50767,
        53453, 55469, 57193, 59369, 61987, 65071, 73421, 77849, 84223, 89009,
    ];
    let units: Vec<u16> = name.encode_utf16().collect();
    let mut remaining = units.len() as u32;
    let mut hash = 0u32;
    for (index, &unit) in units.iter().enumerate().rev() {
        let c = if (b'A' as u16..=b'Z' as u16).contains(&unit) {
            unit + (b'a' - b'A') as u16
        } else {
            unit
        } as u32;
        hash = hash
            .wrapping_add(T[c as usize % T.len()].wrapping_mul(remaining))
            .wrapping_add(c.wrapping_mul(T[index % T.len()]));
        remaining -= 1;
    }
    hash
}

/// `Script::script_type` (+200). The three qualifiers the corpus uses are given stable
/// small ids; the retail encoding is unread, so this is ours until it can be diffed.
/// [inferred]
fn script_type_tag(s: Option<&str>) -> u32 {
    match s.map(|x| x.to_ascii_lowercase()).as_deref() {
        Some("ai") => 1,
        Some("scenario") => 2,
        Some("conquest") => 3,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use don_bhs::host::NullHost;
    use don_bhs::opcode;
    use don_bhs::vm::Vm;

    /// Walk an emitted code array with the engine's own decoder. Every byte must be a
    /// legal opcode and every operand must be inside the array — the check that catches
    /// a mis-sized emission or an unpatched jump.
    pub fn decode_all(code: &[u8]) -> Result<usize, String> {
        let mut i = 0;
        let mut n = 0;
        while i < code.len() {
            let d = opcode::decode(code[i]).ok_or(format!("bad opcode {:#04x} at {i}", code[i]))?;
            if d.code == opcode::OP_ERROR_TOKEN {
                return Err(format!("OP_ERROR_TOKEN at {i}"));
            }
            let len = 1 + 4 * d.operands.len();
            if i + len > code.len() {
                return Err(format!("truncated {} at {i}", d.name));
            }
            i += len;
            n += 1;
        }
        Ok(n)
    }

    fn compile_src(src: &str) -> (Program, Vec<Diag>, Stats) {
        // A unique directory per call: these tests run in parallel and would otherwise
        // compile each other's source.
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("bhscc-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.bhs");
        std::fs::write(&p, src).unwrap();
        let u = crate::sema::analyze(&p, &crate::sema::IncludePath::default()).unwrap();
        compile(&u)
    }

    fn run_src(src: &str) -> Value {
        let (mut prog, diags, _) = compile_src(src);
        assert!(
            diags.iter().all(|d| d.severity != Severity::Error),
            "compile errors: {diags:#?}"
        );
        let mut host = NullHost;
        let mut vm = Vm::new(&mut prog, &mut host);
        vm.run_script(0, "t")
            .expect("compiler emitted an implementation gap")
            .returned
            .expect("test script did not return")
    }

    #[test]
    fn emitted_code_decodes_with_the_engine_table() {
        let (prog, _, _) = compile_src(
            "scenario {\n\
               int i;\n\
               static int s = 3;\n\
               for (i = 0; i < 4; i++) { if (i == 2) break; }\n\
               switch (s) { case 1: case 2: i = 1; break; default: i = 0; }\n\
             }\n",
        );
        let n = decode_all(&prog.files[0].code).unwrap();
        assert!(n > 10, "suspiciously short program: {n} instructions");
    }

    #[test]
    fn compile_errors_poison_output_instead_of_exposing_a_guess() {
        let (prog, diags, _) =
            compile_src("struct Pair { int value; }; int scenario { Pair p; return p.missing; }");
        assert!(diags.iter().any(|d| d.severity == Severity::Error));
        assert_eq!(prog.files[0].code, vec![op::ERROR_TOKEN]);
        assert!(prog.files[0].scripts.iter().all(|s| s.entry == 0));
    }

    #[test]
    fn emitted_aggregate_code_executes_with_retail_stack_order() {
        assert_eq!(
            run_src("int scenario { int a[] = [ 10, 20, 30 ]; return a[1]; }"),
            Value::Int(20)
        );
        assert_eq!(
            run_src("int scenario { int a[3]; a[2] = 7; return a[2]; }"),
            Value::Int(7)
        );
        assert_eq!(
            run_src(
                "struct Pair { int a; int b; }; \
                 int scenario { Pair p; p.b = 7; return p.b; }"
            ),
            Value::Int(7)
        );
    }

    #[test]
    fn user_type_tags_use_retail_string_hash() {
        assert_eq!(bhs_type_hash("int"), ScriptTy::Int.tag());
        assert_eq!(bhs_type_hash("FLOAT"), ScriptTy::Real.tag());
        assert_eq!(bhs_type_hash("String"), ScriptTy::Str.tag());
        assert_eq!(bhs_type_hash("Pair"), bhs_type_hash("pair"));
    }

    #[test]
    fn static_initialiser_is_guarded_by_jump_if_inited() {
        let (prog, _, _) = compile_src("scenario { static int s = 3; }");
        assert_eq!(prog.files[0].code[0], op::JUMP_IF_INITED);
        assert_eq!(
            prog.files[0].scripts[0].static_var_names,
            vec!["s".to_string()]
        );
    }

    #[test]
    fn void_script_emits_no_tail_value() {
        let (prog, _, _) = compile_src("void scenario f() { }\nscenario { f(); }");
        let f = prog.files[0]
            .scripts
            .iter()
            .find(|s| s.name == "f")
            .unwrap();
        // The whole body of a void script that does nothing is one OP_RETURN.
        assert_eq!(prog.files[0].code[f.entry as usize], op::RETURN);
    }

    #[test]
    fn jumps_all_land_inside_the_code_array() {
        let (prog, _, _) =
            compile_src("scenario { int i = 0; while (i < 3) { i++; if (i == 2) continue; } }");
        let code = &prog.files[0].code;
        let mut i = 0;
        while i < code.len() {
            let d = opcode::decode(code[i]).unwrap();
            for (k, o) in d.operands.iter().enumerate() {
                if matches!(o, opcode::OperandKind::CodeOffset) {
                    let at = i + 1 + 4 * k;
                    let t = u32::from_le_bytes(code[at..at + 4].try_into().unwrap()) as usize;
                    assert!(t <= code.len(), "jump to {t} beyond {} bytes", code.len());
                }
            }
            i += 1 + 4 * d.operands.len();
        }
    }
}
