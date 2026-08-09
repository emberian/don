//! The four **utility** `FuncSet`s — the 31 builtins that need no simulation.
//!
//! `ScriptGameInterface::init` (`0x009e1a20`) builds five function sets in a fixed
//! order, and registration order *is* the `OP_CALL_GAME` index space: [measured]
//!
//! | FuncSet | indices | what it is |
//! |---|---|---|
//! | `MathUtilFuncSet` | 0–14 | trig, abs, min/max, and the RNG |
//! | `TriggerUtilFuncSet` | 15–17 | the running script's own trigger bits |
//! | `StringUtilFuncSet` | 18–23 | length / char / format / print |
//! | `ArrayUtilFuncSet` | 24–30 | `ScriptArray` mutators |
//! | `ScenarioFuncSet` | 31–872 | **everything that touches the game** |
//!
//! Only the first four are implementable inside this crate; the 842 scenario
//! functions are the simulation boundary and belong to whatever host owns
//! `don-sim`. That split is not a convenience — it is where the engine itself put
//! the seam.
//!
//! The three `TriggerUtilFuncSet` entries are handled by the VM rather than here,
//! because their retail handlers read the *currently running script* through the
//! global at `[0x00ebeed0]` (`0x00a04440` / `0x00a04460` / `0x00a04480` all begin
//! `mov ecx, [0xebeed0]`), which is VM state and not host state.
//!
//! # The RNG is the main simulation stream, and that is load-bearing
//!
//! `rand_int` (`0x009e1890`) is `Random::get(min, max)` (`0x00a39d70`) on the object
//! at `[0x00c06184]` — `GameAccess::game_random`, **the main simulation stream** with
//! 307 call sites across map generation, units, animals, AI leaders and the
//! pathfinder. A script rolling dice perturbs exactly the sequence the rest of the
//! project reproduces, so these functions route through [`Host`] rather than owning
//! any state here. `rand_real` (`0x009e18b0`) inlines the same LCG
//! (`s = s*0x19660d + 0x3c6ef35f`, i.e. `s*1664525 + 1013904223`) against the same
//! object, and `rand_get_seed` (`0x009e1930`) is a bare `mov eax, [[0xc06184]]`.
//! [measured]
//!
//! # Fidelity
//!
//! Every implemented entry cites the handler it was read from. Three entries are
//! deliberately **not** implemented (`parse`, `remove`, `insert`); they report
//! `HostError::Unimplemented` and appear in the coverage debt list rather than being
//! approximated. Tier C throughout.

use crate::builtin_table::BuiltinDecl;
use crate::host::{Host, HostError, HostResult};
use crate::value::{cell, Value};

/// `float pi` at `0x00b695cc` and `float 180.0` at `0x00b696c0`, the two constants
/// every trig wrapper multiplies and divides by. **BHS trigonometry is in degrees**:
/// `sin` (`0x00a04260`) computes `sinf(x * pi / 180)` and `asin` (`0x00a042f0`)
/// computes `asinf(x) * 180 / pi`. [measured]
pub const BHS_PI: f32 = 3.141_592_7;
pub const BHS_HALF_TURN_DEGREES: f32 = 180.0;

fn deg_to_rad(x: f32) -> f32 {
    x * BHS_PI / BHS_HALF_TURN_DEGREES
}
fn rad_to_deg(x: f32) -> f32 {
    x * BHS_HALF_TURN_DEGREES / BHS_PI
}

/// `minss xmm0, m32` semantics: the destination survives only when it is strictly
/// less; ties and unordered comparisons yield the *source*. Rust's `f32::min`
/// returns the non-NaN operand instead, which differs.
fn minss(a: f32, b: f32) -> f32 {
    if a < b {
        a
    } else {
        b
    }
}
fn maxss(a: f32, b: f32) -> f32 {
    if a > b {
        a
    } else {
        b
    }
}

/// Highest index covered by this module (the last `ArrayUtilFuncSet` entry).
pub const UTIL_MAX_INDEX: u32 = 30;

/// Dispatch one utility builtin.
///
/// Returns `None` when `decl` is not one of the 31 utility registrations, so a host
/// can delegate:
///
/// ```ignore
/// fn call(&mut self, decl: &BuiltinDecl, args: &[Value]) -> HostResult {
///     if let Some(r) = don_bhs::builtins::call_util(self, decl, args) { return r; }
///     /* ... the 842 ScenarioFuncSet entries ... */
/// }
/// ```
pub fn call_util<H: Host + ?Sized>(
    host: &mut H,
    decl: &BuiltinDecl,
    args: &[Value],
) -> Option<HostResult> {
    if decl.index > UTIL_MAX_INDEX {
        return None;
    }
    Some(dispatch(host, decl, args))
}

