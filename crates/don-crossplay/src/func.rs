// SPDX-License-Identifier: GPL-3.0-or-later
//! Ownership of an MSVC x86 `std::function` that crossed the DLL boundary.
//!
//! [`crate::abi`] pins the *shape* — 40 bytes, target pointer at `+0x24`.
//! [`crate::service`] retains one per interface setter. Neither says who owns
//! the heap block behind that target, and getting that wrong is a double free
//! or a leak inside `riseofnations.exe`, not a failing test.
//!
//! # The vtable, measured
//!
//! All 20 `??_7?$_Func_impl_no_alloc@…@std@@6B@` tables in
//! `ron-bin/dll/CrossplayProxy.dll`'s `.rdata` agree slot for slot:
//!
//! ```text
//! 0  _Copy(void* where)        -> new impl      1  _Move(void* where) -> new impl
//! 2  _Do_call(...)                              3  _Target_type()
//! 4  _Delete_this(bool deallocate)              5  _Get()
//! ```
//!
//! Slot 4 is corroborated independently by `~CrossPlayService`, which destroys
//! the retained lobby callbacks with `call dword ptr [edx+0x10]` at
//! `CrossplayProxy.dll` RVA `0x14435`. **[measured]**
//!
//! # Who destroys what
//!
//! MSVC x86 is a **callee-destroys-parameters** ABI, and both ends of this
//! interface are compiled that way:
//!
//! * `riseofnations.exe` `0x0056147c` constructs a `std::function` in a 40-byte
//!   stack slot with exception state `0x1d`, sets the state to `4` — i.e.
//!   "no longer mine to unwind" — and only then executes
//!   `call dword ptr [eax+4]`, `ICrossplayLogger::Register`. The by-value
//!   argument belongs to the callee from the call instruction onward.
//!   **[measured]**
//! * the shipped `CrossplayNetLibSys::set_p2p_callbacks` at `0x10017420` does
//!   the mirror image: clone-assign into the object, then destroy each
//!   by-value input.
//!
//! So a slot that takes `MsvcFunction` **by value** must destroy it, and a slot
//! that takes `*const MsvcFunction` must not. In the shipped 58-slot table
//! every `Set*Callback` setter is by reference and every request-completion
//! pair (`CreateLobby`, `GetLobby`, `FindLobbies`, `StartGame`,
//! `CancelGameStart`, `UpdateLobby`'s eighth argument, and the stats and
//! leaderboard family) is by value. **[measured — `gen/gen_abi.py --check-ret`,
//! 54 of 58 slots' `ret imm16` agree with the derived argument byte count]**
//!
//! # Why a cross-module `operator delete` is sound here
//!
//! `_Delete_this` is *the caller's own code*: the vtable pointer in the target
//! belongs to whichever module constructed the `std::function`. Calling it runs
//! that module's `operator delete` against that module's allocator, which is
//! why this is the correct primitive and a bitwise copy is not.
//!
//! It is additionally safe because there is only one heap in the process:
//! `riseofnations.exe`, `CrossplayProxy.dll` and `CrossplayNetLib.dll` all
//! import `MSVCP140.dll`, `VCRUNTIME140.dll` and
//! `api-ms-win-crt-heap-l1-1-0.dll` — the shared UCRT, not a statically linked
//! CRT each. **[measured — import directories of all three shipped images]**
//!
//! # The stack guard
//!
//! `_Delete_this`'s argument is `deallocate`, and MSVC computes it as
//! `target != &self`: a target living in the object's own 36-byte inline buffer
//! is destroyed in place, a heap target is freed. That test is an address
//! comparison, so it is only correct if `&self` really is the address the
//! caller pushed.
//!
//! For a Rust `extern "thiscall" fn(…, f: MsvcFunction)` that is a question
//! about how rustc lowers a `PassMode::Indirect { on_stack: true }` parameter,
//! not about the C++ ABI. It does keep the address: the emitted
//! `logger::thunks::register` opens with `lea edi, [esp+0x18]` — the caller's
//! own pushed slot, not a spilled local — and ends `ret 0x2c`.
//! **[measured — `crates/don-crossplay/dll/target/i686-pc-windows-msvc/release/CrossplayProxy.dll`]**
//!
//! That is a fact about one rustc, though, not a guarantee, so [`destroy`]
//! never passes `deallocate = true` for a target that lies on the current
//! thread's stack, read from the TIB at `fs:[4]`/`fs:[8]`. If a future rustc
//! ever spills a by-value parameter to a fresh local, the consequence is a
//! leaked inline target rather than `free()` on a stack address.

