//! `ScriptType::do_operator(OpCodeTypes op, ScriptType* rhs)` — vtable slot +44.
//!
//! **Every** arithmetic, comparison, logical and assignment opcode in
//! `VirtualMachine::execute_next` funnels through this one virtual call; the VM
//! loop itself contains no arithmetic. There are five concrete implementations and
//! this module reproduces four of them (the fifth, `ScriptObject::do_operator` for
//! game-object handles, needs simulation state):
//!
//! | receiver | VA | size |
//! |---|---|---|
//! | `ScriptInt::do_operator` | `0x009d7760` | 1580 |
//! | `ScriptFloat::do_operator` | `0x009d7040` | 1420 |
//! | `ScriptString::do_operator` | `0x009d6b10` | 880 |
//! | `ScriptArray::do_operator` | `0x009d5d80` | 838 |
//! | `ScriptObject::do_operator` | `0x009d65d0` | 878 |
//!
//! # What reading them actually changed
//!
//! This module used to be written from the opcode *names* and was the crate's
//! weakest part. Every one of the following is a correction that a name-driven
//! implementation gets wrong, and each is `[measured]` from the switch above:
//!
//! 1. **There is no type promotion. At all.** Every implementation opens with
//!    `if (rhs != NULL && rhs->data_type != this->data_type) { run_time_error(...); return ...; }`.
//!    `int + real` is a *runtime error*, not a promotion — the compiler is
//!    responsible for having inserted `OP_CAST` (`SyntaxNode::auto_cast`,
//!    `0x009dfee0`). A VM that promotes silently accepts programs retail rejects
//!    and produces values retail never produces.
//! 2. **`OP_UNA_NOT` on an int is `value < 1`, not `value == 0`.** So `!(-5)` is
//!    `1`. It agrees with `ScriptInt::is_false` (`setle`), which is the same
//!    `value <= 0` rule.
//! 3. **`OP_AND_OP` and `OP_OR_OP` on an int ignore the right operand entirely**
//!    and both return `value > 0`. The actual short-circuit logic lives in
//!    `OP_JUMP_IF_SC_TRUE` / `OP_JUMP_IF_SC_FALSE`; these opcodes only coerce the
//!    surviving operand to a boolean. On a *string* they do read both operands.
//! 4. **`OP_ADD` on two strings concatenates** (`operator+`, `0x00a1c3d0`) and
//!    `OP_ADD_ASSIGN` is `String::operator+=` — but only when *both* sides are
//!    strings, because of (1).
//! 5. **String comparison is case-insensitive.** `String::operator==`
//!    (`0x00a1f140`) ends in `_wcsicmp` (IAT slot `0x00ac5640`), and its
//!    const-string fast path compares the `hash_insensitive` word at `String+16`.
//! 6. **Divide by zero is not a trap.** `run_time_error("Can't Divide 0")` and the
//!    result is a fresh **zero** of the receiver's type; the frame keeps running.
//! 7. **`>>` is `sar`** (`0x009d7b73`, `0x009d7c2a`) — arithmetic, not logical —
//!    and both shifts mask the count to 5 bits.
//! 8. **`OP_POW_OP` really is exponentiation**, via the helper at `0x0048ef60`
//!    which widens to double, calls the CRT `pow`, and narrows back to `float`. On
//!    an int receiver the result is truncated back to `int`.
//! 9. **Float `<` and `>` are written as `!(x < y) && !(x == y)` pairs**
//!    (`0x009d721d`), so an unordered (NaN) comparison yields **1**, not 0.
//!
//! # Fidelity
//!
//! Tier C. Everything above was read out of the binary — the switch structure from
//! the bulk decompilation under `re/decomp-all/`, and every numeric decision that
//! the decompiler could have reordered (shift kind, division kind, the `setle`
//! predicates, the `_wcsicmp` call) re-checked at the instruction level with
//! capstone. It has still never been *executed* against retail. Raising it to
//! Tier B needs `oracle doop`; see `docs/tracks/bhs-vm.md`.

use crate::value::{cell, OpError, OpResult, Value};

fn b(x: bool) -> Value {
    Value::Int(i32::from(x))
}

