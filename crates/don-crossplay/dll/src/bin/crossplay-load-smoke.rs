// SPDX-License-Identifier: GPL-3.0-or-later
//! Load-only executable acceptance for the replacement `CrossplayProxy.dll`.
//!
//! Deliberately a separate PE32 process that resolves the DLL with
//! `LoadLibraryW`/`GetProcAddress` rather than linking against it. Static
//! export inspection cannot prove that Windows can load the image, that the
//! Rust runtime starts inside it, that an x86 `__thiscall` crosses the vtable
//! with the stack balanced, or — the part this lane exists for — that the MSVC
//! `std::function` ownership primitives actually run.
//!
//! # What the `std::function` probes are
//!
//! The open item this smoke closes is that retaining a caller's
//! `std::function` by copying its 40 bytes is wrong whenever the target is not
//! in the inline buffer. Closing it means calling `_Copy` (vtable slot 0) and
//! `_Delete_this` (slot 4), which is *guest* code — so this process supplies
//! guest code: two synthetic `_Func_impl` vtables built to the measured
//! six-slot layout, one behaving like a heap-allocated target and one like an
//! inline target. Every slot counts its own invocations, so what the DLL did is
//! observed from the caller's side rather than reported by the DLL.
//!
//! This is not a claim that MSVC's real `_Func_impl` behaves like these; it is
//! a claim that the DLL calls the right slots, in the right order, with the
//! right `deallocate` flag, and leaves no target undestroyed.
//!
//! It is also the gate that caught a wrong calling convention that every
//! compile-time layout assertion in `abi.rs` passed: `MsvcFunction` was
//! declared `#[repr(C, align(8))]`, and on `i686-pc-windows-msvc` an aggregate
//! aligned above 4 is passed **as a pointer** rather than pushed. The emitted
//! `Register` ended `ret 8` where the shipped one pops 44, so the first
//! by-value callback the game handed over would have destroyed its own stack
//! frame. Static export parity would never have seen it.
//!
//! # What it does not do
//!
//! It never reads or modifies a retail process, never installs anything into a
//! game directory, and requires no `riseofnations.exe`. A successful run emits
//! one `don.crossplay-load-smoke.v1` JSON line.

#![allow(clippy::missing_safety_doc)]

use core::ffi::c_void;
use std::env;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};

// ---------------------------------------------------------------------------
// The shipped export surface.
// ---------------------------------------------------------------------------

const LOGGER_EXPORT: &[u8] = b"?Logger@Logging@Crossplay@@YAPAVICrossplayLogger@12@XZ\0";
const SERVICE_EXPORT: &[u8] = b"?Service@Crossplay@@YAPAUICrossPlayService@1@XZ\0";
const AMD_EXPORT: &[u8] = b"AmdPowerXpressRequestHighPerformance\0";
const NV_EXPORT: &[u8] = b"NvOptimusEnablement\0";
const SHUTDOWN_EXPORT: &[u8] = b"don_crossplay_shutdown\0";

/// **[measured — `ron-bin/dll/CrossplayProxy.dll`, `.rdata` RVA `0x9adc4`]**
const SERVICE_SLOTS: usize = 58;
/// **[measured — `CrossplayProxy.pdb`]**
const LOGGER_SLOTS: usize = 4;

const LOG_LEVEL_INFO: i32 = 2;
const LOG_LEVEL_ERROR: i32 = 8;