use crate::abi::MsvcFunction;

/// `std::_Func_base` vtable slot indices. **[measured]**
pub const FUNC_COPY_SLOT: usize = 0;
/// See [`FUNC_COPY_SLOT`].
pub const FUNC_MOVE_SLOT: usize = 1;
/// See [`FUNC_COPY_SLOT`].
pub const FUNC_DO_CALL_SLOT: usize = 2;
/// See [`FUNC_COPY_SLOT`].
pub const FUNC_TARGET_TYPE_SLOT: usize = 3;
/// See [`FUNC_COPY_SLOT`].
pub const FUNC_DELETE_THIS_SLOT: usize = 4;
/// See [`FUNC_COPY_SLOT`].
pub const FUNC_GET_SLOT: usize = 5;

/// The inline buffer is the 36 bytes before the target pointer. **[measured]**
pub const FUNC_INLINE_BUFFER_BYTES: usize = 0x24;
const _: () = assert!(core::mem::offset_of!(MsvcFunction, target) == FUNC_INLINE_BUFFER_BYTES);

/// A `std::function` holding nothing.
pub const fn empty() -> MsvcFunction {
    MsvcFunction {
        storage: [0; 36],
        target: 0,
    }
}

/// Whether a target is installed at all.
pub fn installed(f: &MsvcFunction) -> bool {
    f.target != 0
}

/// The address this object occupies, as the guest sees it.
///
/// Only meaningful where a pointer is 32 bits; elsewhere it is the low half of
/// a host address and is used for nothing but an inequality against `target`,
/// which is `0` in every host test.
pub fn base_of(f: *const MsvcFunction) -> u32 {
    f as *const u8 as usize as u32
}

/// Whether `address` is inside the running thread's stack.
///
/// Reads the x86 TIB directly — `fs:[4]` is `StackBase` (the high, exclusive
/// end) and `fs:[8]` is `StackLimit` (the low, inclusive end) — so no import is
/// added to this crate. Anything else answers `false`.
#[cfg(all(target_arch = "x86", target_os = "windows"))]
pub fn on_current_stack(address: u32) -> bool {
    let base: u32;
    let limit: u32;
    // SAFETY: two loads from the thread information block, which is mapped for
    // the lifetime of the thread and is read-only here.
    unsafe {
        core::arch::asm!(
            "mov {0:e}, fs:[4]",
            "mov {1:e}, fs:[8]",
            out(reg) base,
            out(reg) limit,
            options(nostack, preserves_flags, readonly),
        );
    }
    limit != 0 && base > limit && address >= limit && address < base
}

/// See the x86 Windows definition; everywhere else there is no such stack to
/// speak of and this answers `false`.
#[cfg(not(all(target_arch = "x86", target_os = "windows")))]
pub fn on_current_stack(_address: u32) -> bool {
    false
}

/// What [`destroy`] did, so a caller can report it rather than guess.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Destroyed {
    /// Nothing was installed.
    Empty,
    /// `_Delete_this(false)` ran: the target lived in the inline buffer.
    InPlace,
    /// `_Delete_this(true)` ran: the target was heap allocated.
    Deallocated,
    /// The target was neither the object's own inline buffer nor safely off the
    /// stack, so nothing was freed. See the module documentation.
    LeakedOnStack,
    /// The target had no vtable, so no destructor could be reached.
    Unusable,
}

#[cfg(target_arch = "x86")]
mod x86 {
    use super::*;
    use core::ffi::c_void;

    type FuncCopy = unsafe extern "thiscall" fn(*mut c_void, *mut c_void) -> *mut c_void;
    type FuncMove = unsafe extern "thiscall" fn(*mut c_void, *mut c_void) -> *mut c_void;
    type FuncDelete = unsafe extern "thiscall" fn(*mut c_void, bool);