/// Apply an opcode to one or two values.
///
/// `rhs` is `None` for the unary group (`OP_UNA_NOT`, `OP_UNA_NEGA`,
/// `OP_UNA_TILD`) and for pre/post increment and decrement, exactly as the engine
/// passes a null second argument at `0x009e11c5` (`push 0`).
pub fn do_operator(lhs: &Value, op: u8, rhs: Option<&Value>) -> OpResult {
    // The guard at the head of every implementation, e.g. `0x009d7789`:
    //     if (rhs != NULL && rhs->data_type != this->data_type) -> run_time_error
    if let Some(r) = rhs {
        if r.data_type() != lhs.data_type() {
            return Err(OpError::TypeMismatch {
                lhs: lhs.data_type(),
                rhs: r.data_type(),
            });
        }
    }
    match lhs {
        Value::Int(a) => int_op(*a, op, rhs.map(|r| r.as_int())),
        Value::Real(a) => real_op(*a, op, rhs.map(|r| r.as_real())),
        Value::Str(a) => str_op(a, op, rhs),
        Value::Obj(_) => obj_op(lhs, op, rhs),
        Value::Null => Err(OpError::BadOperand("do_operator on a null ScriptType*")),
    }
}

/// `ScriptInt::do_operator` (`0x009d7760`).
fn int_op(a: i32, op: u8, rhs: Option<i32>) -> OpResult {
    // Opcodes that never touch `rhs`. Ordered as the retail switch is.
    match op {
        0x09 => return Ok(b(a < 1)),                             // OP_UNA_NOT
        0x0a | 0x0b => return Ok(b(a > 0)),                      // OP_AND_OP / OP_OR_OP
        0x12 | 0x14 => return Ok(Value::Int(a.wrapping_add(1))), // OP_INC_OP(_POST)
        0x13 | 0x15 => return Ok(Value::Int(a.wrapping_sub(1))), // OP_DEC_OP(_POST)
        0x16 => return Ok(Value::Int(a.wrapping_neg())),         // OP_UNA_NEGA
        0x25 => return Ok(Value::Int(!a)),                       // OP_UNA_TILD
        _ => {}
    }
    let r = match rhs {
        Some(r) => r,
        None => return Err(OpError::BadOperand("int binary opcode with no rhs")),
    };
    let div = |x: i32, y: i32| -> Result<i32, OpError> {
        if y == 0 {
            Err(OpError::DivideByZero)
        } else {
            Ok(x.wrapping_div(y))
        }
    };
    let rem = |x: i32, y: i32| -> Result<i32, OpError> {
        if y == 0 {
            Err(OpError::DivideByZero)
        } else {
            Ok(x.wrapping_rem(y))
        }
    };
    let sh = (r as u32) & 31;
    Ok(Value::Int(match op {
        0x00 => r,                        // OP_ASSIGN — raw copy of the payload dword
        0x01 => i32::from(a == r),        // OP_EQ_OP
        0x02 => i32::from(a != r),        // OP_NE_OP
        0x03 | 0x04 => a.wrapping_add(r), // OP_ADD_ASSIGN / OP_ADD
        0x05 => i32::from(a < r),         // OP_LESS
        0x06 => i32::from(r < a),         // OP_GREA
        0x07 => i32::from(a <= r),        // OP_LE_OP
        0x08 => i32::from(r <= a),        // OP_GE_OP
        0x0c | 0x0f => a.wrapping_mul(r), // OP_MUL_ASSIGN / OP_MUL
        0x0d | 0x10 => div(a, r)?,        // OP_DIV_ASSIGN / OP_DIV
        0x0e | 0x11 => a.wrapping_sub(r), // OP_SUB_ASSIGN / OP_SUBT
        // OP_POW_OP / OP_POW_ASSIGN. `0x0048ef60` is `cvtss2sd x2; call pow;
        // cvtsd2ss`, and the int receiver converts its own value in and truncates
        // the result back out. Which register carries the base is not visible in
        // the decompilation; `pow(this, rhs)` is the reading taken here.
        0x17 | 0x18 => (a as f32).powf(r as f32) as i32,
        0x19 | 0x1f => rem(a, r)?,         // OP_MOD_ASSIGN / OP_MOD
        0x1a | 0x20 => a.wrapping_shl(sh), // OP_LEFT_ASSIGN / OP_LEFT_OP  (shl)
        0x1b | 0x21 => a.wrapping_shr(sh), // OP_RIGHT_ASSIGN / OP_RIGHT_OP (sar)
        0x1c | 0x22 => a & r,              // OP_AND_ASSIGN / OP_AND_BIT
        0x1d | 0x23 => a ^ r,              // OP_XOR_ASSIGN / OP_XOR_BIT
        0x1e | 0x24 => a | r,              // OP_OR_ASSIGN / OP_OR_BIT
        _ => return Err(OpError::BadOperand("opcode not defined for int")),
    }))
}