// ---------------------------------------------------------------------------
// MSVC boundary objects, redeclared here so this process is an independent
// witness rather than a second reader of the same declarations.
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
struct MsvcWstring {
    raw: [u8; 24],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MsvcFunction {
    storage: [u8; 36],
    target: u32,
}

impl MsvcFunction {
    const fn empty() -> Self {
        Self {
            storage: [0; 36],
            target: 0,
        }
    }
}

const WSTRING_SIZE_OFFSET: usize = 16;
const WSTRING_CAPACITY_OFFSET: usize = 20;

fn sso_wstring(text: &str) -> MsvcWstring {
    let units: Vec<u16> = text.encode_utf16().collect();
    assert!(units.len() <= 7, "sso_wstring is for short strings");
    let mut raw = [0u8; 24];
    for (index, unit) in units.iter().enumerate() {
        raw[index * 2..index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
    }
    raw[WSTRING_SIZE_OFFSET..WSTRING_SIZE_OFFSET + 4]
        .copy_from_slice(&(units.len() as u32).to_le_bytes());
    raw[WSTRING_CAPACITY_OFFSET..WSTRING_CAPACITY_OFFSET + 4].copy_from_slice(&7u32.to_le_bytes());
    MsvcWstring { raw }
}

/// A `wstring` whose characters live in a **CRT heap** buffer.
///
/// The buffer comes from the CRT's `malloc`, not Rust's allocator: Rust's
/// Windows allocator is `HeapAlloc(GetProcessHeap())`, and the DLL releases a
/// long `wstring` with the CRT's `free`. Those are different heaps, so building
/// this out of a `Vec` would hand the DLL a pointer its allocator does not own.
/// The shared-UCRT import check in `check-exports.py` is the other half of this.
fn heap_wstring(text: &str) -> (MsvcWstring, u32, usize) {
    let units: Vec<u16> = text.encode_utf16().collect();
    assert!(
        units.len() >= 8,
        "heap_wstring is for strings past the SSO bound"
    );
    let bytes = (units.len() + 1) * 2;
    // SAFETY: a plain sized CRT allocation, checked for null.
    let buffer = unsafe { crt_malloc(bytes) };
    assert!(!buffer.is_null(), "CRT malloc failed");
    // SAFETY: `buffer` owns `bytes` writable bytes.
    unsafe {
        let out = buffer.cast::<u16>();
        for (index, unit) in units.iter().enumerate() {
            *out.add(index) = *unit;
        }
        *out.add(units.len()) = 0;
    }
    let mut raw = [0u8; 24];
    let address = buffer as usize as u32;
    raw[0..4].copy_from_slice(&address.to_le_bytes());
    raw[WSTRING_SIZE_OFFSET..WSTRING_SIZE_OFFSET + 4]
        .copy_from_slice(&(units.len() as u32).to_le_bytes());
    raw[WSTRING_CAPACITY_OFFSET..WSTRING_CAPACITY_OFFSET + 4]
        .copy_from_slice(&(units.len() as u32).to_le_bytes());
    (MsvcWstring { raw }, address, bytes)
}

extern "C" {
    #[link_name = "malloc"]
    fn crt_malloc(size: usize) -> *mut c_void;
    #[link_name = "free"]
    fn crt_free(ptr: *mut c_void);
}

// ---------------------------------------------------------------------------
// Synthetic `std::function` targets.
// ---------------------------------------------------------------------------

static COPY_CALLS: AtomicU32 = AtomicU32::new(0);
static MOVE_CALLS: AtomicU32 = AtomicU32::new(0);
static DELETE_CALLS: AtomicU32 = AtomicU32::new(0);
static DELETE_DEALLOCATING: AtomicU32 = AtomicU32::new(0);
static DELETE_IN_PLACE: AtomicU32 = AtomicU32::new(0);
static LIVE_TARGETS: AtomicI32 = AtomicI32::new(0);
static DO_CALLS: AtomicU32 = AtomicU32::new(0);
static LAST_DO_CALL_ARG: AtomicU32 = AtomicU32::new(0);

/// The six-slot `std::_Func_base` vtable. **[measured — all 20
/// `_Func_impl_no_alloc` tables in `ron-bin/dll/CrossplayProxy.dll`'s `.rdata`
/// agree slot for slot: `_Copy`, `_Move`, `_Do_call`, `_Target_type`,
/// `_Delete_this`, `_Get`]**
#[repr(C)]
struct FuncVtable {
    copy: unsafe extern "thiscall" fn(*mut FakeTarget, *mut c_void) -> *mut c_void,
    move_: unsafe extern "thiscall" fn(*mut FakeTarget, *mut c_void) -> *mut c_void,
    do_call: *const c_void,
    target_type: unsafe extern "thiscall" fn(*mut FakeTarget) -> *const c_void,
    delete_this: unsafe extern "thiscall" fn(*mut FakeTarget, bool),
    get: unsafe extern "thiscall" fn(*mut FakeTarget) -> *mut c_void,
}

// SAFETY: a table of code pointers, never written after construction.
unsafe impl Sync for FuncVtable {}

/// A stand-in `_Func_impl`: a vtable pointer and eight bytes of payload, which
/// fits the 36-byte inline buffer exactly like a small MSVC lambda.
#[repr(C)]
#[derive(Clone, Copy)]
struct FakeTarget {
    vftable: *const FuncVtable,
    tag: u32,
    payload: u32,
}

unsafe extern "thiscall" fn heap_copy(this: *mut FakeTarget, _where: *mut c_void) -> *mut c_void {
    COPY_CALLS.fetch_add(1, Ordering::SeqCst);
    // SAFETY: `this` is one of ours.
    let value = unsafe { *this };
    // SAFETY: a plain sized CRT allocation.
    let fresh = unsafe { crt_malloc(core::mem::size_of::<FakeTarget>()) }.cast::<FakeTarget>();
    if fresh.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: `fresh` owns the bytes just obtained.
    unsafe { *fresh = value };
    LIVE_TARGETS.fetch_add(1, Ordering::SeqCst);
    fresh.cast()
}

unsafe extern "thiscall" fn heap_move(this: *mut FakeTarget, where_: *mut c_void) -> *mut c_void {
    MOVE_CALLS.fetch_add(1, Ordering::SeqCst);
    COPY_CALLS.fetch_sub(1, Ordering::SeqCst);
    // SAFETY: as `heap_copy`.
    unsafe { heap_copy(this, where_) }
}

unsafe extern "thiscall" fn inline_copy(this: *mut FakeTarget, where_: *mut c_void) -> *mut c_void {
    COPY_CALLS.fetch_add(1, Ordering::SeqCst);
    let destination = where_.cast::<FakeTarget>();
    if destination.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: `where` is at least the 36-byte inline buffer of a live
    // `std::function`, and `FakeTarget` is 12 bytes.
    unsafe { *destination = *this };
    LIVE_TARGETS.fetch_add(1, Ordering::SeqCst);
    destination.cast()
}

unsafe extern "thiscall" fn inline_move(this: *mut FakeTarget, where_: *mut c_void) -> *mut c_void {
    MOVE_CALLS.fetch_add(1, Ordering::SeqCst);
    COPY_CALLS.fetch_sub(1, Ordering::SeqCst);
    // SAFETY: as `inline_copy`.
    unsafe { inline_copy(this, where_) }
}

unsafe extern "thiscall" fn delete_this(this: *mut FakeTarget, deallocate: bool) {
    DELETE_CALLS.fetch_add(1, Ordering::SeqCst);
    LIVE_TARGETS.fetch_sub(1, Ordering::SeqCst);
    if deallocate {
        DELETE_DEALLOCATING.fetch_add(1, Ordering::SeqCst);
        // SAFETY: `deallocate` is only true for targets `heap_copy` produced
        // with `crt_malloc`, or for the originals this process allocated the
        // same way.
        unsafe { crt_free(this.cast()) };
    } else {
        DELETE_IN_PLACE.fetch_add(1, Ordering::SeqCst);
    }
}

unsafe extern "thiscall" fn target_type(_this: *mut FakeTarget) -> *const c_void {
    core::ptr::null()
}

unsafe extern "thiscall" fn get_target(this: *mut FakeTarget) -> *mut c_void {
    this.cast()
}

/// `void __cdecl(T)` reduced to one 4-byte argument, which is what the shipped
/// `_Do_call` for `std::function<void(std::wstring)>` is: `ret 4`.
/// **[measured]**
unsafe extern "thiscall" fn do_call_one(_this: *mut FakeTarget, argument: u32) {
    DO_CALLS.fetch_add(1, Ordering::SeqCst);
    LAST_DO_CALL_ARG.store(argument, Ordering::SeqCst);
}

static HEAP_VTABLE: FuncVtable = FuncVtable {
    copy: heap_copy,
    move_: heap_move,
    do_call: do_call_one as *const c_void,
    target_type,
    delete_this,
    get: get_target,
};

static INLINE_VTABLE: FuncVtable = FuncVtable {
    copy: inline_copy,
    move_: inline_move,
    do_call: do_call_one as *const c_void,
    target_type,
    delete_this,
    get: get_target,
};

/// Build a `std::function` whose target is a CRT-heap `FakeTarget`.
///
/// The original is always heap allocated so that destroying it is a real
/// `free`; `vtable` decides whether the *copy* the DLL takes behaves like a
/// heap target or an inline one.
fn make_function(vtable: &'static FuncVtable, tag: u32) -> MsvcFunction {
    // SAFETY: a plain sized CRT allocation, checked for null.
    let target = unsafe { crt_malloc(core::mem::size_of::<FakeTarget>()) }.cast::<FakeTarget>();
    assert!(!target.is_null(), "CRT malloc failed");
    // SAFETY: `target` owns the bytes just obtained.
    unsafe {
        *target = FakeTarget {
            vftable: vtable,
            tag,
            payload: 0xD011_0000 | tag,
        }
    };
    LIVE_TARGETS.fetch_add(1, Ordering::SeqCst);
    let mut function = MsvcFunction::empty();
    function.target = target as usize as u32;
    function
}

/// Release a `std::function` this process still owns, through the same measured
/// slot the DLL uses.
unsafe fn release_function(function: &mut MsvcFunction) {
    if function.target == 0 {
        return;
    }
    // SAFETY: the target is one of ours and was never handed away.
    unsafe {
        let table = *(function.target as usize as *const *const FuncVtable);
        ((*table).delete_this)(function.target as usize as *mut FakeTarget, true);
    }
    function.target = 0;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Counters {
    copies: u32,
    moves: u32,
    deletes: u32,
    deallocating: u32,
    in_place: u32,
    live: i32,
}

fn counters() -> Counters {
    Counters {
        copies: COPY_CALLS.load(Ordering::SeqCst),
        moves: MOVE_CALLS.load(Ordering::SeqCst),
        deletes: DELETE_CALLS.load(Ordering::SeqCst),
        deallocating: DELETE_DEALLOCATING.load(Ordering::SeqCst),
        in_place: DELETE_IN_PLACE.load(Ordering::SeqCst),
        live: LIVE_TARGETS.load(Ordering::SeqCst),
    }
}

// ---------------------------------------------------------------------------
// Win32.
// ---------------------------------------------------------------------------

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
    fn GetLastError() -> u32;
}

fn wide(path: &OsStr) -> Vec<u16> {
    path.encode_wide().chain(std::iter::once(0)).collect()
}

// ---------------------------------------------------------------------------
// Stack discipline.
// ---------------------------------------------------------------------------

fn read_esp() -> u32 {
    let value: u32;
    // SAFETY: reads the stack pointer, touching nothing.
    unsafe {
        std::arch::asm!(
            "mov {0:e}, esp",
            out(reg) value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

fn checked_call<T>(label: &str, checks: &mut usize, call: impl FnOnce() -> T) -> T {
    let before = read_esp();
    let result = call();
    let after = read_esp();
    if before != after {
        fail(&format!(
            "stack cleanup mismatch after {label}: before=0x{before:08x} after=0x{after:08x}"
        ));
    }
    *checks += 1;
    result
}

fn fail(message: &str) -> ! {
    let escaped = message.replace('\\', "\\\\").replace('"', "\\\"");
    eprintln!(
        "{{\"schema\":\"don.crossplay-load-smoke.v1\",\"status\":\"fail\",\"error\":\"{escaped}\",\"retail_process_modified\":false}}"
    );
    std::process::exit(1)
}

fn require(condition: bool, message: &str) {
    if !condition {
        fail(message);
    }
}

/// # Safety
///
/// `object` must be live and its first word must be a vtable pointer with at
/// least `slots` entries.
unsafe fn vtable_slots(object: *mut c_void, slots: usize) -> Vec<*const c_void> {
    // SAFETY: the caller's guarantee.
    let table = unsafe { *object.cast::<*const *const c_void>() };
    require(!table.is_null(), "object has a null vtable pointer");
    // SAFETY: the caller's guarantee.
    (0..slots)
        .map(|index| unsafe { *table.add(index) })
        .collect()
}

/// Reinterpret vtable entry `$index` as the signature `abi.rs` records for it.
macro_rules! slot {
    ($slots:expr, $index:expr, $ty:ty) => {
        // SAFETY: `crates/don-crossplay/src/abi.rs` records the signature of
        // each slot index; every use below names the slot it transmutes.
        unsafe { core::mem::transmute::<*const c_void, $ty>($slots[$index]) }
    };
}

type Void1 = unsafe extern "thiscall" fn(*mut c_void);
type WstringArg = unsafe extern "thiscall" fn(*mut c_void, *const MsvcWstring);
type FunctionRef = unsafe extern "thiscall" fn(*mut c_void, *const MsvcFunction);

fn main() {
    let mut args = env::args_os().skip(1);
    let dll = PathBuf::from(match args.next() {
        Some(path) => path,
        None => fail("usage: crossplay-load-smoke <CrossplayProxy.dll> [trace.log]"),
    });
    if let Some(trace) = args.next() {
        // Read by the DLL on its first export call, so it has to be set before
        // anything inside the image runs.
        env::set_var("DON_CROSSPLAY_TRACE", &trace);
    }
    env::set_var("DON_CROSSPLAY_USER_ID", "smoke-local");
    env::set_var("DON_CROSSPLAY_USER_NAME", "smoke");
    env::set_var("DON_CROSSPLAY_DIRECTORY", "listen:127.0.0.1:0");

    let mut checks = 0usize;
    let mut findings: Vec<String> = Vec::new();

    // -- load ---------------------------------------------------------------
    let wide_path = wide(dll.as_os_str());
    // SAFETY: a NUL-terminated wide path.
    let module = unsafe { LoadLibraryW(wide_path.as_ptr()) };
    if module.is_null() {
        // SAFETY: no intervening Win32 call.
        let code = unsafe { GetLastError() };
        fail(&format!(
            "LoadLibraryW failed with {code} for {}",
            dll.display()
        ));
    }

    let resolve = |name: &[u8]| -> *mut c_void {
        // SAFETY: `module` is loaded and `name` is NUL-terminated.
        unsafe { GetProcAddress(module, name.as_ptr()) }
    };
    let logger_export = resolve(LOGGER_EXPORT);
    let service_export = resolve(SERVICE_EXPORT);
    let amd_export = resolve(AMD_EXPORT);
    let nv_export = resolve(NV_EXPORT);
    let shutdown_export = resolve(SHUTDOWN_EXPORT);
    require(
        !logger_export.is_null(),
        "ordinal 1 did not resolve by name",
    );
    require(
        !service_export.is_null(),
        "ordinal 2 did not resolve by name",
    );
    require(!amd_export.is_null(), "ordinal 3 did not resolve by name");
    require(!nv_export.is_null(), "ordinal 4 did not resolve by name");
    require(
        !shutdown_export.is_null(),
        "DoN shutdown export did not resolve by name",
    );

    // -- the two data exports ----------------------------------------------
    // SAFETY: both resolve to a `DWORD` in the image's writable data.
    let (amd_value, nv_value) = unsafe { (*amd_export.cast::<u32>(), *nv_export.cast::<u32>()) };
    require(
        amd_value == 1,
        "AmdPowerXpressRequestHighPerformance is not 1",
    );
    require(nv_value == 1, "NvOptimusEnablement is not 1");

    // -- ordinal 1 ----------------------------------------------------------
    let logger_slot = [logger_export as *const c_void];
    let logger_factory = slot!(logger_slot, 0, unsafe extern "C" fn() -> *mut c_void);
    let logger = checked_call("Logger()", &mut checks, || unsafe { logger_factory() });
    require(!logger.is_null(), "Logger() returned null");
    let again = checked_call("Logger() again", &mut checks, || unsafe {
        logger_factory()
    });
    require(again == logger, "Logger() is not a stable singleton");

    // SAFETY: `logger` is an `ICrossplayLogger`.
    let logger_vtable = unsafe { vtable_slots(logger, LOGGER_SLOTS) };
    require(
        logger_vtable.iter().all(|entry| !entry.is_null()),
        "an ICrossplayLogger slot is null",
    );

    // slot 3: `void Log(LogLevel, wstring, const char*, int)`, wstring by value.
    let log = slot!(
        logger_vtable,
        3,
        unsafe extern "thiscall" fn(*mut c_void, i32, MsvcWstring, *const u8, i32)
    );
    let file = b"crossplay-load-smoke.rs\0";
    checked_call(
        "ICrossplayLogger[3] Log (inline wstring)",
        &mut checks,
        || unsafe {
            log(
                logger,
                LOG_LEVEL_INFO,
                sso_wstring("short"),
                file.as_ptr(),
                1,
            )
        },
    );

    // The heap case: the DLL must release the buffer on the shared CRT heap.
    let (heap_message, heap_buffer, heap_bytes) =
        heap_wstring("a message past the small-string bound");
    checked_call(
        "ICrossplayLogger[3] Log (heap wstring)",
        &mut checks,
        || unsafe { log(logger, LOG_LEVEL_ERROR, heap_message, file.as_ptr(), 2) },
    );
    // Not a proof of the free — nothing portable is — but a just-released block
    // is the likeliest one to come back, and a *different* answer here would
    // mean the DLL kept it.
    // SAFETY: a plain sized CRT allocation, released immediately.
    let reprobe = unsafe { crt_malloc(heap_bytes) };
    let heap_block_reused = reprobe as usize as u32 == heap_buffer;
    // SAFETY: `reprobe` came from `crt_malloc`.
    unsafe { crt_free(reprobe) };
    findings.push(format!("heap_wstring_block_reused={heap_block_reused}"));

    // slot 1: `void Register(LogLevel, std::function)`, by value.
    let register = slot!(
        logger_vtable,
        1,
        unsafe extern "thiscall" fn(*mut c_void, i32, MsvcFunction)
    );
    // slot 2: `void UnregisterAll()`.
    let unregister_all = slot!(logger_vtable, 2, Void1);

    // --- heap-target ownership cycle ---------------------------------------
    let baseline = counters();
    let heap_function = make_function(&HEAP_VTABLE, 1);
    checked_call(
        "ICrossplayLogger[1] Register (heap target)",
        &mut checks,
        || unsafe { register(logger, LOG_LEVEL_INFO, heap_function) },
    );
    let after_register = counters();
    require(
        after_register.copies == baseline.copies + 1,
        "Register did not run _Copy exactly once on a heap target",
    );
    require(
        after_register.deletes == baseline.deletes + 1
            && after_register.deallocating == baseline.deallocating + 1,
        "Register did not destroy the by-value argument with deallocate=true",
    );
    checked_call(
        "ICrossplayLogger[2] UnregisterAll (heap target)",
        &mut checks,
        || unsafe { unregister_all(logger) },
    );
    let after_unregister = counters();
    require(
        after_unregister.deletes == baseline.deletes + 2
            && after_unregister.deallocating == baseline.deallocating + 2,
        "UnregisterAll did not release the retained heap copy",
    );
    require(
        after_unregister.live == baseline.live,
        "a heap-target std::function outlived the register/unregister cycle",
    );

    // --- inline-target ownership cycle -------------------------------------
    let baseline = counters();
    let inline_function = make_function(&INLINE_VTABLE, 2);
    checked_call(
        "ICrossplayLogger[1] Register (inline target)",
        &mut checks,
        || unsafe { register(logger, LOG_LEVEL_INFO, inline_function) },
    );
    let after_register = counters();
    require(
        after_register.copies == baseline.copies + 1,
        "Register did not run _Copy exactly once on an inline target",
    );
    require(
        after_register.moves == baseline.moves + 1,
        "an inline _Copy result was not relocated into owned storage with _Move",
    );
    require(
        after_register.deallocating == baseline.deallocating + 1,
        "the heap original behind an inline copy was not freed exactly once",
    );
    require(
        after_register.in_place > baseline.in_place,
        "no inline target was destroyed with deallocate=false",
    );
    checked_call(
        "ICrossplayLogger[2] UnregisterAll (inline target)",
        &mut checks,
        || unsafe { unregister_all(logger) },
    );
    require(
        counters().live == baseline.live,
        "an inline-target std::function outlived the register/unregister cycle",
    );

    // slot 0: `void* __vecDelDtor(unsigned int)`. Retail's logger is a
    // function-local static and never calls this; crossing it proves the slot
    // is callable and does not take the singleton away.
    let logger_dtor = slot!(
        logger_vtable,
        0,
        unsafe extern "thiscall" fn(*mut c_void, u32) -> *mut c_void
    );
    let returned = checked_call("ICrossplayLogger[0] __vecDelDtor", &mut checks, || unsafe {
        logger_dtor(logger, 0)
    });
    require(
        returned == logger,
        "the deleting destructor did not return this",
    );

    // -- ordinal 2 ----------------------------------------------------------
    let service_slot = [service_export as *const c_void];
    let service_factory = slot!(service_slot, 0, unsafe extern "C" fn() -> *mut c_void);
    let service = checked_call("Service()", &mut checks, || unsafe { service_factory() });
    require(!service.is_null(), "Service() returned null");
    let again = checked_call("Service() again", &mut checks, || unsafe {
        service_factory()
    });
    require(again == service, "Service() is not a stable singleton");

    // SAFETY: `service` is an `ICrossPlayService`.
    let vt = unsafe { vtable_slots(service, SERVICE_SLOTS) };
    let null_slots = vt.iter().filter(|entry| entry.is_null()).count();
    require(null_slots == 0, "an ICrossPlayService slot is null");

    let init = slot!(vt, 0, Void1);
    checked_call("ICrossPlayService[0] Init", &mut checks, || unsafe {
        init(service)
    });

    // slot 1 `SetServiceUrl` is one of the eight slots MSVC folded onto a bare
    // `ret 4` in the shipped build. **[measured — RVA 0x145b0]**
    let set_service_url = slot!(vt, 1, WstringArg);
    let url = sso_wstring("don");
    checked_call(
        "ICrossPlayService[1] SetServiceUrl",
        &mut checks,
        || unsafe { set_service_url(service, &url) },
    );

    // slot 2 `SetServiceErrorCallback(const std::function&)` — by reference, so
    // the DLL must `_Copy` and must not destroy the caller's object.
    let set_error_callback = slot!(vt, 2, FunctionRef);
    let error_baseline = counters();
    let mut error_callback = make_function(&HEAP_VTABLE, 3);
    checked_call(
        "ICrossPlayService[2] SetServiceErrorCallback",
        &mut checks,
        || unsafe { set_error_callback(service, &error_callback) },
    );
    let after_set = counters();
    require(
        after_set.copies == error_baseline.copies + 1,
        "a by-reference setter did not run _Copy",
    );
    require(
        after_set.deletes == error_baseline.deletes,
        "a by-reference setter destroyed the caller's std::function",
    );
    // The caller still owns it, which is the whole point.
    // SAFETY: the target is one of ours and was never handed away.
    unsafe { release_function(&mut error_callback) };

    let set_reliability = slot!(vt, 3, unsafe extern "thiscall" fn(*mut c_void, bool));
    checked_call(
        "ICrossPlayService[3] SetReliability",
        &mut checks,
        || unsafe { set_reliability(service, true) },
    );

    let get_session_status = slot!(vt, 7, unsafe extern "thiscall" fn(*mut c_void) -> i32);
    let before_session = checked_call(
        "ICrossPlayService[7] GetSessionStatus",
        &mut checks,
        || unsafe { get_session_status(service) },
    );

    // slot 4 `StartSession(const wstring&, const std::function&, const std::function&)`
    let start_session = slot!(
        vt,
        4,
        unsafe extern "thiscall" fn(
            *mut c_void,
            *const MsvcWstring,
            *const MsvcFunction,
            *const MsvcFunction,
        )
    );
    let token = sso_wstring("tok");
    let mut ok_callback = make_function(&HEAP_VTABLE, 4);
    let mut err_callback = make_function(&HEAP_VTABLE, 5);
    checked_call(
        "ICrossPlayService[4] StartSession",
        &mut checks,
        || unsafe { start_session(service, &token, &ok_callback, &err_callback) },
    );

    let tick = slot!(vt, 57, Void1);
    let do_calls_before = DO_CALLS.load(Ordering::SeqCst);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while DO_CALLS.load(Ordering::SeqCst) == do_calls_before {
        checked_call("ICrossPlayService[57] Tick", &mut checks, || unsafe {
            tick(service)
        });
        require(
            std::time::Instant::now() < deadline,
            "configured RPC StartSession did not complete",
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    require(
        DO_CALLS.load(Ordering::SeqCst) > do_calls_before,
        "Tick did not reach a retained std::function through _Do_call",
    );
    require(
        LAST_DO_CALL_ARG.load(Ordering::SeqCst) != 0,
        "_Do_call received a null argument",
    );
    // SAFETY: both are by-reference parameters this process still owns.
    unsafe {
        release_function(&mut ok_callback);
        release_function(&mut err_callback);
    }

    let after_session = checked_call(
        "ICrossPlayService[7] GetSessionStatus",
        &mut checks,
        || unsafe { get_session_status(service) },
    );
    findings.push(format!(
        "session_status_before={before_session} after={after_session}"
    ));

    let get_crossplay_status = slot!(vt, 8, unsafe extern "thiscall" fn(*mut c_void) -> i32);
    let crossplay_status = checked_call(
        "ICrossPlayService[8] GetCrossplayStatus",
        &mut checks,
        || unsafe { get_crossplay_status(service) },
    );
    findings.push(format!("crossplay_status={crossplay_status}"));

    let block_user = slot!(vt, 9, WstringArg);
    let unblock_user = slot!(vt, 10, WstringArg);
    let is_user_blocked = slot!(
        vt,
        11,
        unsafe extern "thiscall" fn(*mut c_void, *const MsvcWstring) -> bool
    );
    let who = sso_wstring("peer");
    checked_call("ICrossPlayService[9] BlockUser", &mut checks, || unsafe {
        block_user(service, &who)
    });
    checked_call(
        "ICrossPlayService[10] UnblockUser",
        &mut checks,
        || unsafe { unblock_user(service, &who) },
    );
    let blocked = checked_call(
        "ICrossPlayService[11] IsUserBlocked",
        &mut checks,
        || unsafe { is_user_blocked(service, &who) },
    );
    require(
        !blocked,
        "IsUserBlocked is a measured constant false in the shipped build",
    );

    // slot 22 `StartGame(const wstring&, bool, std::function, std::function)` —
    // the by-value pair, on the service rather than the logger.
    let start_game = slot!(
        vt,
        22,
        unsafe extern "thiscall" fn(
            *mut c_void,
            *const MsvcWstring,
            bool,
            MsvcFunction,
            MsvcFunction,
        )
    );
    let game_baseline = counters();
    let unknown_lobby = sso_wstring("nolobby");
    let game_ok = make_function(&HEAP_VTABLE, 6);
    let game_err = make_function(&HEAP_VTABLE, 7);
    checked_call("ICrossPlayService[22] StartGame", &mut checks, || unsafe {
        start_game(service, &unknown_lobby, false, game_ok, game_err)
    });
    let after_start_game = counters();
    require(
        after_start_game.copies == game_baseline.copies + 2
            && after_start_game.deallocating == game_baseline.deallocating + 2,
        "StartGame did not take and release both by-value callbacks",
    );

    let before_answer = counters();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while counters().live != before_answer.live - 2 {
        checked_call(
            "ICrossPlayService[57] Tick (answer StartGame)",
            &mut checks,
            || unsafe { tick(service) },
        );
        require(
            std::time::Instant::now() < deadline,
            "configured RPC StartGame refusal did not complete",
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    require(
        counters().live == before_answer.live - 2,
        "answering a request did not release its two retained callbacks",
    );

    // slot 34 `JoinChat` is one of the ten fail-closed slots.
    let join_chat = slot!(
        vt,
        34,
        unsafe extern "thiscall" fn(*mut c_void, *const MsvcWstring, MsvcFunction, MsvcFunction)
    );
    let chat_ok = make_function(&HEAP_VTABLE, 8);
    let chat_err = make_function(&HEAP_VTABLE, 9);
    let chat_baseline = counters();
    checked_call("ICrossPlayService[34] JoinChat", &mut checks, || unsafe {
        join_chat(service, &who, chat_ok, chat_err)
    });
    require(
        counters().deallocating >= chat_baseline.deallocating + 2,
        "a refused slot leaked its by-value callbacks",
    );
    let refusals_before = DO_CALLS.load(Ordering::SeqCst);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while DO_CALLS.load(Ordering::SeqCst) == refusals_before {
        checked_call(
            "ICrossPlayService[57] Tick (answer JoinChat)",
            &mut checks,
            || unsafe { tick(service) },
        );
        require(
            std::time::Instant::now() < deadline,
            "JoinChat refusal did not complete",
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    require(
        DO_CALLS.load(Ordering::SeqCst) > refusals_before,
        "a refused slot did not answer on the caller's error callback",
    );

    let set_username = slot!(vt, 55, WstringArg);
    let name = sso_wstring("smoke");
    checked_call(
        "ICrossPlayService[55] SetUsername",
        &mut checks,
        || unsafe { set_username(service, &name) },
    );

    let get_player_guid = slot!(
        vt,
        56,
        unsafe extern "thiscall" fn(*mut c_void) -> *const MsvcWstring
    );
    let guid = checked_call(
        "ICrossPlayService[56] GetPlayerGuid",
        &mut checks,
        || unsafe { get_player_guid(service) },
    );
    require(!guid.is_null(), "GetPlayerGuid returned null");
    // SAFETY: a 24-byte `wstring` the service keeps alive for its own lifetime.
    let guid_size = unsafe {
        u32::from_le_bytes(
            (&(*guid).raw)[WSTRING_SIZE_OFFSET..WSTRING_SIZE_OFFSET + 4]
                .try_into()
                .unwrap(),
        )
    };
    require(guid_size > 0, "GetPlayerGuid returned an empty string");

    // slot 33 `GetInvitationId(wstring* return)` — MSVC's hidden return object.
    let get_invitation_id = slot!(
        vt,
        33,
        unsafe extern "thiscall" fn(*mut c_void, *mut MsvcWstring) -> *mut MsvcWstring
    );
    let mut invitation = MsvcWstring { raw: [0xAA; 24] };
    let returned = checked_call(
        "ICrossPlayService[33] GetInvitationId",
        &mut checks,
        || unsafe { get_invitation_id(service, &mut invitation) },
    );
    require(
        returned == &mut invitation as *mut MsvcWstring,
        "GetInvitationId did not return its hidden return object",
    );

    let is_connected_to_hub = slot!(vt, 53, unsafe extern "thiscall" fn(*mut c_void) -> bool);
    let connected = checked_call(
        "ICrossPlayService[53] IsConnectedToHub",
        &mut checks,
        || unsafe { is_connected_to_hub(service) },
    );
    findings.push(format!("is_connected_to_hub={connected}"));

    // slot 54 `CreateLocalPlayerLoopback` is `ret 0` in the shipped build.
    // **[measured — RVA 0x1e350]**
    let loopback = slot!(vt, 54, Void1);
    checked_call(
        "ICrossPlayService[54] CreateLocalPlayerLoopback",
        &mut checks,
        || unsafe { loopback(service) },
    );

    let cancel_pending = slot!(vt, 21, Void1);
    checked_call(
        "ICrossPlayService[21] LobbyCancelPendingRequests",
        &mut checks,
        || unsafe { cancel_pending(service) },
    );

    let stop_session = slot!(vt, 5, Void1);
    checked_call("ICrossPlayService[5] StopSession", &mut checks, || unsafe {
        stop_session(service)
    });
    checked_call(
        "ICrossPlayService[5] StopSession (idempotent)",
        &mut checks,
        || unsafe { stop_session(service) },
    );
    checked_call(
        "ICrossPlayService[57] Tick (drain)",
        &mut checks,
        || unsafe { tick(service) },
    );

    let shutdown =
        unsafe { core::mem::transmute::<*mut c_void, unsafe extern "C" fn()>(shutdown_export) };
    checked_call("don_crossplay_shutdown", &mut checks, || unsafe {
        shutdown()
    });
    let final_counters = counters();
    require(
        final_counters.live == 0,
        "shutdown retained one or more callback targets",
    );
    let unloaded = checked_call("FreeLibrary", &mut checks, || unsafe {
        FreeLibrary(module)
    });
    require(unloaded != 0, "FreeLibrary failed after directory shutdown");
    println!(
        "{{\"schema\":\"don.crossplay-load-smoke.v1\",\"status\":\"pass\",\
\"pe\":\"PE32-i386\",\"dll\":{dll:?},\
\"service_vtable_slots\":{service_slots},\"logger_vtable_slots\":{logger_slots},\
\"null_slots\":{null_slots},\"stack_pointer_checks\":{checks},\
\"func_copy_calls\":{copies},\"func_move_calls\":{moves},\
\"func_delete_this_calls\":{deletes},\"func_delete_this_deallocating\":{deallocating},\
\"func_delete_this_in_place\":{in_place},\"func_targets_outstanding\":{live},\
\"func_do_call_invocations\":{do_calls},\"findings\":{findings:?},\
\"configured_rpc\":true,\"shutdown_before_free_library\":true,\"free_library\":true,\
\"retail_process_modified\":false,\"game_directory_modified\":false}}",
        dll = dll.display().to_string(),
        service_slots = SERVICE_SLOTS,
        logger_slots = LOGGER_SLOTS,
        null_slots = null_slots,
        checks = checks,
        copies = final_counters.copies,
        moves = final_counters.moves,
        deletes = final_counters.deletes,
        deallocating = final_counters.deallocating,
        in_place = final_counters.in_place,
        live = final_counters.live,
        do_calls = DO_CALLS.load(Ordering::SeqCst),
        findings = findings,
    );
}