fn arg(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or(Value::Null)
}

fn dispatch<H: Host + ?Sized>(host: &mut H, decl: &BuiltinDecl, args: &[Value]) -> HostResult {
    let f0 = || arg(args, 0).as_real();
    let i0 = || arg(args, 0).as_int();
    match decl.index {
        // ---- MathUtilFuncSet -------------------------------------------------
        0 => Ok(Value::Real(deg_to_rad(f0()).sin())), // sin        0x00a04260
        1 => Ok(Value::Real(deg_to_rad(f0()).cos())), // cos        0x00a04290
        2 => Ok(Value::Real(deg_to_rad(f0()).tan())), // tan        0x00a042c0
        3 => Ok(Value::Real(rad_to_deg(f0().asin()))), // asin       0x00a042f0
        4 => Ok(Value::Real(rad_to_deg(f0().acos()))), // acos       0x00a04330
        5 => Ok(Value::Real(rad_to_deg(f0().atan()))), // atan       0x00a04360
        6 => Ok(Value::Real(f0().sqrt())),            // sqrt       0x00a04390
        // absl_float: `comiss x, 0.0 / jae` then xor of the sign bit — |x|, and
        // -0.0 comes back as +0.0.
        7 => Ok(Value::Real(f32::from_bits(f0().to_bits() & 0x7fff_ffff))),
        8 => Ok(Value::Int(i0().wrapping_abs())), // absl_int   0x00a043e0
        // rand_int(min, max) -> Random::get(min, max) on GameAccess::game_random.
        9 => Ok(Value::Int(host.game_random(i0(), arg(args, 1).as_int())?)),
        // rand_real() -> the mantissa trick on the same stream:
        //   s = s*1664525 + 1013904223;  f = bits(0x3f800000 | (s & 0x7fffff)) - 1.0
        10 => {
            let s = host.game_random_step()?;
            let bits = 0x3f80_0000u32 | (s & 0x007f_ffff);
            let x = f32::from_bits(bits);
            // The engine widens to double, subtracts 1.0, and narrows back.
            Ok(Value::Real(((x as f64) - 1.0) as f32))
        }
        // rand_seed(n): a negative seed asks the CRT for a time-derived one.
        11 => {
            let n = i0();
            if n < 0 {
                host.reseed_from_clock()?;
            } else {
                host.set_game_random_seed(n as u32)?;
            }
            Ok(Value::Null)
        }
        12 => Ok(Value::Int(host.game_random_seed()? as i32)), // rand_get_seed
        13 => Ok(Value::Real(minss(f0(), arg(args, 1).as_real()))), // min_val
        14 => Ok(Value::Real(maxss(f0(), arg(args, 1).as_real()))), // max_val

        // ---- TriggerUtilFuncSet ---------------------------------------------
        // Handled by the VM; reaching here means a host called us directly.
        15..=17 => Err(HostError::Unimplemented),

        // ---- StringUtilFuncSet ----------------------------------------------
        // length(str) is `String::curr_len`, a count of UTF-16 code units.
        18 => Ok(Value::Int(
            arg(args, 0)
                .as_string()
                .map_err(|_| HostError::BadArgs("length: cannot convert value to string"))?
                .encode_utf16()
                .count() as i32,
        )),
        // char_at(str, i): out of range (either end) yields 0, else the UTF-16 unit
        // zero-extended to int.
        19 => {
            let s = arg(args, 0)
                .as_string()
                .map_err(|_| HostError::BadArgs("char_at: cannot convert value to string"))?;
            let i = arg(args, 1).as_int();
            let units: Vec<u16> = s.encode_utf16().collect();
            if i < 0 || i as usize >= units.len() {
                Ok(Value::Int(0))
            } else {
                Ok(Value::Int(units[i as usize] as i32))
            }
        }
        // char_from_int(n): one UTF-16 code unit. Rust strings cannot represent an
        // unpaired surrogate, so reject that subdomain instead of silently replacing
        // it with U+FFFD (which `from_utf16_lossy` would do).
        20 => {
            let n = i0();
            let u = (n as u32 & 0xffff) as u16;
            if (0xd800..=0xdfff).contains(&u) {
                return Err(HostError::Unimplemented);
            }
            let ch = char::from_u32(u as u32).ok_or(HostError::Unimplemented)?;
            Ok(Value::str(ch.to_string()))
        }
        // parse(fmt, ...): the `$NUM0` / `$STRING0` substitution the scenarios use
        // for UI text. `0x00a04720` has NOT been decoded, and the placeholder
        // vocabulary is not derivable from the 311 corpus call sites alone, so this
        // stays unimplemented rather than invented.
        21 => Err(HostError::Unimplemented),
        22 => {
            let s = arg(args, 0)
                .as_string()
                .map_err(|_| HostError::Unimplemented)?;
            host.script_print(&s, false)?;
            Ok(Value::Null)
        }
        23 => {
            let s = arg(args, 0)
                .as_string()
                .map_err(|_| HostError::Unimplemented)?;
            host.script_print(&s, true)?;
            Ok(Value::Null)
        }

        // ---- ArrayUtilFuncSet ------------------------------------------------
        // add(array) `0x00a044a0`: append `blank_base->duplicate()`, return the new
        // element's index (`count - 1` after the increment).
        24 => {
            let a = arg(args, 0);
            let o = a
                .obj()
                .ok_or(HostError::BadArgs("add: not an array"))?
                .clone();
            let base = match &o.borrow().blank_base {
                Some(b) => (**b).clone(),
                None => return Ok(Value::Int(-1)),
            };
            let mut m = o.borrow_mut();
            m.values.push(cell(base));
            Ok(Value::Int(m.values.len() as i32 - 1))
        }
        // add(array, value) `0x00a044e0`: refuses (-1) when `blank_base` is null or
        // `blank_base->data_type != value->data_type`; otherwise appends
        // `value->duplicate()` stamped VM_VAR and returns its index.
        25 => {
            let a = arg(args, 0);
            let v = arg(args, 1);
            let o = a
                .obj()
                .ok_or(HostError::BadArgs("add: not an array"))?
                .clone();
            let ok = match &o.borrow().blank_base {
                Some(b) => b.data_type() == v.data_type(),
                None => false,
            };
            if !ok {
                return Ok(Value::Int(-1));
            }
            let mut m = o.borrow_mut();
            m.values.push(cell(v.duplicate()));
            Ok(Value::Int(m.values.len() as i32 - 1))
        }
        // remove(array, value) `0x00a04540` and insert(array, value, index)
        // `0x00a045b0` were not read past their type guards. Not implemented rather
        // than guessed; neither is called anywhere in the 363-script corpus.
        26 | 28 => Err(HostError::Unimplemented),
        // remove_index(array, i) -> ScriptArray::remove(int) `0x004d2080`: releases
        // the slot, shifts the tail down, decrements the count and returns the NEW
        // count. There is no bounds check in retail; ours refuses instead of
        // reading out of bounds, and says so.
        27 => {
            let a = arg(args, 0);
            let i = arg(args, 1).as_int();
            let o = a
                .obj()
                .ok_or(HostError::BadArgs("remove_index: not an array"))?
                .clone();
            let mut m = o.borrow_mut();
            if i < 0 || i as usize >= m.values.len() {
                return Err(HostError::BadArgs("remove_index: out of range"));
            }
            m.values.remove(i as usize);
            Ok(Value::Int(m.values.len() as i32))
        }
        // find(array, value) -> ScriptArray::find `0x009d5c70`: a linear scan that
        // compares with `elem->do_operator(OP_EQ_OP, value)` and then tests the
        // result with `is_false` (vtable +32). Returns -1 when absent.
        29 => {
            let a = arg(args, 0);
            let v = arg(args, 1);
            let o = a
                .obj()
                .ok_or(HostError::BadArgs("find: not an array"))?
                .clone();
            let m = o.borrow();
            for (i, c) in m.values.iter().enumerate() {
                let eq = crate::ops::do_operator(&c.borrow(), crate::opcode::OP_EQ_OP, Some(&v));
                if matches!(eq, Ok(ref r) if r.is_true()) {
                    return Ok(Value::Int(i as i32));
                }
            }
            Ok(Value::Int(-1))
        }
        // clear(array) -> ScriptArray::clear `0x009d5c30`: releases every element and
        // sets count to 0. `blank_base` survives, so the array can still grow.
        30 => {
            let a = arg(args, 0);
            let o = a
                .obj()
                .ok_or(HostError::BadArgs("clear: not an array"))?
                .clone();
            o.borrow_mut().values.clear();
            Ok(Value::Null)
        }
        _ => Err(HostError::Unimplemented),
    }
}

