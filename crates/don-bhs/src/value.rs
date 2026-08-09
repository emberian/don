//! The BHS value model.
//!
//! The engine's runtime value is `ScriptType`, an abstract base whose layout the PDB
//! gives in full: [measured]
//!
//! ```text
//! ScriptType   +0  vptr
//!              +4  int            data_type      // the type tag, e.g. 0x57bad = int
//!              +8  ScriptScope    scope          // VarScope, see below
//!              +12 unsigned short ref_count      //   sizeof(ScriptType) == 16
//! ```
//!
//! and whose vtable is
//! `close, clear, push, get_int, get_float, get_string, get_object, duplicate,`
//! `is_false, is_array, get_array_type, do_operator, walk_data, log_data`
//! at slots +0..+52. **Every arithmetic, comparison and assignment opcode funnels
//! through the single virtual `do_operator(OpCodeTypes op, ScriptType* rhs)` at
//! vtable slot +44 (0x2c)** — there is no per-opcode arithmetic in the VM loop.
//!
//! `VarScope` is recovered whole from the PDB (`LF_ENUM`, field list `0x3CD3`):
//! `VM_TEMP = 1, VM_CONST = 2, VM_VAR = 3, VM_CLEARED = 4, VM_ARRAY = 128`.
//! [measured] It is a lifetime tag, not a value: `execute_next` releases an operand
//! (`vtable+8`, `push`, which returns the object to a `Recycler` free pool) exactly
//! when `scope == VM_TEMP`; every value stored into an aggregate is stamped
//! `VM_VAR`. This is not observable in any *value*, only in allocation, so we model
//! ownership with Rust's own lifetimes and `Rc` and carry no `scope` field.
//!
//! # The concrete leaves, with their storage offsets [measured, rise.pdb]
//!
//! | class | sizeof | payload |
//! |---|---|---|
//! | `ScriptInt` | 20 | `int value` at +16 |
//! | `ScriptFloat` | 20 | `float value` at +16 |
//! | `ScriptString` | 36 | `String value` at +16 (`curr_len` therefore at +24) |
//! | `ScriptObject` | 44 | `PtrArray<ScriptType*> values` at +16 (count +20, data +32) |
//! | `ScriptArray` | 48 | *derives from `ScriptObject`*, adds `ScriptType* blank_base` at +44 |
//!
//! **The single most load-bearing structural fact recovered by this lane: a BHS
//! struct and a BHS array are the same runtime object.** `ScriptArray` *inherits*
//! `ScriptObject`; both keep their contents in the one `values` pointer array, and
//! the only difference is the `blank_base` prototype element an array uses to grow
//! itself and the `is_array()` vtable answer. That is why `OP_PUSH_STRUCT_FIELD`
//! (0x2f) and `OP_PUSH_ARRAY_INDEX` (0x2d) are the same instruction with the index
//! coming from an operand instead of the stack: both do
//! `push &obj->values[i]`.
//!
//! # Fidelity
//!
//! Type tags, layouts and the `VarScope` enum are `[measured]` from the PDB. The
//! predicates and conversions below are `[measured]` from their retail
//! implementations at the instruction level — each cites its address. Nothing here
//! has been *executed* against retail, so this is Tier C: behaviourally faithful to
//! our reading, divergence unmeasured.

use std::cell::RefCell;
use std::rc::Rc;

/// A BHS static type, identified in the binary by a 32-bit `SymType` tag.
///
/// The engine has **exactly ten** distinct tags across all 873 builtin
/// registrations, six of which are the base scalars. [measured] The 53
/// `PARAMTYPE` entries in `ron-data/paramtypes.xml` are *authoring aliases* over
/// these — `who`, `unit_o`, `x`, `y`, `dist_radius` and friends are all
/// `ScriptTy::Int` to the engine, and carry no runtime distinction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScriptTy {
    /// `0x0005_7bad`
    Int,
    /// `0x0012_f35f`
    Real,
    /// `0x0016_8174`
    Str,
    /// `0x0008_4048` — a builtin returning this pushes nothing.
    Void,
    /// `0x0027_b2c1`
    Any,
    /// `0x0064_ea2a` — the varargs sentinel; `ScriptFuncSet::call_func` stops
    /// parameter validation when it sees this tag.
    Params,
    /// `0x0139_fd8d` — a named unit group. Distinct in the engine; the catalogue
    /// flattens it to `who`, which is wrong.
    Group,
    /// `0x0015_3c88`
    Array,
    /// `0x0020_d693`
    StringArray,
    /// `0x1d06_55f3` — a Conquer-the-World diplomatic offer.
    Offer,
}

