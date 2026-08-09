//! The BHS stack virtual machine.
//!
//! This mirrors `RunTimeEnv::exec` (`0x009c3600`) and
//! `VirtualMachine::execute_next` (`0x009e0840`) instruction for instruction where
//! we have read them, and says so explicitly where we have not.
//!
//! # The fetch loop, [measured] from `0x009c3600`
//!
//! ```text
//! loop {
//!     this->script_status = 0;
//!     while (check_vm_running()) {
//!         vm  = this->cur_vm;
//!         op  = vm->code[vm->bip];      // ONE byte
//!         vm->bip += 1;
//!         vm->execute_next(op);
//!         this->bytecodes_executed += 1;
//!     }
//!     ...
//! }
//! ```
//!
//! `RunTimeEnv` layout, verbatim from the PDB: `stack_frames` +0, `run_stack` +16,
//! `cur_vm` +32, `return_value` +36, `bytecodes_executed` +40, **`err_count` +44**,
//! **`script_status` +48**. The object is `RunTimeEnv script_run_time` at
//! `0x00ebeeb0` and has a dedicated checksum entry point,
//! `CheckSums::check_script_run_time` — which is why script state is sim-critical
//! and why a faithful VM is a prerequisite for validating any scripted game
//! against a replay.
//!
//! # Runtime errors are fatal to the run, not warnings
//!
//! `RunTimeEnv::run_time_error(String const&)` (`0x009c31e0`) ends with
//! `err_count++; script_status = 3` [measured, `0x009c31e0` tail]. `exec` clears
//! `script_status` at the top of each outer iteration and its inner loop is gated on
//! `check_vm_running()`, so raising an error **stops the current script run**. This
//! VM reproduces that: a retail-reachable error is reported in
//! [`RunOutcome::error`] and execution stops, while a gap in *our* implementation is
//! a [`VmError`] — the two are never conflated.
//!
//! # Variable references
//!
//! Every variable-shaped operand is a tagged 32-bit word, decoded by
//! `VirtualMachine::get_value` (`0x004d1010`), read here at the instruction level
//! because the decompiler dropped one of the two masks: [measured]
//!
//! ```text
//! test edx, 0x20000000 ; jne -> const_pool[ref & 0xdfffffff]   (ScriptFile+0x48)
//! test edx, 0x40000000 ; jne -> static_vars[ref & 0xbfffffff]  (Script+0x4c, bound Script+0x40)
//! otherwise            ->        locals[ref]                   (VM+0x28,     bound VM+0x1c)
//! ```
//!
//! # Frames
//!
//! `RunTimeEnv::call_script` (`0x009c3840`) recycles a `VirtualMachine`, calls
//! `VirtualMachine::init(file, script, &run_stack)`, then sets
//! `expected_stack_size -= script->params.count` and `+= 1` unless the callee is
//! void. `close_frame` (`0x009c36c0`) asserts the stack landed exactly there and
//! raises `run_time_error` otherwise. The run stack is **shared across frames** —
//! arguments are passed by leaving them on it.
//!
//! # Operand order, settled
//!
//! The assignment handler and the binary-operator handler take their receiver from
//! **opposite ends**, and the two were read at the instruction level to settle it:
//!
//! - `0x009e10e9` (assignment family) calls `Stack::pop` twice; the **first** popped
//!   becomes `ecx` (the `this` of `do_operator`) and is saved at `[ebp+8]`, which the
//!   shared epilogue at `0x009e0a11` pushes back. So the **assignment target is on
//!   top of the stack** and the value beneath it.
//! - `0x009e11e5` (all 19 binary operators) pops inline, twice; the **first** popped
//!   becomes the `rhs` argument and the **second** becomes `ecx`. So the **left
//!   operand is beneath the right**, the ordinary stack-machine convention.
//!
//! `OP_SET_ARRAY_LENGTH` (`0x009e0f0c`) corroborates the first: it too takes the
//! assignment *target* (the array) from the top of the stack and the value beneath.

use crate::builtin_table::{builtin, BuiltinDecl};
use crate::host::{survey_err_return, Coverage, Host, HostError};
use crate::opcode::{self, OP_POP};
use crate::program::{Program, ValueWalkMeta, ValueWalkNested};
use crate::value::{cell, Cell, Obj, OpError, ScriptTy, Value};

/// A decoded variable reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarRef {
    /// `ScriptFile::const_pool[i]` — immutable.
    Const(u32),
    /// `Script::static_vars[i]` — **persists across frames**, and is the entire
    /// cross-frame memory of a script.
    Static(u32),
    /// The current frame's local slot.
    Local(u32),
}

impl VarRef {
    /// Exactly the tag test in `VirtualMachine::get_value`.
    #[inline]
    pub fn decode(raw: u32) -> VarRef {
        if raw & 0x2000_0000 != 0 {
            VarRef::Const(raw & 0xdfff_ffff)
        } else if raw & 0x4000_0000 != 0 {
            VarRef::Static(raw & 0xbfff_ffff)
        } else {
            VarRef::Local(raw)
        }
    }

    pub fn encode(self) -> u32 {
        match self {
            VarRef::Const(i) => i | 0x2000_0000,
            VarRef::Static(i) => i | 0x4000_0000,
            VarRef::Local(i) => i,
        }
    }
}

/// A run-stack entry.
///
/// The engine's stack holds `ScriptType*` — *pointers to storage*, not copies.
/// `OP_PUSH` pushes what `get_value` returned and assignment mutates through it;
/// `OP_PUSH_ARRAY_INDEX` pushes `&obj->values[i]`, the element slot itself. Both
/// aliases must be representable or assignment silently becomes a no-op.
#[derive(Debug, Clone)]
pub enum Slot {
    Val(Value),
    /// A bound alias for a local / static / constant. Retail's run stack holds raw
    /// `ScriptType*`, so a reference pushed by a caller keeps naming the caller's
    /// storage after `OP_CALL` installs the callee frame. Keeping only a `VarRef`
    /// here would reinterpret `local[0]` relative to the callee and silently break
    /// every shipped `ref` parameter.
    Ref(BoundRef),
    /// An alias for one element slot inside an aggregate.
    Cell(Cell),
}

/// The storage identity behind a run-stack pointer.
///
/// `VarRef` is an encoded bytecode operand and is relative to the *currently
/// executing* VM. `BoundRef` is the corresponding runtime pointer identity. This
/// distinction is required by the measured ref-parameter prologue: retail emits
/// `OP_INIT local[n]` for `ref` and `OP_INIT_COPY local[n]` for by-value, so the
/// former stores the caller's pointer in the callee's local array unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundRef {
    Const {
        file: usize,
        index: u32,
    },
    Static {
        file: usize,
        script: usize,
        index: u32,
    },
    Local {
        frame: usize,
        index: u32,
    },
}