    /// # Safety
    ///
    /// `f.target`, when non-zero, must be a live `_Func_base` whose first word
    /// is its vtable.
    pub unsafe fn vtable(f: &MsvcFunction) -> Option<*const *const c_void> {
        if f.target == 0 {
            return None;
        }
        let target = f.target as usize as *const *const *const c_void;
        // SAFETY: the caller's guarantee.
        let vtable = unsafe { *target };
        (!vtable.is_null()).then_some(vtable)
    }

    /// # Safety
    ///
    /// `f` must be at the address the ABI placed it at, and its target must be
    /// live. Afterwards `f` is empty.
    pub unsafe fn destroy(f: &mut MsvcFunction) -> Destroyed {
        let base = base_of(f);
        let had_target = f.target != 0;
        // SAFETY: the caller's guarantee.
        let Some(vtable) = (unsafe { vtable(f) }) else {
            *f = empty();
            // Read before the clear: an installed target with no vtable is a
            // corrupt object, not an empty one, and the two must not report the
            // same outcome.
            return if had_target {
                Destroyed::Unusable
            } else {
                Destroyed::Empty
            };
        };
        let target = f.target;
        let inline = target == base;
        if !inline && on_current_stack(target) {
            *f = empty();
            return Destroyed::LeakedOnStack;
        }
        // SAFETY: slot 4 is `_Delete_this`, measured; `deallocate` is MSVC's own
        // `target != &self` test, additionally guarded above.
        unsafe {
            let entry = *vtable.add(FUNC_DELETE_THIS_SLOT);
            let del: FuncDelete = core::mem::transmute(entry);
            del(target as usize as *mut c_void, !inline);
        }
        *f = empty();
        if inline {
            Destroyed::InPlace
        } else {
            Destroyed::Deallocated
        }
    }

    /// # Safety
    ///
    /// `destination` must be empty and at its ABI address; `source` must hold a
    /// live target or none.
    unsafe fn clone_into_empty(destination: &mut MsvcFunction, source: &MsvcFunction) -> bool {
        // SAFETY: the caller's guarantee.
        let Some(vtable) = (unsafe { vtable(source) }) else {
            return source.target == 0;
        };
        // SAFETY: slot 0 is `_Copy`, measured. It placement-constructs into
        // `where` and returns the new target, which is `where` itself when the
        // target fits the inline buffer.
        let target = unsafe {
            let copy: FuncCopy = core::mem::transmute(*vtable.add(FUNC_COPY_SLOT));
            copy(
                source.target as usize as *mut c_void,
                destination as *mut MsvcFunction as *mut c_void,
            )
        };
        destination.target = target as usize as u32;
        !target.is_null()
    }

    /// # Safety
    ///
    /// Both must be at their ABI addresses. `source` is left untouched — this
    /// is the by-reference discipline; a by-value caller destroys it after.
    pub unsafe fn retain(destination: &mut MsvcFunction, source: &MsvcFunction) -> bool {
        let mut replacement = empty();
        // SAFETY: `replacement` is a fresh empty local.
        if !unsafe { clone_into_empty(&mut replacement, source) } {
            return false;
        }
        // SAFETY: the caller's guarantee.
        unsafe { destroy(destination) };
        if replacement.target == 0 {
            *destination = empty();
            return true;
        }
        if replacement.target != base_of(&replacement) {
            // A heap target is independent of either object's address, so the
            // 40 bytes can simply move.
            *destination = replacement;
            return true;
        }
        // An inline target points into `replacement`, which is about to go out
        // of scope, so it has to be moved rather than copied.
        *destination = empty();
        // SAFETY: slot 1 is `_Move`, measured; it writes only the 36-byte
        // inline buffer, which is disjoint from `target` at `+0x24`.
        let moved = unsafe {
            let Some(vtable) = vtable(&replacement) else {
                return false;
            };
            let mv: FuncMove = core::mem::transmute(*vtable.add(FUNC_MOVE_SLOT));
            mv(
                replacement.target as usize as *mut c_void,
                destination as *mut MsvcFunction as *mut c_void,
            )
        };
        destination.target = moved as usize as u32;
        // SAFETY: the moved-from source still needs its destructor.
        unsafe { destroy(&mut replacement) };
        !moved.is_null()
    }
}