impl ScriptTy {
    /// The engine's 32-bit `SymType` tag for this type, as pushed at every
    /// `add_new_func` / `add_param` site. [measured]
    pub const fn tag(self) -> u32 {
        match self {
            ScriptTy::Int => 0x0005_7bad,
            ScriptTy::Real => 0x0012_f35f,
            ScriptTy::Str => 0x0016_8174,
            ScriptTy::Void => 0x0008_4048,
            ScriptTy::Any => 0x0027_b2c1,
            ScriptTy::Params => 0x0064_ea2a,
            ScriptTy::Group => 0x0139_fd8d,
            ScriptTy::Array => 0x0015_3c88,
            ScriptTy::StringArray => 0x0020_d693,
            ScriptTy::Offer => 0x1d06_55f3,
        }
    }

    pub fn from_tag(tag: u32) -> Option<ScriptTy> {
        Some(match tag {
            0x0005_7bad => ScriptTy::Int,
            0x0012_f35f => ScriptTy::Real,
            0x0016_8174 => ScriptTy::Str,
            0x0008_4048 => ScriptTy::Void,
            0x0027_b2c1 => ScriptTy::Any,
            0x0064_ea2a => ScriptTy::Params,
            0x0139_fd8d => ScriptTy::Group,
            0x0015_3c88 => ScriptTy::Array,
            0x0020_d693 => ScriptTy::StringArray,
            0x1d06_55f3 => ScriptTy::Offer,
            _ => return None,
        })
    }
}

/// `VarScope`, recovered whole from the PDB enum. Kept for documentation and for
/// anyone reading the engine alongside this crate; the VM does not carry it.
pub mod var_scope {
    pub const VM_TEMP: u32 = 1;
    pub const VM_CONST: u32 = 2;
    pub const VM_VAR: u32 = 3;
    pub const VM_CLEARED: u32 = 4;
    pub const VM_ARRAY: u32 = 128;
}

/// One storage slot. The engine's aggregates hold `ScriptType*`, and
/// `OP_PUSH_ARRAY_INDEX` pushes the address of the slot, so a write through an
/// index aliases the aggregate. `Rc<RefCell<Value>>` reproduces exactly that.
pub type Cell = Rc<RefCell<Value>>;

pub fn cell(v: Value) -> Cell {
    Rc::new(RefCell::new(v))
}

/// A `ScriptObject` (a BHS struct) or a `ScriptArray` (a BHS array).
///
/// One Rust type for both, because the engine has one C++ object for both:
/// `ScriptArray : public ScriptObject`, sharing `values`.
#[derive(Debug, Clone, PartialEq)]
pub struct Obj {
    /// `ScriptType::data_type` (+4). For an array this is the *element* type tag
    /// the create opcode carried; for a struct it is the `StructType`'s own tag,
    /// which is a `String::generate_hash` of the struct name and therefore not one
    /// of the ten builtin tags.
    pub data_type: u32,
    /// `ScriptObject::values` (+16). Struct fields are indexed by declaration
    /// order; array elements by subscript. Same array either way.
    pub values: Vec<Cell>,
    /// `ScriptArray::blank_base` (+44) — the prototype element that
    /// `ScriptArray::resize_array` (`0x009d5cd0`) duplicates when the array grows.
    /// `None` marks a plain struct; `resize_array` raises `run_time_error` and does
    /// nothing when it is null, which is exactly what growing a struct does.
    pub blank_base: Option<Box<Value>>,
    /// `ScriptArray::is_array()` (`0x004cf5c0`) returns 1; `ScriptObject` inherits
    /// `ScriptType::is_array` which returns 0. `OP_CREATE_ARRAY_INDEX` and
    /// `OP_SET_ARRAY_LENGTH` both gate on it.
    pub is_array: bool,
}

pub type ObjRef = Rc<RefCell<Obj>>;

impl Obj {
    /// A fresh empty array with the given element type and prototype element.
    /// `ScriptArray::init_array(count, type, base, scope)` (`0x009d8760`).
    pub fn new_array(data_type: u32, base: Value, count: usize) -> Obj {
        let mut o = Obj {
            data_type,
            values: Vec::with_capacity(count),
            blank_base: Some(Box::new(base.clone())),
            is_array: true,
        };
        for _ in 0..count {
            o.values.push(cell(base.clone()));
        }
        o
    }