#[derive(Debug, Clone)]
struct Frame {
    file: usize,
    script: usize,
    /// `VirtualMachine::bip` (+12) — a **byte** offset into the file's code array.
    bip: usize,
    /// `VirtualMachine::vars` (+24) — the local slots.
    /// `VirtualMachine::vars`: raw `ScriptType*` slots. An entry can therefore be
    /// an owned value or an alias into an older frame. Retail ref parameters rely
    /// on the latter; flattening this to `Value` turns mutation into pass-by-value.
    locals: Vec<Option<Slot>>,
    /// `VirtualMachine::expected_stack_size` (+16).
    expected_stack: usize,
    /// `VirtualMachine::flags` bit 3 — the callee returns void.
    void_return: bool,
}

/// Why the VM stopped, when the reason is a **gap in this implementation**.
///
/// A condition retail also rejects is *not* one of these — see [`RuntimeError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmError {
    /// `execute_next`'s `cmp edi, 0x47 / ja` bound check failed.
    BadOpcode(u8),
    /// Ran off the end of the code array.
    CodeOverrun(usize),
    /// A pop on an empty run stack.
    StackUnderflow,
    /// `VirtualMachine::get_value`'s bounds check failed.
    BadVarRef(VarRef),
    /// `VirtualMachine::call_func`'s `func_index >= funcs.count` check.
    BadBuiltinIndex(u32),
    BadFileIndex(usize),
    BadScriptIndex(usize),
    /// An opcode this implementation has not recovered well enough to run. Named
    /// rather than silently wrong — the whole point of the coverage discipline.
    Unimplemented(&'static str),
    /// A script reached a builtin that the supplied host does not implement.
    UnimplementedBuiltin {
        index: u32,
        name: &'static str,
    },
    /// A host claimed a builtin but rejected the values it received.
    HostRejected {
        index: u32,
        name: &'static str,
        reason: &'static str,
    },
    /// The instruction budget was exhausted (our addition; the engine has no such
    /// limit, it has a `break_callback` instead).
    BudgetExhausted,
}

/// A `RunTimeEnv::run_time_error` — a condition retail itself reports.
///
/// Raising one increments `err_count` and sets `script_status = 3`, which ends the
/// run. The message text is ours; the *condition* is the engine's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError {
    pub message: String,
    /// `VirtualMachine::bip` at the start of the faulting instruction.
    pub bip: usize,
    pub op: u8,
}

impl From<OpError> for RuntimeError {
    fn from(e: OpError) -> RuntimeError {
        RuntimeError {
            message: match e {
                OpError::DivideByZero => "Can't Divide 0".into(),
                OpError::TypeMismatch { lhs, rhs } => {
                    format!("operator type mismatch: lhs {lhs:#x}, rhs {rhs:#x}")
                }
                OpError::BadOperand(m) => m.into(),
                OpError::NotAnArray => "variable is not an array".into(),
                OpError::IndexOutOfRange(i) => format!("array index {i} out of range"),
            },
            bip: 0,
            op: 0,
        }
    }
}

/// Outcome of a top-level `run_script`.
#[derive(Debug, Clone, PartialEq)]
pub struct RunOutcome {
    pub returned: Option<Value>,
    /// `RunTimeEnv::bytecodes_executed` for this run.
    pub bytecodes_executed: u64,
    /// `RunTimeEnv::err_count` accumulated over the VM's lifetime.
    pub err_count: u32,
    /// The error that stopped the run, if any.
    pub error: Option<RuntimeError>,
}

impl RunOutcome {
    pub fn ok(&self) -> bool {
        self.error.is_none()
    }
}

pub struct Vm<'a, H: Host> {
    prog: &'a mut Program,
    host: &'a mut H,
    stack: Vec<Slot>,
    frames: Vec<Frame>,
    returned: Option<Value>,
    pub coverage: Coverage,
    pub bytecodes_executed: u64,
    /// `RunTimeEnv::err_count` (+44).
    pub err_count: u32,
    budget: u64,
    missing_builtin_policy: MissingBuiltinPolicy,
}

/// Policy for implementation gaps at the simulation host boundary.
///
/// Fidelity execution is [`Fail`](Self::Fail). [`Survey`](Self::Survey) is an
/// explicitly lossy debt-discovery mode: it substitutes retail's rejected-call
/// value and therefore must not be used for gameplay, replay validation, or hashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MissingBuiltinPolicy {
    #[default]
    Fail,
    Survey,
}

/// Control flow out of one instruction.
enum Step {
    Continue,
    /// A `run_time_error` was raised; `exec`'s loop stops.
    Abort(RuntimeError),
}

impl<'a, H: Host> Vm<'a, H> {
    pub fn new(prog: &'a mut Program, host: &'a mut H) -> Self {
        Vm {
            prog,
            host,
            stack: Vec::new(),
            frames: Vec::new(),
            returned: None,
            coverage: Coverage::default(),
            bytecodes_executed: 0,
            err_count: 0,
            // The engine has no instruction cap; this exists so a malformed program
            // fails a test instead of hanging a lane. Raise it freely.
            budget: 50_000_000,
            missing_builtin_policy: MissingBuiltinPolicy::Fail,
        }
    }

    pub fn with_budget(mut self, budget: u64) -> Self {
        self.budget = budget;
        self
    }

    /// Opt into lossy missing-builtin coverage collection.
    pub fn with_missing_builtin_policy(mut self, policy: MissingBuiltinPolicy) -> Self {
        self.missing_builtin_policy = policy;
        self
    }

    /// `RunTimeEnv::run_script(name, ...)` for the zero-argument, per-frame form
    /// that `Game::do_frame` uses: `run_script(&script_run_time, game+0x500, 0)`.
    pub fn run_script(&mut self, file: usize, name: &str) -> Result<RunOutcome, VmError> {
        let file_ref = self
            .prog
            .files
            .get(file)
            .ok_or(VmError::BadFileIndex(file))?;
        let idx = file_ref
            .find_script(name)
            .ok_or(VmError::BadScriptIndex(usize::MAX))?;
        self.run_script_index(file, idx, &[])
    }

    pub fn run_script_index(
        &mut self,
        file: usize,
        script: usize,
        args: &[Value],
    ) -> Result<RunOutcome, VmError> {
        let slots = args.iter().cloned().map(Slot::Val).collect();
        self.run_script_slots(file, script, slots)
    }