/// Destroy a `std::function` this code owns.
///
/// On x86 this runs the measured `_Delete_this` (vtable slot 4). Everywhere
/// else there is no guest code to run, so the object is simply cleared and the
/// host tests see the same state machine without the call.
///
/// # Safety
///
/// `f` must be at the address the ABI gave it and must hold either a live
/// target or none.
pub unsafe fn destroy(f: &mut MsvcFunction) -> Destroyed {
    #[cfg(target_arch = "x86")]
    {
        // SAFETY: forwarded.
        unsafe { x86::destroy(f) }
    }
    #[cfg(not(target_arch = "x86"))]
    {
        let was = f.target != 0;
        *f = empty();
        if was {
            Destroyed::Unusable
        } else {
            Destroyed::Empty
        }
    }
}

/// Take an independent copy of a `std::function` the caller still owns.
///
/// On x86 this runs the measured `_Copy` (slot 0), destroys whatever
/// `destination` held, and `_Move`s an inline result into place. Everywhere
/// else it is the bitwise copy the host tests have always exercised — which is
/// correct exactly when the target lives in the inline buffer, and is why this
/// function exists.
///
/// # Safety
///
/// Both objects must be at their ABI addresses, and `source` must hold either
/// a live target or none.
pub unsafe fn retain(destination: &mut MsvcFunction, source: &MsvcFunction) -> bool {
    #[cfg(target_arch = "x86")]
    {
        // SAFETY: forwarded.
        unsafe { x86::retain(destination, source) }
    }
    #[cfg(not(target_arch = "x86"))]
    {
        *destination = *source;
        true
    }
}

/// Take ownership of a `std::function` that arrived **by value**.
///
/// Equivalent to [`retain`] followed by [`destroy`] of the argument, which is
/// what MSVC's own move-assignment-from-a-parameter compiles to and what the
/// shipped `set_p2p_callbacks` does.
///
/// # Safety
///
/// As [`retain`], and `source` must be the caller's by-value parameter, which
/// this function consumes.
pub unsafe fn adopt(destination: &mut MsvcFunction, source: &mut MsvcFunction) -> bool {
    // SAFETY: forwarded.
    let ok = unsafe { retain(destination, source) };
    // SAFETY: forwarded; a by-value parameter is the callee's to destroy.
    unsafe { destroy(source) };
    ok
}

// ---------------------------------------------------------------------------
// Owned storage.
// ---------------------------------------------------------------------------

/// A `std::function` this code owns, at an address that never moves.
///
/// # Why the indirection is not optional
///
/// An inline target *is* a pointer into the object's own 36-byte buffer, so
/// moving the 40 bytes leaves `target` aimed at the vacated storage. MSVC never
/// hits this because its move constructor calls `_Move` (slot 1) and fixes the
/// pointer up; a Rust `struct` containing an `MsvcFunction` by value silently
/// does not, and every `BTreeMap::insert`, `Vec::push` and `let` binding is a
/// move. Keeping the object behind a `Box` makes its address stable for its
/// whole life, so `_Delete_this`'s `target != &self` test stays true at the end
/// exactly as it was at the start.
///
/// Dropping one runs the measured destructor, so the ownership is a type
/// invariant rather than a call every site has to remember.
pub struct OwnedFunction {
    slot: alloc::boxed::Box<MsvcFunction>,
}

impl OwnedFunction {
    /// Storage holding nothing.
    pub fn empty() -> Self {
        Self {
            slot: alloc::boxed::Box::new(empty()),
        }
    }

    /// Take an independent copy of a `std::function` the caller keeps — the
    /// discipline for every `*const MsvcFunction` parameter.
    ///
    /// # Safety
    ///
    /// `source` must hold either a live target or none.
    pub unsafe fn retain(source: &MsvcFunction) -> Self {
        let mut owned = Self::empty();
        // SAFETY: `owned.slot` is fresh, empty and at its final address.
        unsafe { retain(&mut owned.slot, source) };
        owned
    }

    /// Take over a `std::function` that arrived **by value** — the discipline
    /// for every `MsvcFunction` parameter.
    ///
    /// # Safety
    ///
    /// `source` must be the caller's by-value argument, at its ABI address.
    pub unsafe fn adopt(source: &mut MsvcFunction) -> Self {
        let mut owned = Self::empty();
        // SAFETY: as above; `adopt` also destroys `source`, which a by-value
        // parameter requires.
        unsafe { adopt(&mut owned.slot, source) };
        owned
    }

