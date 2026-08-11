// SPDX-License-Identifier: GPL-3.0-or-later
//! A DoN-owned object that presents the shipped 4-slot
//! `Crossplay::Logging::ICrossplayLogger` vtable — export ordinal 1.
//!
//! # Why this exists separately from [`crate::service`]
//!
//! `riseofnations.exe` reaches `CrossplayProxy.dll` through two imported free
//! functions, and **ordinal 1 is not optional**. Fifteen call sites do
//! `call dword ptr [0xac5038]` and immediately dereference the result:
//!
//! ```text
//! 004ff326  call dword ptr [0xac5038]   ; Crossplay::Logging::Logger()
//! 004ff330  mov  ecx, eax               ; no null check
//! 004ff332  call 0x004fef40             ; rise.exe's own Log wrapper
//! ```
//!
//! and that wrapper ends in `call dword ptr [eax + 0x0c]` — vtable slot 3,
//! `Log` — after building a 24-byte `wstring` in place on the stack
//! (`sub esp, 0x18`; `_Myres = 7`; `_Mysize = 0`; `_Buf[0] = 0`) and pushing
//! `file` then `line`. `0x004fef40` pushes level `8` (`Error`) and
//! `0x004fee90` pushes level `2` (`Info`). One further site,
//! `0x0056143d`, builds a 40-byte `std::function` on the stack and calls
//! `[eax + 0x04]` — slot 1, `Register` — with level `2`. **[measured]**
//!
//! So a replacement DLL must return a real object here before the service is
//! ever asked for, and it must honour the two by-value parameters those two
//! slots take. Returning null crashes the game in its own logging wrapper.
//!
//! # What this one does
//!
//! Keeps a bounded ring of the lines retail emitted and, if the host installed
//! one, hands each to a [`LogSink`]. That is **[DoN policy]**: nothing was
//! measured about where Q-LOC's logger writes, and this file does not pretend
//! otherwise. What *is* reproduced from measurement is the ownership discipline
//! on the two by-value arguments — see [`crate::func`].

use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_char;

use crate::abi::{ICrossplayLogger, ICrossplayLoggerVtable, LogLevel, MsvcFunction, MsvcWstring};
use crate::func::{self, Destroyed, OwnedFunction};

/// Largest number of `Register` callbacks retained. Bounded so a caller that
/// registers in a loop cannot grow this object without limit. **[DoN policy]**
pub const MAX_REGISTRATIONS: usize = 16;

/// Largest number of `Log` lines retained. **[DoN policy]**
pub const LOG_RING_CAPACITY: usize = 256;

/// Longest message decoded out of a by-value `wstring`. A retail log line is
/// far below this; the bound turns a corrupt `_Mysize` into a refusal.
/// **[DoN policy]**
pub const MAX_MESSAGE_UNITS: usize = 4096;

/// Where a decoded line goes, if the host wants it live rather than at the end.
///
/// A plain `fn` pointer, not a closure: this crate is `alloc`-only and the
/// logger is a process-wide singleton, so there is no owner for captured state.
pub type LogSink = fn(level: LogLevel, message: &str, file: Option<&str>, line: i32);

/// One line retail logged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogRecord {
    pub level: LogLevel,
    pub message: String,
    pub file: Option<String>,
    pub line: i32,
    /// What became of the by-value `wstring` that carried the message.
    pub message_storage: MessageStorage,
}

/// Where the by-value `wstring` argument kept its characters, and what this
/// code did about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageStorage {
    /// `_Myres < 8`: the characters were in the object's own 16-byte buffer and
    /// nothing had to be released. **[measured — `_Find_last` at
    /// `CrossplayProxy.dll` RVA `0x35c0` is `cmp dword ptr [ebx+0x1c], 8`]**
    Inline,
    /// `_Myres >= 8`: a heap buffer, released on the shared UCRT heap.
    HeapReleased,
    /// A heap buffer that was left alone because its address is on this
    /// thread's stack, which no allocator owns. See [`crate::func`].
    HeapLeaked,
    /// `_Mysize`/`_Myres` did not describe a readable string.
    Unreadable,
}