/// `ScriptFloat::do_operator` (`0x009d7040`).
///
/// Note which results are floats and which are ints: the comparison and logical
/// group allocates through `ScriptInt::pop` (`0x009d75d0`) and the arithmetic group
/// through `ScriptFloat::pop` (`0x009d6e80`). A float comparison therefore yields an
/// `int`, and the next `do_operator` up the tree sees an int receiver.
fn real_op(a: f32, op: u8, rhs: Option<f32>) -> OpResult {
    match op {
        0x09 => return Ok(b(a <= 0.0)),                 // OP_UNA_NOT
        0x0a | 0x0b => return Ok(b(a > 0.0)),           // OP_AND_OP / OP_OR_OP
        0x12 | 0x14 => return Ok(Value::Real(a + 1.0)), // OP_INC_OP(_POST)
        0x13 | 0x15 => return Ok(Value::Real(a - 1.0)), // OP_DEC_OP(_POST)
        // `xor` of the sign bit, not a negation instruction: -0.0 flips to +0.0.
        0x16 => return Ok(Value::Real(f32::from_bits(a.to_bits() ^ 0x8000_0000))),
        _ => {}
    }
    let r = match rhs {
        Some(r) => r,
        None => return Err(OpError::BadOperand("real binary opcode with no rhs")),
    };
    Ok(match op {
        0x00 => Value::Real(r),            // OP_ASSIGN
        0x01 => b(a == r),                 // OP_EQ_OP
        0x02 => b(a != r),                 // OP_NE_OP
        0x03 | 0x04 => Value::Real(a + r), // OP_ADD_ASSIGN / OP_ADD
        // `!(r < a) && !(r == a)` — unordered compares as TRUE.
        0x05 => b(!(r < a) && !(r == a)),  // OP_LESS
        0x06 => b(!(a < r) && !(a == r)),  // OP_GREA
        0x07 => b(a <= r),                 // OP_LE_OP
        0x08 => b(r <= a),                 // OP_GE_OP
        0x0c | 0x0f => Value::Real(a * r), // OP_MUL_ASSIGN / OP_MUL
        0x0d | 0x10 => {
            if r == 0.0 {
                return Err(OpError::DivideByZero);
            }
            Value::Real(a / r)
        }
        0x0e | 0x11 => Value::Real(a - r), // OP_SUB_ASSIGN / OP_SUBT
        0x17 | 0x18 => Value::Real(a.powf(r).trunc()), // OP_POW_OP / OP_POW_ASSIGN
        // No modulo, no shifts, no bitwise ops on a float: the switch has no cases
        // for them and falls into `run_time_error` + a fresh 0.0.
        _ => return Err(OpError::BadOperand("opcode not defined for real")),
    })
}