    /// The retained object, for reading only.
    pub fn value(&self) -> &MsvcFunction {
        &self.slot
    }

    /// The `_Func_base` address to dispatch through, or `0`.
    pub fn target(&self) -> u32 {
        self.slot.target
    }

    /// Whether anything is installed.
    pub fn installed(&self) -> bool {
        self.slot.target != 0
    }
}

impl Drop for OwnedFunction {
    fn drop(&mut self) {
        // SAFETY: the box has held this object at this address since `_Copy`
        // constructed it there.
        unsafe { destroy(&mut self.slot) };
    }
}

impl Default for OwnedFunction {
    fn default() -> Self {
        Self::empty()
    }
}

// ---------------------------------------------------------------------------
// A `_Func_impl` the tests can own.
// ---------------------------------------------------------------------------

/// A minimal `std::function` target, for tests that need an *installed*
/// callback rather than a fabricated pointer.
///
/// Before this module existed, a test could "install" a callback by writing an
/// arbitrary non-zero `target`, because retention was a bitwise copy that never
/// read it. Retention now runs `_Copy`, so a fabricated pointer is a fault on
/// the target where the ABI is real. This provides an actual six-slot
/// `_Func_base` whose `_Copy` behaves like an inline target — which is what a
/// small MSVC lambda is — so the same test is meaningful on the host and on
/// i686.
///
/// `_Do_call` only counts; nothing here reproduces a shipped callback body.
#[cfg(test)]
pub(crate) mod probe {
    use super::*;
    use core::ffi::c_void;
    use core::sync::atomic::{AtomicU32, Ordering};

    /// How many times a probe target was reached through `_Do_call`.
    pub(crate) static DO_CALLS: AtomicU32 = AtomicU32::new(0);
    /// How many times a probe target was destroyed.
    pub(crate) static DELETES: AtomicU32 = AtomicU32::new(0);

    macro_rules! probe_impl {
        ($abi:literal) => {
            use super::*;

            /// `_Copy(where)`: place a copy in the destination's inline buffer
            /// and return it, which is what MSVC does for a target that fits.
            pub(crate) unsafe extern $abi fn copy(
                _this: *mut c_void,
                where_: *mut c_void,
            ) -> *mut c_void {
                if where_.is_null() {
                    return core::ptr::null_mut();
                }
                // SAFETY: `where` is the 36-byte inline buffer of a live
                // `std::function`; one pointer fits.
                unsafe { *where_.cast::<*const *const c_void>() = VTABLE.as_ptr() };
                where_
            }

            pub(crate) unsafe extern $abi fn move_(
                this: *mut c_void,
                where_: *mut c_void,
            ) -> *mut c_void {
                // SAFETY: as `copy`.
                unsafe { copy(this, where_) }
            }

            pub(crate) unsafe extern $abi fn do_call(_this: *mut c_void, _argument: u32) {
                DO_CALLS.fetch_add(1, Ordering::Relaxed);
            }

            pub(crate) unsafe extern $abi fn target_type(_this: *mut c_void) -> *const c_void {
                core::ptr::null()
            }

            pub(crate) unsafe extern $abi fn delete_this(_this: *mut c_void, _deallocate: bool) {
                DELETES.fetch_add(1, Ordering::Relaxed);
            }

            pub(crate) unsafe extern $abi fn get(this: *mut c_void) -> *mut c_void {
                this
            }
        };
    }

    #[cfg(target_arch = "x86")]
    mod thunks {
        probe_impl!("thiscall");
    }
    #[cfg(not(target_arch = "x86"))]
    mod thunks {
        probe_impl!("C");
    }

    struct Table([*const c_void; 6]);
    // SAFETY: six code pointers, never written after construction.
    unsafe impl Sync for Table {}
    impl Table {
        fn as_ptr(&self) -> *const *const c_void {
            self.0.as_ptr()
        }
    }

    static VTABLE: Table = Table([
        thunks::copy as *const c_void,
        thunks::move_ as *const c_void,
        thunks::do_call as *const c_void,
        thunks::target_type as *const c_void,
        thunks::delete_this as *const c_void,
        thunks::get as *const c_void,
    ]);