/// One retained `Register` callback.
///
/// The `std::function` is behind [`OwnedFunction`] because pushing this record
/// into a `Vec` moves it, and an inline target is a pointer into the object's
/// own buffer. See [`crate::func`].
pub struct Registration {
    pub level: LogLevel,
    callback: OwnedFunction,
}

impl Registration {
    /// Whether a target survived the `_Copy`.
    pub fn installed(&self) -> bool {
        self.callback.installed()
    }

    /// The `_Func_base` to dispatch through, or `0`.
    pub fn target(&self) -> u32 {
        self.callback.target()
    }
}

/// Counters and the retained state behind [`LocalLogger`].
#[derive(Default)]
pub struct LoggerState {
    /// Every `Register` call that arrived, retained or not.
    pub register_calls: u32,
    /// `Register` calls refused because [`MAX_REGISTRATIONS`] was already full.
    /// The argument is still destroyed; only the copy is skipped.
    pub register_refused: u32,
    /// `UnregisterAll` calls.
    pub unregister_calls: u32,
    /// `Log` calls.
    pub log_calls: u32,
    /// Deleting-destructor calls. Retail's own logger is a function-local
    /// static, so this is expected to stay zero.
    pub destructor_calls: u32,
    /// Outcome tally of every by-value `std::function` this logger destroyed
    /// without retaining it, in the order they happened. Bounded by
    /// `register_refused`.
    pub destructions: Vec<Destroyed>,
    registrations: Vec<Registration>,
    lines: Vec<LogRecord>,
    sink: Option<LogSink>,
}

impl LoggerState {
    /// The retained `Register` callbacks.
    pub fn registrations(&self) -> &[Registration] {
        &self.registrations
    }

    /// The retained log lines, oldest first.
    pub fn lines(&self) -> &[LogRecord] {
        &self.lines
    }

    /// Install the live sink. Replaces any previous one.
    pub fn set_sink(&mut self, sink: LogSink) {
        self.sink = Some(sink);
    }
}

/// `Crossplay::Logging::ICrossplayLogger`, DoN-side.
///
/// `#[repr(C)]` with the vtable pointer first is the whole of what retail knows
/// about this object.
#[repr(C)]
pub struct LocalLogger {
    /// **Must stay first.**
    #[allow(dead_code)]
    vftable: *const ICrossplayLoggerVtable,
    state: LoggerState,
}

impl LocalLogger {
    /// The vtable this object publishes.
    pub fn vtable() -> &'static ICrossplayLoggerVtable {
        &thunks::VTABLE
    }

    /// A fresh logger.
    pub fn new() -> alloc::boxed::Box<Self> {
        alloc::boxed::Box::new(Self {
            vftable: Self::vtable(),
            state: LoggerState::default(),
        })
    }

    /// The pointer `Crossplay::Logging::Logger()` would return.
    pub fn as_logger(&self) -> *mut ICrossplayLogger {
        self as *const Self as *mut ICrossplayLogger
    }

    pub fn state(&self) -> &LoggerState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut LoggerState {
        &mut self.state
    }

    /// # Safety
    ///
    /// `this` must be a pointer this module produced.
    unsafe fn of<'a>(this: *mut ICrossplayLogger) -> Option<&'a mut Self> {
        if this.is_null() {
            return None;
        }
        // SAFETY: the vtable pointer is at offset 0 by `repr(C)`, so the two
        // pointers have the same address; the caller guarantees provenance.
        Some(unsafe { &mut *(this as *mut Self) })
    }
}

// ---------------------------------------------------------------------------
// The by-value `wstring` argument.
// ---------------------------------------------------------------------------