    pub fn new_struct(data_type: u32, fields: Vec<Value>) -> Obj {
        Obj {
            data_type,
            values: fields.into_iter().map(cell).collect(),
            blank_base: None,
            is_array: false,
        }
    }

    /// `ScriptArray::resize_array(int count, int grow_only)` (`0x009d5cd0`).
    ///
    /// Growing appends `blank_base->duplicate()` (stamped `VM_VAR`) one element at
    /// a time. Shrinking happens **only when the second argument is zero**:
    /// `OP_SET_ARRAY_LENGTH` passes 0 and can shrink, `OP_CREATE_ARRAY_INDEX`
    /// passes 1 and only ever grows. A null `blank_base` is a `run_time_error` and
    /// a no-op — which is how the engine refuses to resize a struct.
    pub fn resize(&mut self, count: usize, grow_only: bool) -> Result<(), OpError> {
        let base = match &self.blank_base {
            Some(b) => (**b).clone(),
            None => return Err(OpError::NotAnArray),
        };
        if self.values.len() < count {
            while self.values.len() < count {
                self.values.push(cell(base.clone()));
            }
        } else if !grow_only {
            self.values.truncate(count);
        }
        Ok(())
    }
}

/// A runtime BHS value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i32),
    Real(f32),
    Str(Rc<String>),
    /// A `ScriptObject` / `ScriptArray`. Shared by reference, because the engine
    /// passes `ScriptType*` everywhere and `duplicate()` is the only deep copy.
    Obj(ObjRef),
    /// A null slot. `VirtualMachine::get_value` can legitimately yield null for an
    /// uninitialised static, and `OP_JUMP_IF_INITED` tests exactly that.
    Null,
}

impl Value {
    pub fn str(s: impl Into<String>) -> Value {
        Value::Str(Rc::new(s.into()))
    }

    pub fn array(elem_type: u32, base: Value, count: usize) -> Value {
        Value::Obj(Rc::new(RefCell::new(Obj::new_array(
            elem_type, base, count,
        ))))
    }

    pub fn strukt(data_type: u32, fields: Vec<Value>) -> Value {
        Value::Obj(Rc::new(RefCell::new(Obj::new_struct(data_type, fields))))
    }

    pub fn obj(&self) -> Option<&ObjRef> {
        match self {
            Value::Obj(o) => Some(o),
            _ => None,
        }
    }

    /// `ScriptType::data_type` (+4). This is the value the type-mismatch guard at
    /// the head of every `do_operator` compares.
    pub fn data_type(&self) -> u32 {
        match self {
            Value::Int(_) => ScriptTy::Int.tag(),
            Value::Real(_) => ScriptTy::Real.tag(),
            Value::Str(_) => ScriptTy::Str.tag(),
            Value::Obj(o) => o.borrow().data_type,
            Value::Null => 0,
        }
    }

    pub fn ty(&self) -> ScriptTy {
        match self {
            Value::Int(_) => ScriptTy::Int,
            Value::Real(_) => ScriptTy::Real,
            Value::Str(_) => ScriptTy::Str,
            Value::Obj(_) => ScriptTy::Array,
            Value::Null => ScriptTy::Any,
        }
    }

    /// `ScriptType::get_int` (vtable +12).
    ///
    /// `ScriptFloat::get_int` (`0x004cf280`) is a bare `cvttss2si` — truncation
    /// toward zero. `ScriptString::get_int` (`0x004cf470`) runs
    /// `String::convert_int` (`0x00a15fc0`). `ScriptObject::get_int` (`0x0041bff0`)
    /// is `xor eax,eax; ret` — an aggregate is integer **zero**. [measured]
    pub fn as_int(&self) -> i32 {
        match self {
            Value::Int(i) => *i,
            Value::Real(f) => *f as i32,
            Value::Str(s) => s.trim().parse::<i32>().unwrap_or(0),
            Value::Obj(_) => 0,
            Value::Null => 0,
        }
    }

    /// `ScriptType::get_float` (vtable +16). `ScriptObject::get_float`
    /// (`0x004cf550`) is `fldz; ret` — an aggregate is float **zero**. [measured]
    pub fn as_real(&self) -> f32 {
        match self {
            Value::Int(i) => *i as f32,
            Value::Real(f) => *f,
            Value::Str(s) => s.trim().parse::<f32>().unwrap_or(0.0),
            Value::Obj(_) => 0.0,
            Value::Null => 0.0,
        }
    }