/// `ScriptString::do_operator` (`0x009d6b10`).
///
/// Every comparison goes through `String`'s operators, which are **case-insensitive**
/// (`_wcsicmp`). `curr_len` is the emptiness test the logical group uses.
fn str_op(a: &str, op: u8, rhs: Option<&Value>) -> OpResult {
    if op == 0x09 {
        // OP_UNA_NOT: `curr_len == 0`.
        return Ok(b(a.is_empty()));
    }
    let rv = match rhs {
        Some(r) => r,
        None => return Err(OpError::BadOperand("string binary opcode with no rhs")),
    };
    let r = match rv {
        Value::Str(s) => &**s,
        _ => return Err(OpError::BadOperand("string operator with non-string rhs")),
    };
    let al = a.to_lowercase();
    let rl = r.to_lowercase();
    Ok(match op {
        0x00 => Value::str(r),                 // OP_ASSIGN  (String::operator=)
        0x01 => b(al == rl),                   // OP_EQ_OP   (_wcsicmp == 0)
        0x02 => b(al != rl),                   // OP_NE_OP
        0x03 => Value::str(format!("{a}{r}")), // OP_ADD_ASSIGN (operator+=)
        0x04 => Value::str(format!("{a}{r}")), // OP_ADD        (operator+)
        0x05 => b(al < rl),                    // OP_LESS
        0x06 => b(al > rl),                    // OP_GREA
        0x07 => b(al < rl || al == rl),        // OP_LE_OP
        0x08 => b(al > rl || al == rl),        // OP_GE_OP
        // These two DO read both operands, unlike the int forms.
        0x0a => b(!a.is_empty() && !r.is_empty()), // OP_AND_OP
        0x0b => b(!a.is_empty() || !r.is_empty()), // OP_OR_OP
        _ => return Err(OpError::BadOperand("opcode not defined for string")),
    })
}

/// `ScriptArray::do_operator` (`0x009d5d80`) and `ScriptObject::do_operator`
/// (`0x009d65d0`). Both switches accept exactly three opcodes — assign, equal,
/// not-equal — and `run_time_error` on everything else.
fn obj_op(lhs: &Value, op: u8, rhs: Option<&Value>) -> OpResult {
    let this = lhs.obj().expect("obj_op on a non-object").clone();
    let other = match rhs.and_then(|r| r.obj()) {
        Some(o) => o.clone(),
        None => {
            return match op {
                0x01 => Ok(b(false)),
                0x02 => Ok(b(true)),
                _ => Err(OpError::BadOperand(
                    "aggregate operator with no aggregate rhs",
                )),
            }
        }
    };
    match op {
        0x00 => {
            // Release every element, then append `duplicate()` of each of the rhs's
            // elements stamped VM_VAR. `blank_base` is left alone.
            let src: Vec<Value> = other
                .borrow()
                .values
                .iter()
                .map(|c| c.borrow().duplicate())
                .collect();
            let mut t = this.borrow_mut();
            t.values.clear();
            for v in src {
                t.values.push(cell(v));
            }
            drop(t);
            Ok(lhs.clone())
        }
        0x01 | 0x02 => {
            let eq = {
                let t = this.borrow();
                let o = other.borrow();
                t.values.len() == o.values.len()
                    && t.values.iter().zip(o.values.iter()).all(|(x, y)| {
                        matches!(
                            do_operator(&x.borrow(), 0x01, Some(&y.borrow())),
                            Ok(Value::Int(1))
                        )
                    })
            };
            Ok(b(if op == 0x01 { eq } else { !eq }))
        }
        _ => Err(OpError::BadOperand("opcode not defined for an aggregate")),
    }
}