/// MSVC x86 `std::basic_string<wchar_t>` field offsets. **[measured]**
const WSTRING_SIZE_OFFSET: usize = 16;
const WSTRING_CAPACITY_OFFSET: usize = 20;
/// `_Myres < 8` means the characters are in `_Buf`. **[measured]**
const WSTRING_INLINE_CAPACITY: u32 = 7;

fn wstring_field(value: &MsvcWstring, offset: usize) -> u32 {
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&value.raw[offset..offset + 4]);
    u32::from_le_bytes(bytes)
}

fn decode_units(units: &[u16]) -> String {
    let mut out = String::new();
    for unit in char::decode_utf16(units.iter().copied()) {
        out.push(unit.unwrap_or(char::REPLACEMENT_CHARACTER));
    }
    out
}

/// Decode a by-value `wstring` and release whatever it owned.
///
/// The object is the callee's from the call instruction onward — see
/// [`crate::func`] — so this both reads it and destroys it. Only the heap case
/// has anything to release; MSVC's own destructor frees `_Ptr` exactly when
/// `_Myres >= 8`.
///
/// # Safety
///
/// `value` must be the caller's by-value argument, at its ABI address.
unsafe fn take_wstring(value: &mut MsvcWstring) -> (String, MessageStorage) {
    let size = wstring_field(value, WSTRING_SIZE_OFFSET) as usize;
    let capacity = wstring_field(value, WSTRING_CAPACITY_OFFSET);
    if size > MAX_MESSAGE_UNITS || (capacity as usize) < size {
        return (String::new(), MessageStorage::Unreadable);
    }
    if capacity <= WSTRING_INLINE_CAPACITY {
        let mut units = [0u16; 8];
        for (index, unit) in units.iter_mut().enumerate() {
            let at = index * 2;
            *unit = u16::from_le_bytes([value.raw[at], value.raw[at + 1]]);
        }
        return (decode_units(&units[..size]), MessageStorage::Inline);
    }
    #[cfg(target_pointer_width = "32")]
    {
        let pointer = wstring_field(value, 0);
        if pointer == 0 {
            return (String::new(), MessageStorage::Unreadable);
        }
        // SAFETY: `_Myres >= 8` means `_Bx._Ptr` is live and holds at least
        // `_Mysize` code units. **[measured]**
        let text = unsafe {
            decode_units(core::slice::from_raw_parts(
                pointer as usize as *const u16,
                size,
            ))
        };
        if func::on_current_stack(pointer) {
            return (text, MessageStorage::HeapLeaked);
        }
        // SAFETY: the buffer came from `operator new` in a module sharing this
        // process's single UCRT heap. **[measured — every shipped image imports
        // `api-ms-win-crt-heap-l1-1-0.dll`]**
        unsafe { crt_free(pointer as usize as *mut core::ffi::c_void) };
        (text, MessageStorage::HeapReleased)
    }
    #[cfg(not(target_pointer_width = "32"))]
    {
        // A 32-bit `_Ptr` is not an address on this host, and the tests only
        // ever build inline strings, so there is nothing to read or release.
        (String::new(), MessageStorage::Unreadable)
    }
}

#[cfg(target_pointer_width = "32")]
extern "C" {
    #[link_name = "free"]
    fn crt_free(ptr: *mut core::ffi::c_void);
}

// ---------------------------------------------------------------------------
// The four slots.
// ---------------------------------------------------------------------------