    /// Turn `f` into an installed `std::function` with an **inline** target.
    ///
    /// The target is `f`'s own address, exactly as MSVC's `_Local()` test
    /// expects, so `f` must not move afterwards.
    pub(crate) fn install(f: &mut MsvcFunction) {
        let vtable = VTABLE.as_ptr() as usize as u32;
        f.storage[0..4].copy_from_slice(&vtable.to_le_bytes());
        f.target = base_of(f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_probe_target_survives_a_copy_and_a_destroy() {
        use core::sync::atomic::Ordering;
        let mut source = empty();
        probe::install(&mut source);
        assert!(installed(&source));
        let expected_source_target = source.target;

        let owned = unsafe { OwnedFunction::retain(&source) };
        assert!(owned.installed());
        #[cfg(target_arch = "x86")]
        {
            // The measured path: `_Copy` placement-constructs into the
            // destination, so an inline target ends up at the destination's own
            // address.
            assert_eq!(
                owned.target(),
                base_of(owned.value()),
                "an inline _Copy must land in the destination's own buffer"
            );
        }
        #[cfg(not(target_arch = "x86"))]
        {
            // No guest code exists here, so retention degrades to the bitwise
            // copy the host tests have always exercised.
            assert_eq!(owned.target(), expected_source_target);
        }
        drop(owned);
        // A by-reference retain leaves the caller's object untouched either way.
        assert_eq!(source.target, expected_source_target);

        // `destroy` reports what it did, which is an observation of this call
        // rather than of a process-wide counter — the test harness runs these
        // in parallel and the probe's counters are shared.
        let outcome = unsafe { destroy(&mut source) };
        if cfg!(target_arch = "x86") {
            assert_eq!(
                outcome,
                Destroyed::InPlace,
                "an inline target must be destroyed without a deallocation"
            );
            assert!(probe::DELETES.load(Ordering::Relaxed) > 0);
        } else {
            assert_eq!(outcome, Destroyed::Unusable);
        }
        assert_eq!(source.target, 0);
    }

    #[test]
    fn an_owned_function_keeps_one_address_across_moves() {
        let owned = OwnedFunction::empty();
        let first = owned.value() as *const MsvcFunction as usize;
        let moved = owned;
        let boxed = alloc::vec![moved];
        assert_eq!(boxed[0].value() as *const MsvcFunction as usize, first);
        assert!(!boxed[0].installed());
        assert_eq!(boxed[0].target(), 0);
    }

    #[test]
    fn slot_indices_are_the_measured_func_base_layout() {
        assert_eq!(FUNC_COPY_SLOT, 0);
        assert_eq!(FUNC_MOVE_SLOT, 1);
        assert_eq!(FUNC_DO_CALL_SLOT, 2);
        assert_eq!(FUNC_TARGET_TYPE_SLOT, 3);
        assert_eq!(FUNC_DELETE_THIS_SLOT, 4);
        assert_eq!(FUNC_GET_SLOT, 5);
        assert_eq!(FUNC_INLINE_BUFFER_BYTES, 36);
        assert_eq!(core::mem::size_of::<MsvcFunction>(), 40);
    }

    #[test]
    fn an_empty_function_destroys_to_empty() {
        let mut f = empty();
        assert!(!installed(&f));
        // SAFETY: `f` is a local with no target.
        assert_eq!(unsafe { destroy(&mut f) }, Destroyed::Empty);
        assert_eq!(f.target, 0);
    }

    #[test]
    fn retaining_an_empty_source_clears_the_destination() {
        let mut destination = empty();
        destination.storage[0] = 0xAB;
        let source = empty();
        // SAFETY: both are locals with no target.
        assert!(unsafe { retain(&mut destination, &source) });
        assert_eq!(destination.target, 0);
    }

    #[test]
    fn a_host_stack_address_is_never_reported_as_guest_stack() {
        // The TIB read only exists on x86 Windows; everywhere else the guard is
        // constant `false` and the caller falls back to the address test.
        let probe = empty();
        let _ = base_of(&probe);
        #[cfg(not(all(target_arch = "x86", target_os = "windows")))]
        assert!(!on_current_stack(base_of(&probe)));
    }
}