/// The value the engine substitutes after a `run_time_error` inside `do_operator`:
/// a fresh zero/empty of the **receiver's** type, scope `VM_TEMP`.
///
/// `ScriptInt` and `ScriptString` return `this` unchanged on a *type mismatch*
/// specifically (`0x009d77e2` / `0x009d6b4d` return `local_18 = this`), while
/// `ScriptFloat` returns a null pointer there (`0x009d7080` returns 0). Callers that
/// want to reproduce the divergence must special-case that; this helper only covers
/// the ordinary "bad opcode / divide by zero" exit.
pub fn error_substitute(recv: &Value) -> Value {
    match recv {
        Value::Real(_) => Value::Real(0.0),
        Value::Str(_) => Value::str(""),
        _ => Value::Int(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value::*;

    #[test]
    fn integer_arithmetic() {
        assert_eq!(do_operator(&Int(7), 0x04, Some(&Int(5))), Ok(Int(12)));
        assert_eq!(do_operator(&Int(7), 0x11, Some(&Int(5))), Ok(Int(2)));
        assert_eq!(do_operator(&Int(7), 0x10, Some(&Int(2))), Ok(Int(3)));
        assert_eq!(do_operator(&Int(7), 0x1f, Some(&Int(3))), Ok(Int(1)));
        assert_eq!(
            do_operator(&Int(7), 0x10, Some(&Int(0))),
            Err(OpError::DivideByZero)
        );
        // sar, not shr: sign is preserved.
        assert_eq!(do_operator(&Int(-8), 0x21, Some(&Int(1))), Ok(Int(-4)));
        // C truncation toward zero on both division and remainder.
        assert_eq!(do_operator(&Int(-7), 0x10, Some(&Int(2))), Ok(Int(-3)));
        assert_eq!(do_operator(&Int(-7), 0x1f, Some(&Int(2))), Ok(Int(-1)));
    }

    #[test]
    fn there_is_no_type_promotion() {
        // `int + real` is a run_time_error in retail, not a promotion. If someone
        // "fixes" this to promote, this test is the tripwire.
        assert_eq!(
            do_operator(&Int(1), 0x04, Some(&Real(2.0))),
            Err(OpError::TypeMismatch {
                lhs: 0x0005_7bad,
                rhs: 0x0012_f35f
            })
        );
        assert!(do_operator(&Value::str("a"), 0x04, Some(&Int(1))).is_err());
    }

    #[test]
    fn not_is_less_than_one_not_equal_to_zero() {
        // ScriptInt::do_operator case 9 is `value < 1`, matching is_false's `setle`.
        assert_eq!(do_operator(&Int(0), 0x09, None), Ok(Int(1)));
        assert_eq!(do_operator(&Int(5), 0x09, None), Ok(Int(0)));
        assert_eq!(do_operator(&Int(-5), 0x09, None), Ok(Int(1)));
        assert!(Int(-5).is_false());
    }

    #[test]
    fn and_or_on_an_int_ignore_the_right_operand() {
        // Both cases 10 and 11 return `this->value > 0` and never read rhs.
        assert_eq!(do_operator(&Int(3), 0x0a, Some(&Int(0))), Ok(Int(1)));
        assert_eq!(do_operator(&Int(0), 0x0b, Some(&Int(9))), Ok(Int(0)));
        assert_eq!(do_operator(&Int(-1), 0x0a, Some(&Int(1))), Ok(Int(0)));
    }

    #[test]
    fn strings_concatenate_and_compare_case_insensitively() {
        let a = Value::str("Hello");
        assert_eq!(
            do_operator(&a, 0x04, Some(&Value::str(" World"))),
            Ok(Value::str("Hello World"))
        );
        assert_eq!(
            do_operator(&a, 0x01, Some(&Value::str("HELLO"))),
            Ok(Int(1))
        );
        assert_eq!(
            do_operator(&a, 0x02, Some(&Value::str("HELLO"))),
            Ok(Int(0))
        );
        // Logical ops on strings read both sides, unlike the int forms.
        assert_eq!(do_operator(&a, 0x0a, Some(&Value::str(""))), Ok(Int(0)));
        assert_eq!(do_operator(&a, 0x0b, Some(&Value::str(""))), Ok(Int(1)));
    }

    #[test]
    fn compound_assignment_uses_the_same_arithmetic() {
        assert_eq!(do_operator(&Int(7), 0x03, Some(&Int(5))), Ok(Int(12)));
        assert_eq!(do_operator(&Int(7), 0x0e, Some(&Int(5))), Ok(Int(2)));
        assert_eq!(
            do_operator(&Value::str("ab"), 0x03, Some(&Value::str("cd"))),
            Ok(Value::str("abcd"))
        );
    }

    #[test]
    fn float_negation_flips_the_sign_bit() {
        // `uVar2 ^ 0x80000000`, so -0.0 becomes +0.0 and vice versa.
        match do_operator(&Real(-0.0), 0x16, None) {
            Ok(Real(f)) => assert!(f.is_sign_positive() && f == 0.0),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn floats_have_no_modulo_or_bitwise_ops() {
        assert!(do_operator(&Real(7.0), 0x1f, Some(&Real(2.0))).is_err());
        assert!(do_operator(&Real(7.0), 0x22, Some(&Real(2.0))).is_err());
    }
}