/// Both ABIs from one body: `extern "thiscall"` is the shipped one and only
/// exists on x86, `extern "C"` is what the host tests drive the same table
/// through. Same idiom as [`crate::service`].
macro_rules! logger_impl {
    ($abi:literal) => {
        use super::*;

        /// Slot 0. The logger is a process-wide singleton, mirroring retail's
        /// function-local static at `CrossplayProxy.dll` RVA `0x12180`
        /// (`_Init_thread_header` at `0x100c3a7c`), so this **never frees**.
        /// It records the call and hands the pointer back. **[DoN policy, and
        /// the reason retail is expected never to call it]**
        pub unsafe extern $abi fn destructor(
            this: *mut ICrossplayLogger,
            _flags: u32,
        ) -> *mut core::ffi::c_void {
            // SAFETY: `this` came from this module.
            if let Some(logger) = unsafe { LocalLogger::of(this) } {
                logger.state.destructor_calls += 1;
            }
            this.cast()
        }

        /// Slot 1. `void Register(LogLevel, std::function<...>)` — the
        /// `std::function` arrives **by value** and is this callee's to destroy.
        /// **[measured — `rise.exe` `0x0056147c` releases its unwind claim on
        /// the object one instruction before the call]**
        pub unsafe extern $abi fn register(
            this: *mut ICrossplayLogger,
            level: LogLevel,
            mut callback: MsvcFunction,
        ) {
            // SAFETY: `this` came from this module.
            let Some(logger) = (unsafe { LocalLogger::of(this) }) else {
                // Still the callee's to destroy, even with nowhere to put it.
                // SAFETY: a by-value parameter at its ABI address.
                unsafe { func::destroy(&mut callback) };
                return;
            };
            logger.state.register_calls += 1;
            if logger.state.registrations.len() >= MAX_REGISTRATIONS {
                logger.state.register_refused += 1;
                // SAFETY: a by-value parameter at its ABI address.
                let outcome = unsafe { func::destroy(&mut callback) };
                logger.state.destructions.push(outcome);
                return;
            }
            // SAFETY: `callback` is the caller's by-value argument, which
            // `adopt` copies out of and then destroys.
            let retained = unsafe { OwnedFunction::adopt(&mut callback) };
            logger.state.registrations.push(Registration {
                level,
                callback: retained,
            });
        }

        /// Slot 2. `void UnregisterAll()`. Destroys every copy this object
        /// took — the other half of the ownership cycle.
        pub unsafe extern $abi fn unregister_all(this: *mut ICrossplayLogger) {
            // SAFETY: `this` came from this module.
            let Some(logger) = (unsafe { LocalLogger::of(this) }) else {
                return;
            };
            logger.state.unregister_calls += 1;
            // Dropping each `OwnedFunction` runs the measured `_Delete_this`
            // on the copy this object took.
            logger.state.registrations.clear();
        }

        /// Slot 3. `void Log(LogLevel, wstring, const char*, int)` — the
        /// `wstring` arrives **by value**, 24 bytes on the stack, and is this
        /// callee's to destroy. `file` is a plain literal pointer into the
        /// caller's image and is **not** owned. **[measured — `rise.exe`
        /// `0x004fef74`..`0x004fefa9`]**
        pub unsafe extern $abi fn log(
            this: *mut ICrossplayLogger,
            level: LogLevel,
            mut message: MsvcWstring,
            file: *const c_char,
            line: i32,
        ) {
            // SAFETY: a by-value parameter at its ABI address.
            let (text, storage) = unsafe { take_wstring(&mut message) };
            // SAFETY: `this` came from this module.
            let Some(logger) = (unsafe { LocalLogger::of(this) }) else {
                return;
            };
            logger.state.log_calls += 1;
            // SAFETY: retail passes a NUL-terminated `__FILE__` literal living
            // in its own read-only data, or null.
            let file_text = unsafe { read_c_string(file) };
            if let Some(sink) = logger.state.sink {
                sink(level, &text, file_text.as_deref(), line);
            }
            if logger.state.lines.len() >= LOG_RING_CAPACITY {
                logger.state.lines.remove(0);
            }
            logger.state.lines.push(LogRecord {
                level,
                message: text,
                file: file_text,
                line,
                message_storage: storage,
            });
        }

        pub static VTABLE: ICrossplayLoggerVtable = ICrossplayLoggerVtable {
            vector_deleting_destructor: destructor,
            Register: register,
            UnregisterAll: unregister_all,
            Log: log,
        };
    };
}