/// The set of indices [`call_util`] actually implements — the seed of a measured
/// coverage number rather than a claimed one.
pub fn implemented_indices() -> std::collections::BTreeSet<u32> {
    (0u32..=30)
        .filter(|i| !matches!(i, 15 | 16 | 17 | 21 | 26 | 28))
        .collect()
}

/// A host that implements exactly the utility surface and nothing else, over a
/// caller-supplied RNG. Useful on its own for running scripts that only compute,
/// and as the base a `don-sim`-backed host delegates to.
pub struct UtilHost {
    /// `GameAccess::game_random`'s seed word. The LCG is
    /// `s <- s*1664525 + 1013904223` [measured, `Random::get` `0x00a39cf0`].
    pub seed: u32,
    /// Everything `print` / `print_line` emitted, in order.
    pub output: Vec<String>,
}

impl Default for UtilHost {
    fn default() -> Self {
        UtilHost {
            seed: 0,
            output: Vec::new(),
        }
    }
}

impl UtilHost {
    pub fn with_seed(seed: u32) -> UtilHost {
        UtilHost {
            seed,
            output: Vec::new(),
        }
    }
}

impl Host for UtilHost {
    fn call(&mut self, decl: &BuiltinDecl, args: &[Value]) -> HostResult {
        match call_util(self, decl, args) {
            Some(r) => r,
            None => Err(HostError::Unimplemented),
        }
    }