    /// Run a script through the external `RunTimeEnv::run_script` boundary and
    /// copy mutations of `ref` parameters back into the caller's values.
    ///
    /// Retail's boundary passes `ScriptType*` arguments into the shared run stack;
    /// the shipped compiler's OP_INIT prologue keeps that pointer for a `ref`
    /// parameter and OP_INIT_COPY duplicates it for a value parameter. `&[Value]`
    /// cannot express the observable write-back, so this explicit mutable entry
    /// point supplies stable cells. The bytecode—not the metadata—decides whether
    /// a cell changes: OP_INIT aliases it, OP_INIT_COPY leaves it untouched. This
    /// mirrors the retail VM, which does not consult `Script::refs` while calling.
    pub fn run_script_index_mut(
        &mut self,
        file: usize,
        script: usize,
        args: &mut [Value],
    ) -> Result<RunOutcome, VmError> {
        self.prog
            .files
            .get(file)
            .and_then(|f| f.scripts.get(script))
            .ok_or(VmError::BadScriptIndex(script))?;
        let cells: Vec<Cell> = args.iter().cloned().map(cell).collect();
        let slots = cells.iter().cloned().map(Slot::Cell).collect();
        let out = self.run_script_slots(file, script, slots)?;
        for (i, arg) in args.iter_mut().enumerate() {
            *arg = cells[i].borrow().clone();
        }
        Ok(out)
    }

    fn run_script_slots(
        &mut self,
        file: usize,
        script: usize,
        args: Vec<Slot>,
    ) -> Result<RunOutcome, VmError> {
        let before = self.bytecodes_executed;
        self.returned = None;
        // Retail callers emit logical arguments right-to-left, leaving parameter 0
        // on top for the forward OP_INIT[_COPY] callee prologue. Mirror that at the
        // external entry boundary, whose API accepts ordinary logical order.
        for a in args.into_iter().rev() {
            self.stack.push(a);
        }
        self.push_frame(file, script)?;
        let mut error = None;
        while !self.frames.is_empty() {
            if self.bytecodes_executed - before > self.budget {
                return Err(VmError::BudgetExhausted);
            }
            match self.step()? {
                Step::Continue => {}
                Step::Abort(e) => {
                    error = Some(e);
                    // script_status = 3 ends the run: unwind every frame.
                    self.frames.clear();
                    break;
                }
            }
            self.bytecodes_executed += 1;
        }
        Ok(RunOutcome {
            returned: self.returned.take(),
            bytecodes_executed: self.bytecodes_executed - before,
            err_count: self.err_count,
            error,
        })
    }

    /// `RunTimeEnv::run_time_error`: `err_count++; script_status = 3`.
    fn rte(&mut self, msg: impl Into<String>, bip: usize, op: u8) -> Step {
        self.err_count += 1;
        Step::Abort(RuntimeError {
            message: msg.into(),
            bip,
            op,
        })
    }

    fn rte_op(&mut self, e: OpError, bip: usize, op: u8) -> Step {
        let mut r: RuntimeError = e.into();
        r.bip = bip;
        r.op = op;
        self.err_count += 1;
        Step::Abort(r)
    }

    // ---------------------------------------------------------------- frames

    fn push_frame(&mut self, file: usize, script: usize) -> Result<(), VmError> {
        let file_ref = self
            .prog
            .files
            .get(file)
            .ok_or(VmError::BadFileIndex(file))?;
        let s = file_ref
            .scripts
            .get(script)
            .ok_or(VmError::BadScriptIndex(script))?;
        let void_return = s.return_type == ScriptTy::Void.tag();
        let arity = s.arity;
        let entry = s.entry as usize;
        if self.stack.len() < arity {
            return Err(VmError::StackUnderflow);
        }
        // `expected_stack_size = stack.len() - nparams (+1 if non-void)`.
        let expected = self.stack.len() - arity + usize::from(!void_return);
        // Arguments remain on the shared run stack. The shipped compiler emits one
        // prologue instruction per parameter to install them in `vars`: OP_INIT_COPY
        // (0x32) for by-value, OP_INIT (0x33) for `ref`. Two shipped-compiler captures
        // of otherwise identical functions differ at exactly that byte. Consuming the
        // arguments here would bypass that measured ownership transition and make the
        // retail bytecode underflow on its first instruction.
        self.frames.push(Frame {
            file,
            script,
            bip: entry,
            locals: Vec::new(),
            expected_stack: expected,
            void_return,
        });
        Ok(())
    }

    fn close_frame(&mut self) -> Result<Step, VmError> {
        let f = self.frames.pop().expect("close_frame with no frame");
        if self.stack.len() != f.expected_stack {
            // `RunTimeEnv::close_frame` (`0x009c36c0`) raises run_time_error here.
            self.err_count += 1;
            return Ok(Step::Abort(RuntimeError {
                message: format!(
                    "wrong number of params: stack {} != expected {}",
                    self.stack.len(),
                    f.expected_stack
                ),
                bip: f.bip,
                op: 0x3e,
            }));
        }
        if self.frames.is_empty() && !f.void_return {
            // `RunTimeEnv::save_return_val` stashed the value; surface it.
            self.returned = match self.stack.pop() {
                Some(s) => Some(self.deref(&s)?),
                None => None,
            };
        }
        Ok(Step::Continue)
    }

    // ------------------------------------------------------------ operands

    fn fetch_u8(&mut self) -> Result<u8, VmError> {
        let f = self.frames.last().unwrap();
        let b = *self.prog.files[f.file]
            .code
            .get(f.bip)
            .ok_or(VmError::CodeOverrun(f.bip))?;
        self.frames.last_mut().unwrap().bip += 1;
        Ok(b)
    }

    /// Read a 4-byte little-endian operand at `bip` and advance. Operands are
    /// **unaligned** — `bip` is a raw byte offset and opcodes are one byte.
    fn fetch_u32(&mut self) -> Result<u32, VmError> {
        let f = self.frames.last().unwrap();
        let c = &self.prog.files[f.file].code;
        let b = c.get(f.bip..f.bip + 4).ok_or(VmError::CodeOverrun(f.bip))?;
        let v = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        self.frames.last_mut().unwrap().bip += 4;
        Ok(v)
    }