/// Read a NUL-terminated byte string, bounded.
///
/// # Safety
///
/// `pointer`, when non-null, must be a NUL-terminated C string.
unsafe fn read_c_string(pointer: *const c_char) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    let mut bytes = Vec::new();
    for index in 0..MAX_MESSAGE_UNITS {
        // SAFETY: the caller's guarantee, bounded by `MAX_MESSAGE_UNITS`.
        let byte = unsafe { *pointer.add(index) } as u8;
        if byte == 0 {
            break;
        }
        bytes.push(byte);
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(target_arch = "x86")]
mod thunks {
    logger_impl!("thiscall");
}
#[cfg(not(target_arch = "x86"))]
mod thunks {
    logger_impl!("C");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::{LOG_LEVEL_ERROR, LOG_LEVEL_INFO};

    fn wstring_inline(text: &str) -> MsvcWstring {
        let units: Vec<u16> = text.encode_utf16().collect();
        assert!(units.len() <= 7, "test helper builds SSO strings only");
        let mut raw = [0u8; 24];
        for (index, unit) in units.iter().enumerate() {
            raw[index * 2..index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
        }
        raw[WSTRING_SIZE_OFFSET..WSTRING_SIZE_OFFSET + 4]
            .copy_from_slice(&(units.len() as u32).to_le_bytes());
        raw[WSTRING_CAPACITY_OFFSET..WSTRING_CAPACITY_OFFSET + 4]
            .copy_from_slice(&WSTRING_INLINE_CAPACITY.to_le_bytes());
        MsvcWstring { raw }
    }

    #[test]
    fn the_vtable_is_four_slots_and_the_object_leads_with_it() {
        assert_eq!(
            core::mem::size_of::<ICrossplayLoggerVtable>(),
            4 * core::mem::size_of::<*const ()>()
        );
        let logger = LocalLogger::new();
        assert_eq!(
            logger.as_logger() as usize,
            &*logger as *const LocalLogger as usize
        );
    }

    #[test]
    fn log_decodes_an_inline_message_and_records_it() {
        let mut logger = LocalLogger::new();
        let this = logger.as_logger();
        let file = b"crossplay.cpp\0";
        // SAFETY: `this` is this module's object; the arguments match slot 3.
        unsafe {
            (LocalLogger::vtable().Log)(
                this,
                LOG_LEVEL_INFO,
                wstring_inline("hello"),
                file.as_ptr().cast(),
                190,
            );
        }
        let state = logger.state_mut();
        assert_eq!(state.log_calls, 1);
        assert_eq!(state.lines().len(), 1);
        let record = &state.lines()[0];
        assert_eq!(record.message, "hello");
        assert_eq!(record.file.as_deref(), Some("crossplay.cpp"));
        assert_eq!(record.line, 190);
        assert_eq!(record.level, LOG_LEVEL_INFO);
        assert_eq!(record.message_storage, MessageStorage::Inline);
    }

    #[test]
    fn a_null_file_pointer_is_recorded_as_absent() {
        let logger = LocalLogger::new();
        let this = logger.as_logger();
        // SAFETY: as above, with the documented null `file`.
        unsafe {
            (LocalLogger::vtable().Log)(
                this,
                LOG_LEVEL_ERROR,
                wstring_inline("x"),
                core::ptr::null(),
                0,
            );
        }
        assert_eq!(logger.state().lines()[0].file, None);
    }

    #[test]
    fn the_line_ring_is_bounded() {
        let logger = LocalLogger::new();
        let this = logger.as_logger();
        for _ in 0..(LOG_RING_CAPACITY + 5) {
            // SAFETY: as above.
            unsafe {
                (LocalLogger::vtable().Log)(
                    this,
                    LOG_LEVEL_INFO,
                    wstring_inline("m"),
                    core::ptr::null(),
                    1,
                );
            }
        }
        assert_eq!(logger.state().lines().len(), LOG_RING_CAPACITY);
        assert_eq!(logger.state().log_calls as usize, LOG_RING_CAPACITY + 5);
    }

    #[test]
    fn register_retains_and_unregister_all_releases() {
        let logger = LocalLogger::new();
        let this = logger.as_logger();
        // A `std::function` with no target: the only kind a host test can build,
        // because a real one needs guest code behind `_Copy`.
        // SAFETY: `this` is this module's object; the argument matches slot 1.
        unsafe { (LocalLogger::vtable().Register)(this, LOG_LEVEL_INFO, func::empty()) };
        assert_eq!(logger.state().register_calls, 1);
        assert_eq!(logger.state().registrations().len(), 1);
        assert_eq!(logger.state().registrations()[0].level, LOG_LEVEL_INFO);
        assert!(!logger.state().registrations()[0].installed());
        // SAFETY: as above.
        unsafe { (LocalLogger::vtable().UnregisterAll)(this) };
        assert_eq!(logger.state().unregister_calls, 1);
        assert!(logger.state().registrations().is_empty());
    }

    #[test]
    fn registrations_are_bounded_and_the_argument_is_still_consumed() {
        let logger = LocalLogger::new();
        let this = logger.as_logger();
        for _ in 0..(MAX_REGISTRATIONS + 3) {
            // SAFETY: as above.
            unsafe { (LocalLogger::vtable().Register)(this, LOG_LEVEL_INFO, func::empty()) };
        }
        assert_eq!(
            logger.state().register_calls as usize,
            MAX_REGISTRATIONS + 3
        );
        assert_eq!(logger.state().register_refused, 3);
        assert_eq!(logger.state().registrations().len(), MAX_REGISTRATIONS);
    }

    #[test]
    fn the_destructor_never_frees_the_singleton() {
        let logger = LocalLogger::new();
        let this = logger.as_logger();
        // SAFETY: as above; slot 0 takes the `__vecDelDtor` flags word.
        let returned = unsafe { (LocalLogger::vtable().vector_deleting_destructor)(this, 1) };
        assert_eq!(returned as usize, this as usize);
        assert_eq!(logger.state().destructor_calls, 1);
        // Still usable: nothing was released.
        // SAFETY: as above.
        unsafe {
            (LocalLogger::vtable().Log)(
                this,
                LOG_LEVEL_INFO,
                wstring_inline("after"),
                core::ptr::null(),
                2,
            );
        }
        assert_eq!(logger.state().lines().len(), 1);
    }

    #[test]
    fn a_corrupt_size_is_refused_rather_than_read() {
        let logger = LocalLogger::new();
        let this = logger.as_logger();
        let mut raw = [0u8; 24];
        raw[WSTRING_SIZE_OFFSET..WSTRING_SIZE_OFFSET + 4]
            .copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        raw[WSTRING_CAPACITY_OFFSET..WSTRING_CAPACITY_OFFSET + 4]
            .copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        // SAFETY: as above.
        unsafe {
            (LocalLogger::vtable().Log)(
                this,
                LOG_LEVEL_INFO,
                MsvcWstring { raw },
                core::ptr::null(),
                3,
            );
        }
        assert_eq!(
            logger.state().lines()[0].message_storage,
            MessageStorage::Unreadable
        );
        assert_eq!(logger.state().lines()[0].message, "");
    }

    #[test]
    fn a_sink_sees_every_line() {
        use core::sync::atomic::{AtomicU32, Ordering};
        static SEEN: AtomicU32 = AtomicU32::new(0);
        fn sink(_level: LogLevel, message: &str, _file: Option<&str>, _line: i32) {
            if message == "sunk" {
                SEEN.fetch_add(1, Ordering::Relaxed);
            }
        }
        let mut logger = LocalLogger::new();
        logger.state_mut().set_sink(sink);
        let this = logger.as_logger();
        // SAFETY: as above.
        unsafe {
            (LocalLogger::vtable().Log)(
                this,
                LOG_LEVEL_INFO,
                wstring_inline("sunk"),
                core::ptr::null(),
                4,
            );
        }
        assert_eq!(SEEN.load(Ordering::Relaxed), 1);
    }
}