    /// Retail `Random::get(int,int)` (`0x00a39d70`): equal bounds do not advance,
    /// inverted bounds are swapped, and the low 16 seed bits scale an exclusive
    /// range. Differential: 1,500,012 retail cases, zero mismatches.
    fn game_random(&mut self, lo: i32, hi: i32) -> Result<i32, HostError> {
        if lo == hi {
            return Ok(lo);
        }
        let (lo, hi) = if lo < hi { (lo, hi) } else { (hi, lo) };
        let s = self.game_random_step()?;
        let span = hi.wrapping_sub(lo) as u32;
        let scaled = ((s & 0xffff).wrapping_mul(span)) >> 16;
        Ok(lo.wrapping_add(scaled as i32))
    }

    fn game_random_step(&mut self) -> Result<u32, HostError> {
        self.seed = self
            .seed
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        Ok(self.seed)
    }

    fn game_random_seed(&self) -> Result<u32, HostError> {
        Ok(self.seed)
    }

    fn set_game_random_seed(&mut self, s: u32) -> Result<(), HostError> {
        self.seed = s;
        Ok(())
    }

    fn script_print(&mut self, s: &str, newline: bool) -> Result<(), HostError> {
        if newline {
            self.output.push(s.to_string());
        } else {
            match self.output.last_mut() {
                Some(l) => l.push_str(s),
                None => self.output.push(s.to_string()),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_table::builtin;

    fn call(h: &mut UtilHost, index: u32, args: &[Value]) -> HostResult {
        let d = builtin(index).unwrap();
        h.call(d, args)
    }

    #[test]
    fn trig_is_in_degrees() {
        let mut h = UtilHost::default();
        // sin(x) is sinf(x * pi / 180), so 90 degrees is 1, not sin(90 rad).
        match call(&mut h, 0, &[Value::Real(90.0)]) {
            Ok(Value::Real(v)) => assert!((v - 1.0).abs() < 1e-6, "{v}"),
            other => panic!("{other:?}"),
        }
        match call(&mut h, 3, &[Value::Real(1.0)]) {
            Ok(Value::Real(v)) => assert!((v - 90.0).abs() < 1e-3, "{v}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn absolute_values() {
        let mut h = UtilHost::default();
        assert_eq!(call(&mut h, 8, &[Value::Int(-7)]), Ok(Value::Int(7)));
        match call(&mut h, 7, &[Value::Real(-0.0)]) {
            Ok(Value::Real(v)) => assert!(v.is_sign_positive()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn string_utilities_count_utf16_units() {
        let mut h = UtilHost::default();
        assert_eq!(call(&mut h, 18, &[Value::str("abc")]), Ok(Value::Int(3)));
        assert_eq!(
            call(&mut h, 19, &[Value::str("abc"), Value::Int(1)]),
            Ok(Value::Int('b' as i32))
        );
        // Out of range at either end yields 0, not an error.
        assert_eq!(
            call(&mut h, 19, &[Value::str("abc"), Value::Int(9)]),
            Ok(Value::Int(0))
        );
        assert_eq!(
            call(&mut h, 19, &[Value::str("abc"), Value::Int(-1)]),
            Ok(Value::Int(0))
        );
        assert_eq!(call(&mut h, 20, &[Value::Int(65)]), Ok(Value::str("A")));
        assert_eq!(
            call(&mut h, 20, &[Value::Int(0xd800)]),
            Err(HostError::Unimplemented),
            "an unpaired UTF-16 surrogate must not be replaced approximately"
        );
    }

    #[test]
    fn array_utilities_operate_through_the_alias() {
        let mut h = UtilHost::default();
        let a = Value::array(crate::value::ScriptTy::Int.tag(), Value::Int(0), 0);
        assert_eq!(
            call(&mut h, 25, &[a.clone(), Value::Int(5)]),
            Ok(Value::Int(0))
        );
        assert_eq!(
            call(&mut h, 25, &[a.clone(), Value::Int(7)]),
            Ok(Value::Int(1))
        );
        assert_eq!(call(&mut h, 24, &[a.clone()]), Ok(Value::Int(2)));
        assert_eq!(
            call(&mut h, 29, &[a.clone(), Value::Int(7)]),
            Ok(Value::Int(1))
        );
        assert_eq!(
            call(&mut h, 29, &[a.clone(), Value::Int(99)]),
            Ok(Value::Int(-1))
        );
        assert_eq!(a.obj().unwrap().borrow().values.len(), 3);
        // A type-mismatched add is refused with -1, not an error.
        assert_eq!(
            call(&mut h, 25, &[a.clone(), Value::str("x")]),
            Ok(Value::Int(-1))
        );
        assert_eq!(
            call(&mut h, 27, &[a.clone(), Value::Int(0)]),
            Ok(Value::Int(2))
        );
        assert_eq!(a.obj().unwrap().borrow().values[0].borrow().as_int(), 7);
        assert_eq!(call(&mut h, 30, &[a.clone()]), Ok(Value::Null));
        assert!(a.obj().unwrap().borrow().values.is_empty());
    }

    #[test]
    fn the_unimplemented_ones_are_named_not_approximated() {
        let mut h = UtilHost::default();
        for i in [21u32, 26, 28] {
            assert_eq!(call(&mut h, i, &[]), Err(HostError::Unimplemented));
        }
    }

    #[test]
    fn rand_real_lands_in_the_unit_interval() {
        let mut h = UtilHost::with_seed(12345);
        for _ in 0..1000 {
            match call(&mut h, 10, &[]) {
                Ok(Value::Real(v)) => assert!((0.0..1.0).contains(&v), "{v}"),
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn rand_int_matches_retail_reduction_and_state_rules() {
        let mut h = UtilHost::with_seed(0);
        assert_eq!(
            call(&mut h, 9, &[Value::Int(0), Value::Int(0)]),
            Ok(Value::Int(0))
        );
        assert_eq!(h.seed, 0, "equal bounds do not advance retail Random");

        assert_eq!(
            call(&mut h, 9, &[Value::Int(0), Value::Int(1)]),
            Ok(Value::Int(0))
        );
        assert_eq!(h.seed, 0x3c6e_f35f);

        let mut forward = UtilHost::with_seed(0x89ab_cdef);
        let mut inverted = UtilHost::with_seed(0x89ab_cdef);
        let a = call(&mut forward, 9, &[Value::Int(-30), Value::Int(30)]);
        let b = call(&mut inverted, 9, &[Value::Int(30), Value::Int(-30)]);
        assert_eq!(a, b, "retail swaps inverted bounds");
        assert_eq!(forward.seed, inverted.seed);

        let mut h = UtilHost::with_seed(u32::MAX);
        for _ in 0..10_000 {
            match call(&mut h, 9, &[Value::Int(-5), Value::Int(7)]) {
                Ok(Value::Int(v)) => assert!((-5..7).contains(&v), "{v}"),
                other => panic!("{other:?}"),
            }
        }
    }
}