    /// Peek the operand without advancing (for the two-operand conditional jumps,
    /// which read operand 2 only on one branch).
    fn peek_u32(&self, at: usize) -> Result<u32, VmError> {
        let f = self.frames.last().unwrap();
        let c = &self.prog.files[f.file].code;
        let b = c.get(at..at + 4).ok_or(VmError::CodeOverrun(at))?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn peek_op(&self) -> Option<u8> {
        let f = self.frames.last().unwrap();
        self.prog.files[f.file].code.get(f.bip).copied()
    }

    fn bip(&self) -> usize {
        self.frames.last().unwrap().bip
    }

    fn set_bip(&mut self, v: usize) {
        self.frames.last_mut().unwrap().bip = v;
    }

    // -------------------------------------------------------------- values

    /// Bind a bytecode-relative variable reference to the same storage identity a
    /// retail `ScriptType*` on the run stack carries.
    fn bind_ref(&self, r: VarRef) -> Result<BoundRef, VmError> {
        let frame = self.frames.len() - 1;
        let f = &self.frames[frame];
        match r {
            VarRef::Const(i) => {
                self.prog.files[f.file]
                    .const_pool
                    .get(i as usize)
                    .ok_or(VmError::BadVarRef(r))?;
                Ok(BoundRef::Const {
                    file: f.file,
                    index: i,
                })
            }
            VarRef::Static(i) => {
                let s = &self.prog.files[f.file].scripts[f.script];
                match s.statics.get(i as usize) {
                    // Note the engine's bound is `static_vars.count > i`; a slot
                    // that exists but is null is legal and is what
                    // OP_JUMP_IF_INITED tests.
                    Some(_) => Ok(BoundRef::Static {
                        file: f.file,
                        script: f.script,
                        index: i,
                    }),
                    None => Err(VmError::BadVarRef(r)),
                }
            }
            VarRef::Local(i) => {
                f.locals.get(i as usize).ok_or(VmError::BadVarRef(r))?;
                Ok(BoundRef::Local { frame, index: i })
            }
        }
    }

    /// `VirtualMachine::set_value` (`0x009e07b0`). Note the engine *grows* the
    /// target array with null entries until the index is in range rather than
    /// failing, so we do the same.
    fn set_slot(
        &mut self,
        r: VarRef,
        v: Slot,
        static_walk_meta: Option<ValueWalkMeta>,
    ) -> Result<(), VmError> {
        let f = self.frames.last().unwrap();
        let (file, script) = (f.file, f.script);
        match r {
            VarRef::Const(_) => Err(VmError::BadVarRef(r)),
            VarRef::Static(i) => {
                // Retail stores a ScriptType* here. Program statics are the public,
                // checksummed value representation rather than a VM-frame pointer
                // graph, so materialise the value. The shipped compiler uses
                // OP_INIT_COPY for aliased initialisers and OP_INIT for fresh
                // temporaries; ref parameters are locals.
                let v = self.deref(&v)?;
                let s = &mut self.prog.files[file].scripts[script];
                if s.statics.len() <= i as usize {
                    s.statics.resize(i as usize + 1, None);
                }
                s.statics[i as usize] = Some(v);

                // `VirtualMachine::set_value` (0x009e07b0) grows the parallel retail
                // static pointer array with nulls, then promotes a non-VM_VAR source
                // to scope 3. Compiler-generated scalar initialisers arrive either as
                // an OP_INIT_COPY duplicate or a fresh VM_TEMP, both with ref_count 0.
                // Aggregate children carry additional ownership transitions that are
                // not recoverable from `Value` alone; mark those slots unavailable so
                // the channel-15 adapter fails closed instead of hashing guessed data.
                if let Some(program_meta) = self.prog.walk_meta_mut() {
                    if let Some(script_meta) = program_meta
                        .files
                        .get_mut(file)
                        .and_then(|file_meta| file_meta.script_meta.get_mut(script))
                    {
                        if script_meta.statics.len() <= i as usize {
                            script_meta.statics.resize(i as usize + 1, None);
                        }
                        script_meta.statics[i as usize] = static_walk_meta;
                    }
                }
                Ok(())
            }
            VarRef::Local(i) => {
                let fr = self.frames.last_mut().unwrap();
                if fr.locals.len() <= i as usize {
                    fr.locals.resize(i as usize + 1, None);
                }
                fr.locals[i as usize] = Some(v);
                Ok(())
            }
        }
    }

    fn deref(&self, s: &Slot) -> Result<Value, VmError> {
        match s {
            Slot::Val(v) => Ok(v.clone()),
            Slot::Ref(BoundRef::Const { file, index }) => self.prog.files[*file]
                .const_pool
                .get(*index as usize)
                .cloned()
                .ok_or(VmError::BadVarRef(VarRef::Const(*index))),
            Slot::Ref(BoundRef::Static {
                file,
                script,
                index,
            }) => self.prog.files[*file].scripts[*script]
                .statics
                .get(*index as usize)
                .cloned()
                .map(|v| v.unwrap_or(Value::Null))
                .ok_or(VmError::BadVarRef(VarRef::Static(*index))),
            Slot::Ref(BoundRef::Local { frame, index }) => {
                let inner = self
                    .frames
                    .get(*frame)
                    .and_then(|f| f.locals.get(*index as usize))
                    .and_then(Option::as_ref)
                    .ok_or(VmError::BadVarRef(VarRef::Local(*index)))?;
                self.deref(inner)
            }
            Slot::Cell(c) => Ok(c.borrow().clone()),
        }
    }

    fn pop(&mut self) -> Result<Slot, VmError> {
        self.stack.pop().ok_or(VmError::StackUnderflow)
    }

    fn pop_value(&mut self) -> Result<Value, VmError> {
        let s = self.pop()?;
        self.deref(&s)
    }

    fn peek_value(&self) -> Result<Value, VmError> {
        let s = self.stack.last().ok_or(VmError::StackUnderflow)?;
        self.deref(s)
    }

    /// Write through a stack slot, which is how assignment mutates a variable.
    fn store(&mut self, target: &Slot, v: Value) -> Result<(), VmError> {
        match target {
            Slot::Ref(BoundRef::Const { index, .. }) => {
                Err(VmError::BadVarRef(VarRef::Const(*index)))
            }
            Slot::Ref(BoundRef::Static {
                file,
                script,
                index,
            }) => {
                let preserves_scalar_meta = !matches!(v, Value::Obj(_) | Value::Null);
                let slot = self.prog.files[*file].scripts[*script]
                    .statics
                    .get_mut(*index as usize)
                    .ok_or(VmError::BadVarRef(VarRef::Static(*index)))?;
                *slot = Some(v);
                if let Some(meta) = self
                    .prog
                    .walk_meta_mut()
                    .and_then(|meta| meta.files.get_mut(*file))
                    .and_then(|meta| meta.script_meta.get_mut(*script))
                    .and_then(|meta| meta.statics.get_mut(*index as usize))
                {
                    // Assignment invokes do_operator on the existing ScriptType; for
                    // scalars the object identity, scope and ref-count do not change.
                    // A shape-changing crafted program cannot preserve this sidecar.
                    if !matches!(
                        meta,
                        Some(ValueWalkMeta {
                            nested: ValueWalkNested::Scalar,
                            ..
                        })
                    ) || !preserves_scalar_meta
                    {
                        *meta = None;
                    }
                }
                Ok(())
            }
            Slot::Ref(BoundRef::Local { frame, index }) => {
                let inner = self
                    .frames
                    .get(*frame)
                    .and_then(|f| f.locals.get(*index as usize))
                    .and_then(Option::as_ref)
                    .cloned()
                    .ok_or(VmError::BadVarRef(VarRef::Local(*index)))?;
                match inner {
                    Slot::Val(_) => {
                        self.frames[*frame].locals[*index as usize] = Some(Slot::Val(v));
                        Ok(())
                    }
                    alias => self.store(&alias, v),
                }
            }
            Slot::Cell(c) => {
                *c.borrow_mut() = v;
                Ok(())
            }
            // Assigning to a temporary is a no-op in the engine too: the value is
            // released immediately afterwards.
            Slot::Val(_) => Ok(()),
        }
    }

    /// `ScriptType::get_object` (vtable +24) followed by the null check every
    /// aggregate opcode performs. Returns the shared aggregate.
    fn as_obj(v: &Value) -> Option<std::rc::Rc<std::cell::RefCell<Obj>>> {
        v.obj().cloned()
    }

    // ---------------------------------------------------------------- step

    fn step(&mut self) -> Result<Step, VmError> {
        let here = self.bip();
        let op = self.fetch_u8()?;
        if op >= opcode::OP_ERROR_TOKEN {
            return Err(VmError::BadOpcode(op));
        }
        match op {
            // ---- OP_PUSH: get_value(operand), push. Pushes the *reference*.
            0x26 => {
                let r = VarRef::decode(self.fetch_u32()?);
                // Validate exactly where the engine validates.
                let bound = self.bind_ref(r)?;
                self.deref(&Slot::Ref(bound))?;
                self.stack.push(Slot::Ref(bound));
            }
            // ---- OP_POP
            0x27 => {
                self.pop()?;
            }
            // ---- assignment family (handler 0x009e10e9)
            // pop A (TOP, the target), pop B, A.do_operator(op, B), push A back.
            0x00 | 0x03 | 0x0c | 0x0d | 0x0e | 0x18..=0x1e => {
                let target = self.pop()?;
                let rhs = self.pop_value()?;
                let cur = self.deref(&target)?;
                match crate::ops::do_operator(&cur, op, Some(&rhs)) {
                    Ok(out) => {
                        self.store(&target, out)?;
                        self.stack.push(target);
                    }
                    Err(e) => return Ok(self.rte_op(e, here, op)),
                }
            }
            // ---- binary operators (handler 0x009e11e5)
            // pop rhs (TOP), pop lhs, push lhs.do_operator(op, rhs).
            0x01 | 0x02 | 0x04..=0x08 | 0x0a | 0x0b | 0x0f..=0x11 | 0x17 | 0x1f..=0x24 => {
                let rhs = self.pop_value()?;
                let lhs = self.pop_value()?;
                match crate::ops::do_operator(&lhs, op, Some(&rhs)) {
                    Ok(out) => self.stack.push(Slot::Val(out)),
                    Err(e) => return Ok(self.rte_op(e, here, op)),
                }
            }
            // ---- unary operators (handler 0x009e11bd): do_operator(op, NULL)
            0x09 | 0x16 | 0x25 => {
                let v = self.pop_value()?;
                match crate::ops::do_operator(&v, op, None) {
                    Ok(out) => self.stack.push(Slot::Val(out)),
                    Err(e) => return Ok(self.rte_op(e, here, op)),
                }
            }
            // ---- pre inc/dec (handler 0x009e1127). The handler peeks the *next
            // opcode byte* and swallows a following OP_POP: `i++;` in statement
            // position emits no stack traffic. On the non-POP path it pushes the
            // *object* back, which do_operator has already mutated in place.
            0x12 | 0x13 => {
                let target = self.pop()?;
                let cur = self.deref(&target)?;
                match crate::ops::do_operator(&cur, op, None) {
                    Ok(out) => {
                        self.store(&target, out)?;
                        if self.peek_op() == Some(OP_POP) {
                            self.frames.last_mut().unwrap().bip += 1;
                        } else {
                            self.stack.push(target);
                        }
                    }
                    Err(e) => return Ok(self.rte_op(e, here, op)),
                }
            }
            // ---- post inc/dec (handler 0x009e1159): same OP_POP peek, but on the
            // non-POP path `duplicate()` runs FIRST, so the pre-mutation value is
            // what gets pushed.
            0x14 | 0x15 => {
                let target = self.pop()?;
                let cur = self.deref(&target)?;
                match crate::ops::do_operator(&cur, op, None) {
                    Ok(out) => {
                        self.store(&target, out)?;
                        if self.peek_op() == Some(OP_POP) {
                            self.frames.last_mut().unwrap().bip += 1;
                        } else {
                            self.stack.push(Slot::Val(cur));
                        }
                    }
                    Err(e) => return Ok(self.rte_op(e, here, op)),
                }
            }
            // ---- OP_INIT / OP_INIT_COPY: pop, set_value(operand, popped).
            // 0x32 (`OP_INIT_COPY`, handler 0x009e0f62) calls `duplicate()` on the
            // popped value first (`call [eax+0x1c]`); 0x33 (`OP_INIT`, handler
            // 0x009e0f99) stores the pointer as-is. The shipped compiler uses this
            // exact one-byte distinction for parameters: 0x32 for by-value and
            // 0x33 for `ref`. It therefore matters for scalars as well as aggregates.
            0x32 => {
                let v = self.pop_value()?.duplicate();
                let static_walk_meta = promoted_scalar_walk_meta(&v);
                let r = VarRef::decode(self.fetch_u32()?);
                self.set_slot(r, Slot::Val(v), static_walk_meta)?;
            }
            0x33 => {
                let v = self.pop()?;
                // A direct transfer is ownership-exact only for a fresh temporary.
                // OP_PUSH yields Ref/Cell and would mutate the source object's scope
                // in retail; without pointer-level sidecar identity that path must not
                // leave an apparently complete checksum projection.
                let static_walk_meta = match &v {
                    Slot::Val(value) => promoted_scalar_walk_meta(value),
                    Slot::Ref(_) | Slot::Cell(_) => None,
                };
                let r = VarRef::decode(self.fetch_u32()?);
                self.set_slot(r, v, static_walk_meta)?;
            }
            // ---- OP_CAST_BOOL: push is_false ? 0 : 1
            0x35 => {
                let v = self.pop_value()?;
                self.stack
                    .push(Slot::Val(Value::Int(i32::from(v.is_true()))));
            }
            // ---- OP_CAST: convert to the operand's type tag
            0x34 => {
                let v = self.pop_value()?;
                let tag = self.fetch_u32()?;
                let out = match ScriptTy::from_tag(tag) {
                    Some(ScriptTy::Int) => Value::Int(v.as_int()),
                    Some(ScriptTy::Real) => Value::Real(v.as_real()),
                    Some(ScriptTy::Str) => Value::str(
                        v.as_string()
                            .map_err(|_| VmError::Unimplemented("ScriptObject::get_string"))?,
                    ),
                    _ => v,
                };
                self.stack.push(Slot::Val(out));
            }
            // ---- OP_CREATE_SIMPLE: a fresh zero value of the operand's type
            // (`ScriptType::init_simple`, 0x009d7fb0).
            0x28 => {
                let tag = self.fetch_u32()?;
                self.stack.push(Slot::Val(zero_of(tag)));
            }
            // ---- OP_CREATE_ARRAY (handler 0x009e0aa4):
            //   base = pop();  push ScriptArray::init_array(0, type, base, VM_TEMP)
            // An empty array whose `blank_base` is the popped prototype element.
            0x29 => {
                let base = self.pop_value()?;
                let tag = self.fetch_u32()?;
                self.stack.push(Slot::Val(Value::array(tag, base, 0)));
            }
            // ---- OP_CREATE_ARRAY_DYN (handler 0x009e0ac2):
            //   n = pop()->get_int();  base = pop();
            //   push ScriptArray::init_array(n, type, base, VM_TEMP)
            // The size is on TOP, the prototype beneath it.
            0x2a => {
                let n = self.pop_value()?.as_int();
                let base = self.pop_value()?;
                let tag = self.fetch_u32()?;
                if n < 0 {
                    return Ok(self.rte(format!("negative array size {n}"), here, op));
                }
                self.stack
                    .push(Slot::Val(Value::array(tag, base, n as usize)));
            }
            // ---- OP_CREATE_ARRAY_INITER (handler 0x009e0b47) and
            //      OP_CREATE_STRUCT       (handler 0x009e0b67)
            // Both take two operands [count][type] and call the stack-consuming
            // constructor: `ScriptArray::init_array(count, type, &run_stack, ...)`
            // (0x009d6170) / `ScriptObject::init_struct(count, type, &run_stack)`
            // (0x009d8410). Each pops `count` values off the run stack, calls
            // `duplicate()` on each and appends it, so **values[0] is the value that
            // was on TOP of the stack** — the emitter must therefore push an
            // initialiser list in reverse. `init_array` additionally derives
            // `blank_base` from `values[0]->duplicate()->clear()`.
            0x2b | 0x2c => {
                let count = self.fetch_u32()? as usize;
                let tag = self.fetch_u32()?;
                let mut vals = Vec::with_capacity(count);
                for _ in 0..count {
                    vals.push(self.pop_value()?.duplicate());
                }
                if op == 0x2c {
                    self.stack.push(Slot::Val(Value::strukt(tag, vals)));
                } else {
                    if vals.is_empty() {
                        // `0x009d6228`: an initialiser with no elements is an error.
                        return Ok(self.rte("array initialiser with no elements", here, op));
                    }
                    let base = zero_like(&vals[0]);
                    let o = Obj {
                        data_type: tag,
                        values: vals.into_iter().map(cell).collect(),
                        blank_base: Some(Box::new(base)),
                        is_array: true,
                    };
                    self.stack.push(Slot::Val(Value::Obj(std::rc::Rc::new(
                        std::cell::RefCell::new(o),
                    ))));
                }
            }
            // ---- OP_PUSH_ARRAY_INDEX (handler 0x009e0b87): the *rvalue* subscript.
            //   idx = pop()->get_int();  obj = pop()->get_object();
            //   if (!obj) rte;  if (idx >= obj->values.count) rte;  if (idx < 0) rte;
            //   push &obj->values[idx];
            // Note it does NOT call is_array() and it never grows the array.
            0x2d => {
                let idx = self.pop_value()?.as_int();
                let base = self.pop_value()?;
                let o = match Self::as_obj(&base) {
                    Some(o) => o,
                    None => return Ok(self.rte("variable is not an array", here, op)),
                };
                let len = o.borrow().values.len() as i32;
                if idx >= len || idx < 0 {
                    return Ok(self.rte(
                        format!("array index {idx} out of range ({len})"),
                        here,
                        op,
                    ));
                }
                let c = o.borrow().values[idx as usize].clone();
                self.stack.push(Slot::Cell(c));
            }
            // ---- OP_CREATE_ARRAY_INDEX (handler 0x009e0cc3): the *lvalue* subscript.
            // Same shape, plus an is_array() check, and instead of failing on an
            // index past the end it calls resize_array(idx + 1, /*grow_only=*/1).
            0x2e => {
                let idx = self.pop_value()?.as_int();
                let base = self.pop_value()?;
                let o = match Self::as_obj(&base).filter(|o| o.borrow().is_array) {
                    Some(o) => o,
                    None => return Ok(self.rte("variable is not an array", here, op)),
                };
                if idx < 0 {
                    return Ok(self.rte(format!("array index {idx} out of range"), here, op));
                }
                if idx as usize >= o.borrow().values.len() {
                    if let Err(e) = o.borrow_mut().resize(idx as usize + 1, true) {
                        return Ok(self.rte_op(e, here, op));
                    }
                }
                let c = o.borrow().values[idx as usize].clone();
                self.stack.push(Slot::Cell(c));
            }
            // ---- OP_PUSH_STRUCT_FIELD (handler 0x009e0dc5): one operand, the field
            // index. Structs and arrays share `values`, so this is the subscript
            // instruction with the index in the instruction stream.
            0x2f => {
                let base = self.pop_value()?;
                let idx = self.fetch_u32()? as usize;
                let o = match Self::as_obj(&base) {
                    Some(o) => o,
                    None => return Ok(self.rte("variable is not a struct", here, op)),
                };
                let len = o.borrow().values.len();
                if idx >= len {
                    return Ok(self.rte(
                        format!("struct field {idx} out of range ({len})"),
                        here,
                        op,
                    ));
                }
                let c = o.borrow().values[idx].clone();
                self.stack.push(Slot::Cell(c));
            }
            // ---- OP_PUSH_ARRAY_LENGTH (handler 0x009e0e95): pushes a fresh
            // ScriptInt holding values.count. On a non-aggregate it raises
            // run_time_error and pushes 0 — so the stack stays balanced either way.
            0x30 => {
                let base = self.pop_value()?;
                match Self::as_obj(&base) {
                    Some(o) => {
                        let n = o.borrow().values.len() as i32;
                        self.stack.push(Slot::Val(Value::Int(n)));
                    }
                    None => {
                        self.stack.push(Slot::Val(Value::Int(0)));
                        return Ok(self.rte("variable is not an array", here, op));
                    }
                }
            }
            // ---- OP_SET_ARRAY_LENGTH (handler 0x009e0f0c):
            //   arr = pop()->get_object();  n = pop()->get_int();
            //   if (arr && arr->is_array()) resize_array(n, /*grow_only=*/0);
            //   push n;                       <- the shared 0x009e0a11 epilogue
            // The array is on TOP and the length beneath it — the same operand
            // order as OP_ASSIGN, and the opposite of a binary operator.
            0x31 => {
                let arr = self.pop_value()?;
                let nv = self.pop_value()?;
                let n = nv.as_int();
                let ok = match Self::as_obj(&arr).filter(|o| o.borrow().is_array) {
                    Some(o) => {
                        if n < 0 {
                            false
                        } else {
                            o.borrow_mut().resize(n as usize, false).is_ok()
                        }
                    }
                    None => false,
                };
                self.stack.push(Slot::Val(nv));
                if !ok {
                    return Ok(self.rte("variable is not an array", here, op));
                }
            }
            // ---- calls
            0x36 => {
                // OP_CALL: call_script(-1, script_index) — same file.
                let script = self.fetch_u32()? as usize;
                let file = self.frames.last().unwrap().file;
                self.push_frame(file, script)?;
            }
            0x37 => {
                // OP_CALL_INCLUDE: operands are [script_index][linked-file slot].
                // RunTimeEnv::call_script resolves the latter through the current
                // ScriptFile's linked_files pointer array before opening the frame.
                let script = self.fetch_u32()? as usize;
                let link = self.fetch_u32()? as usize;
                let current_file = self.frames.last().unwrap().file;
                let file = *self
                    .prog
                    .resolved_links(current_file)
                    .ok_or(VmError::BadFileIndex(current_file))?
                    .get(link)
                    .ok_or(VmError::BadFileIndex(link))?;
                self.push_frame(file, script)?;
            }
            0x38 => {
                // OP_CALL_GAME: arity comes from the declaration (`nargs < 0`).
                let idx = self.fetch_u32()?;
                let decl = builtin(idx).ok_or(VmError::BadBuiltinIndex(idx))?;
                self.call_builtin(decl, decl.arity as usize)?;
            }
            0x39 => {
                // OP_CALL_GAME_VARIED: operands are [func_index][argc].
                let idx = self.fetch_u32()?;
                let argc = self.fetch_u32()? as usize;
                let decl = builtin(idx).ok_or(VmError::BadBuiltinIndex(idx))?;
                self.call_builtin(decl, argc)?;
            }
            // ---- OP_RETURN (handler 0x009e1545)
            0x3e => {
                let void = self.frames.last().unwrap().void_return;
                if !void {
                    // Replaces the top of stack with `duplicate()` so the returned
                    // value stops aliasing whatever variable produced it.
                    let v = self.peek_value()?;
                    let n = self.stack.len();
                    self.stack[n - 1] = Slot::Val(v.duplicate());
                }
                return self.close_frame();
            }
            // ---- unconditional jump: operand is an absolute code offset
            0x3f => {
                let t = self.fetch_u32()? as usize;
                self.set_bip(t);
            }
            // ---- OP_JUMP_IF: jump when TRUE
            0x40 => {
                let v = self.pop_value()?;
                let at = self.bip();
                if v.is_true() {
                    let t = self.peek_u32(at)? as usize;
                    self.set_bip(t);
                } else {
                    self.set_bip(at + 4);
                }
            }
            // ---- OP_JUMP_IF_NOT: jump when FALSE
            0x41 => {
                let v = self.pop_value()?;
                let at = self.bip();
                if v.is_false() {
                    let t = self.peek_u32(at)? as usize;
                    self.set_bip(t);
                } else {
                    self.set_bip(at + 4);
                }
            }
            // ---- short-circuit `||`: true keeps the value and jumps, false
            // discards it and falls through to evaluate the next operand.
            0x42 => {
                let s = self.pop()?;
                let v = self.deref(&s)?;
                let at = self.bip();
                if v.is_true() {
                    self.stack.push(s);
                    let t = self.peek_u32(at)? as usize;
                    self.set_bip(t);
                } else {
                    self.set_bip(at + 4);
                }
            }
            // ---- short-circuit `&&`
            0x43 => {
                let s = self.pop()?;
                let v = self.deref(&s)?;
                let at = self.bip();
                if v.is_false() {
                    self.stack.push(s);
                    let t = self.peek_u32(at)? as usize;
                    self.set_bip(t);
                } else {
                    self.set_bip(at + 4);
                }
            }
            // ---- OP_CASE (handler 0x009e1259): peek the subject, compare against
            // const_pool[operand1] with OP_EQ_OP; on MATCH pop the subject and jump
            // to the case body, on mismatch fall through leaving the subject.
            0x3a => {
                let ci = self.fetch_u32()? & 0xdfff_ffff;
                let file = self.frames.last().unwrap().file;
                let k = self.prog.files[file]
                    .const_pool
                    .get(ci as usize)
                    .cloned()
                    .ok_or(VmError::BadVarRef(VarRef::Const(ci)))?;
                let subject = self.peek_value()?;
                let eq = match crate::ops::do_operator(&subject, opcode::OP_EQ_OP, Some(&k)) {
                    Ok(v) => v,
                    Err(e) => return Ok(self.rte_op(e, here, op)),
                };
                let at = self.bip();
                if eq.is_true() {
                    let t = self.peek_u32(at)? as usize;
                    self.set_bip(t);
                    self.pop()?;
                } else {
                    self.set_bip(at + 4);
                }
            }
            // ---- OP_JUMP_IF_INITED (handler 0x009e0fca): the `static` guard.
            // If the static slot is in range AND non-null, jump past the
            // initialiser; otherwise fall through and run it. This is the whole
            // mechanism by which a per-frame script keeps state.
            0x44 => {
                let raw = self.fetch_u32()?;
                let i = (raw & 0xbfff_ffff) as usize;
                let f = self.frames.last().unwrap();
                let s = &self.prog.files[f.file].scripts[f.script];
                let inited = s.statics.get(i).map(|v| v.is_some()).unwrap_or(false);
                let at = self.bip();
                if inited {
                    let t = self.peek_u32(at)? as usize;
                    self.set_bip(t);
                } else {
                    self.set_bip(at + 4);
                }
            }
            // ---- trigger enable bits. The MACHINE is the authority here and it
            // contradicts the enum names: 0x3c is `btr` (clear), 0x3d is `bts` (set).
            0x3c => {
                let i = self.fetch_u32()? as i32;
                let f = self.frames.last().unwrap();
                self.prog.files[f.file].scripts[f.script].trigger_bit_clear(i);
            }
            0x3d => {
                let i = self.fetch_u32()? as i32;
                let f = self.frames.last().unwrap();
                self.prog.files[f.file].scripts[f.script].trigger_bit_set(i);
            }
            // ---- OP_JUMP_IF_BITSET (handler 0x009e109c): fall INTO the trigger
            // body when enabled, jump PAST it when not.
            0x45 => {
                let i = self.fetch_u32()? as i32;
                let f = self.frames.last().unwrap();
                let on = self.prog.files[f.file].scripts[f.script].is_trigger_enabled(i);
                let at = self.bip();
                if on {
                    self.set_bip(at + 4);
                } else {
                    let t = self.peek_u32(at)? as usize;
                    self.set_bip(t);
                }
            }
            // ---- markers. OP_MARKER is never emitted (count_code skips it);
            // OP_SCRIPT_MARKER is emitted with a 4-byte operand and does nothing.
            0x46 => {}
            0x47 => {
                self.fetch_u32()?;
            }
            // ---- OP_BREAK is the debugger breakpoint: the compiler patches the
            // original opcode byte out and `Breakpoints::break_at` hands it back to
            // be re-dispatched. Compiled output we run never contains it.
            0x3b => return Err(VmError::Unimplemented("OP_BREAK (debugger breakpoint)")),
            other => {
                return Err(VmError::Unimplemented(
                    opcode::decode(other).map(|d| d.name).unwrap_or("?"),
                ))
            }
        }
        Ok(Step::Continue)
    }

    /// `VirtualMachine::call_func` (`0x009e0550`) + `ScriptFuncSet::call_func`.
    fn call_builtin(&mut self, decl: &'static BuiltinDecl, argc: usize) -> Result<(), VmError> {
        // Shipped compiler captures push arguments right-to-left, so each pop is
        // already the next logical parameter: param 0, param 1, ... . Native
        // ScriptParamStack handlers consume the same top-first order.
        let mut args = Vec::with_capacity(argc);
        for _ in 0..argc {
            args.push(self.pop_value()?);
        }
        // `TriggerUtilFuncSet` (indices 15..=17) is the one part of the builtin
        // surface that is *VM* state rather than *host* state: all three handlers
        // (`0x00a04440` / `0x00a04460` / `0x00a04480`) open with
        // `mov ecx, [0xebeed0]` and operate on the currently running `Script`'s
        // trigger bits. So the VM answers them itself and the host never sees them.
        if let Some(v) = self.call_trigger_util(decl, &args) {
            self.coverage.record_implemented(decl.index);
            if decl.ret != ScriptTy::Void {
                self.stack.push(Slot::Val(v));
            }
            return Ok(());
        }
        let r = self.host.call(decl, &args);
        let push_ret = decl.ret != ScriptTy::Void;
        match r {
            Ok(v) => {
                self.coverage.record_implemented(decl.index);
                if push_ret {
                    self.stack.push(Slot::Val(v));
                }
            }
            Err(HostError::Unimplemented) => {
                self.coverage.record_unimplemented(decl.index, decl.name);
                match self.missing_builtin_policy {
                    MissingBuiltinPolicy::Fail => {
                        return Err(VmError::UnimplementedBuiltin {
                            index: decl.index,
                            name: decl.name,
                        });
                    }
                    MissingBuiltinPolicy::Survey if push_ret => {
                        self.stack.push(Slot::Val(survey_err_return(decl.ret)));
                    }
                    MissingBuiltinPolicy::Survey => {}
                }
            }
            Err(HostError::BadArgs(reason)) => {
                return Err(VmError::HostRejected {
                    index: decl.index,
                    name: decl.name,
                    reason,
                });
            }
        }
        Ok(())
    }

    /// `TriggerUtilFuncSet`, indices 15..=17, against the running script.
    ///
    /// - 15 `enable_all_triggers` -> `Script::enable_all_triggers(1)` (`0x009c5c90`)
    /// - 16 `disable_all_triggers` -> `Script::enable_all_triggers(0)`
    /// - 17 `is_trigger_enabled(name)` -> `Script::is_trigger_enabled` (`0x009c5ba0`);
    ///   with no script bound, the handler returns `-1` (`or eax, 0xffffffff`).
    ///
    /// Note these say what they mean, unlike `OP_BIT_SET`/`OP_BIT_UNSET`: *here*
    /// "enable" sets the bit.
    fn call_trigger_util(&mut self, decl: &BuiltinDecl, args: &[Value]) -> Option<Value> {
        let f = self.frames.last()?;
        let (file, script) = (f.file, f.script);
        let s = &mut self.prog.files[file].scripts[script];
        match decl.index {
            15 => {
                s.enable_all_triggers(true);
                Some(Value::Null)
            }
            16 => {
                s.enable_all_triggers(false);
                Some(Value::Null)
            }
            17 => {
                let name = match args.first() {
                    Some(v) => match v.as_string() {
                        Ok(s) => s,
                        Err(_) => return Some(Value::Int(-1)),
                    },
                    None => String::new(),
                };
                Some(Value::Int(match s.find_trigger(&name) {
                    Some(i) => i32::from(s.is_trigger_enabled(i as i32)),
                    None => -1,
                }))
            }
            _ => None,
        }
    }
}

/// Checksum-visible ownership after `set_value` installs a duplicated or transferred
/// temporary scalar. Scalar constructors and `duplicate()` allocate with ref-count
/// zero; the store promotes scope 1 to `VM_VAR` (3). Aggregate ownership is
/// intentionally not inferred from the Rust value graph.
fn promoted_scalar_walk_meta(value: &Value) -> Option<ValueWalkMeta> {
    match value {
        Value::Int(_) | Value::Real(_) | Value::Str(_) => Some(ValueWalkMeta::scalar(3, 0)),
        Value::Obj(_) | Value::Null => None,
    }
}

/// `ScriptType::init_simple(tag, ...)` (`0x009d7fb0`) — a fresh zeroed value.
pub fn zero_of(tag: u32) -> Value {
    match ScriptTy::from_tag(tag) {
        Some(ScriptTy::Real) => Value::Real(0.0),
        Some(ScriptTy::Str) => Value::str(""),
        _ => Value::Int(0),
    }
}

/// `v->duplicate()` followed by `clear()` — how `ScriptArray::init_array`
/// (`0x009d6170`) derives an initialiser-list array's `blank_base` from its first
/// element.
fn zero_like(v: &Value) -> Value {
    match v {
        Value::Real(_) => Value::Real(0.0),
        Value::Str(_) => Value::str(""),
        Value::Obj(o) => {
            let src = o.borrow();
            Value::Obj(std::rc::Rc::new(std::cell::RefCell::new(Obj {
                data_type: src.data_type,
                values: src
                    .values
                    .iter()
                    .map(|c| cell(zero_like(&c.borrow())))
                    .collect(),
                blank_base: src.blank_base.clone(),
                is_array: src.is_array,
            })))
        }
        _ => Value::Int(0),
    }
}