    /// `ScriptType::get_string` (vtable +20). This is the conversion `print` and
    /// `print_line` use on their `anytype` argument.
    ///
    /// `ScriptObject::get_string` (`0x009d6370`, 353 bytes) formats an aggregate by
    /// walking its members; we have not decoded that format, so we render the
    /// members comma-separated and say so rather than pretending.
    pub fn as_string(&self) -> String {
        match self {
            Value::Int(i) => i.to_string(),
            Value::Real(f) => f.to_string(),
            Value::Str(s) => (**s).clone(),
            Value::Obj(o) => {
                let o = o.borrow();
                let inner: Vec<String> = o.values.iter().map(|c| c.borrow().as_string()).collect();
                inner.join(",")
            }
            Value::Null => String::new(),
        }
    }

    /// `ScriptType::is_false` (vtable +32). Every conditional opcode
    /// (`OP_JUMP_IF`, `OP_JUMP_IF_NOT`, both short-circuit jumps, `OP_CASE`,
    /// `OP_CAST_BOOL`) calls exactly this.
    ///
    /// **This is not C truthiness and getting it wrong silently inverts branches.**
    /// [measured, at the instruction level]
    ///
    /// - `ScriptInt::is_false` `0x004cf230`: `cmp [ecx+0x10], 0 / setle` —
    ///   **`value <= 0`**. A negative int is FALSE, so `if (-1)` does not run and
    ///   `if (get_err_return())` never runs either.
    /// - `ScriptFloat::is_false` `0x004cf360`: `comiss 0.0, value / setae` —
    ///   `value <= 0.0`, with NaN reported *not* false.
    /// - `ScriptString::is_false` `0x004cf510`: `cmp word [ecx+0x18], 0 / sete` —
    ///   the empty string is false.
    /// - `ScriptObject::is_false` `0x004bcab0`: `xor al,al; ret` — an object or
    ///   array is **always true**, empty or not. `ScriptArray` does not override it.
    pub fn is_false(&self) -> bool {
        match self {
            Value::Int(i) => *i <= 0,
            Value::Real(f) => *f <= 0.0,
            Value::Str(s) => s.is_empty(),
            Value::Obj(_) => false,
            // A null `ScriptType*` would fault in retail; treating it as false is
            // our own containment, not an engine behaviour.
            Value::Null => true,
        }
    }

    pub fn is_true(&self) -> bool {
        !self.is_false()
    }

    /// `ScriptType::duplicate` (vtable +28) — the engine's only deep copy. Aggregates
    /// copy their elements; scalars copy their payload.
    pub fn duplicate(&self) -> Value {
        match self {
            Value::Obj(o) => {
                let src = o.borrow();
                Value::Obj(Rc::new(RefCell::new(Obj {
                    data_type: src.data_type,
                    values: src
                        .values
                        .iter()
                        .map(|c| cell(c.borrow().duplicate()))
                        .collect(),
                    blank_base: src.blank_base.clone(),
                    is_array: src.is_array,
                })))
            }
            v => v.clone(),
        }
    }
}

/// Result of `ScriptType::do_operator`.
pub type OpResult = Result<Value, OpError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpError {
    /// Integer or float division / modulo by zero. The engine raises
    /// `run_time_error("Can't Divide 0")` and substitutes a fresh **zero** of the
    /// receiver's own type rather than trapping.
    DivideByZero,
    /// The guard at the head of every `do_operator`:
    /// `if (rhs && rhs->data_type != this->data_type) run_time_error(...)`.
    /// **There is no arithmetic type promotion in BHS at all** — mixed operands are
    /// a runtime error, and the compiler is expected to have inserted `OP_CAST`.
    TypeMismatch { lhs: u32, rhs: u32 },
    /// The opcode fell through this type's switch. The engine raises
    /// `run_time_error` naming the opcode and substitutes a zero/empty value.
    BadOperand(&'static str),
    /// `get_object()` returned null, or `is_array()` said no. The engine's
    /// "variable is not an array" / "not a struct" `run_time_error`.
    NotAnArray,
    /// `OP_PUSH_ARRAY_INDEX` / `OP_PUSH_STRUCT_FIELD` index outside `values.count`,
    /// or a negative subscript.
    IndexOutOfRange(i32),
}
