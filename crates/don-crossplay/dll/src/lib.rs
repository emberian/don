// SPDX-License-Identifier: GPL-3.0-or-later
//! `CrossplayProxy.dll` — a drop-in replacement image.
//!
//! [`don_crossplay`] is the checked ABI plus a DoN-owned service behind it.
//! This crate is the *image*: a PE32 i386 DLL exporting the four names the
//! shipped `CrossplayProxy.dll` exports, at the same ordinals, with the same
//! two marked as data.
//!
//! ```text
//! ord 1  ?Logger@Logging@Crossplay@@YAPAVICrossplayLogger@12@XZ   code, RVA 0x12180
//! ord 2  ?Service@Crossplay@@YAPAUICrossPlayService@1@XZ          code, RVA 0x130e0
//! ord 3  AmdPowerXpressRequestHighPerformance                     data, RVA 0xb2048 = 1
//! ord 4  NvOptimusEnablement                                      data, RVA 0xb2044 = 1
//! ```
//!
//! `riseofnations.exe` imports ordinals 1 and 2 by name — 15 and 75 call sites
//! respectively — and never resolves anything here with `GetProcAddress`.
//! **[measured — import directory of `ron-bin/riseofnations.exe`, and a `.text`
//! scan for `call dword ptr [0xac5038]` / `[0xac503c]`]**
//!
//! # Honest status
//!
//! Built, export-checked, and executed in a disposable PE32 process. **Never
//! loaded by `riseofnations.exe`.** Nothing has been installed into a game
//! directory and no live process was read or modified by the lane that wrote
//! this. See `docs/tracks/crossplay-proxy-dll.md`.
//!
//! # Configuration
//!
//! There is no UI to hang settings off, so they come from the environment, the
//! same way `netsys-shim` does it.
//!
//! | variable | meaning | default |
//! |---|---|---|
//! | `DON_CROSSPLAY_USER_ID` | the local user id the service reports | `don-<pid>` |
//! | `DON_CROSSPLAY_USER_NAME` | the local display name | `don` |
//! | `DON_CROSSPLAY_TRACE` | path of a diagnostic log; setting it enables tracing | unset, tracing off |
//!
//! There is deliberately **no load-only mode**. `netsys-shim` needs one because
//! it owns a socket; this service has no socket, no file, no account and no
//! remote host to fail closed against, so a flag named for that would be
//! decoration.
//!
//! # Threading
//!
//! Both objects are process-wide singletons behind a `OnceLock`, mirroring the
//! shipped DLL: `Logger` and `Service` are both function-local statics, guarded
//! by `_Init_thread_header` on `0x100c3a7c` and `0x100c3a84`. **[measured]**
//! Initialisation is therefore race-free, but the objects themselves are not
//! internally synchronised — no more than the shipped ones are — and the
//! service is pumped from the thread that calls `Tick`.

#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use std::cell::UnsafeCell;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use don_crossplay::abi::{ICrossPlayService, ICrossplayLogger, LogLevel};
use don_crossplay::logger::LocalLogger;
use don_crossplay::msvc::RawMem;
use don_crossplay::LocalCrossPlayService;

// ---------------------------------------------------------------------------
// Ordinals 3 and 4 — the hybrid-GPU hints. Data, not code.
// ---------------------------------------------------------------------------

/// A `DWORD` in a writable section, which is what a vendor driver expects to
/// find when it walks a process's export tables for these two names.
///
/// The `UnsafeCell` is what puts it in `.data` rather than `.rdata`; the
/// shipped DLL keeps both at `.data` RVA `0xb2044`/`0xb2048`. Nothing in this
/// crate ever writes them. **[measured]**
#[repr(transparent)]
struct GpuHint(UnsafeCell<u32>);

// SAFETY: a plain `u32` that this crate never writes and only the loader and
// the graphics driver ever read.
unsafe impl Sync for GpuHint {}

/// `AmdPowerXpressRequestHighPerformance = 1`. **[measured — shipped value at
/// RVA `0xb2048` is `1`]**
#[no_mangle]
#[used]
static proxy_amd_power_xpress_request_high_performance: GpuHint = GpuHint(UnsafeCell::new(1));

/// `NvOptimusEnablement = 1`. **[measured — shipped value at RVA `0xb2044` is
/// `1`]**
#[no_mangle]
#[used]
static proxy_nv_optimus_enablement: GpuHint = GpuHint(UnsafeCell::new(1));

// ---------------------------------------------------------------------------
// Diagnostics.
// ---------------------------------------------------------------------------

static TRACE: OnceLock<Option<Mutex<TraceSink>>> = OnceLock::new();
static TRACE_SEQ: AtomicU32 = AtomicU32::new(0);

struct TraceSink {
    path: std::path::PathBuf,
}

impl TraceSink {
    fn write(&mut self, line: &str) {
        let sequence = TRACE_SEQ.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = writeln!(file, "seq={sequence} {line}");
            let _ = file.flush();
        }
    }
}

fn trace_sink() -> Option<&'static Mutex<TraceSink>> {
    TRACE
        .get_or_init(|| {
            let path = std::env::var_os("DON_CROSSPLAY_TRACE")?;
            Some(Mutex::new(TraceSink { path: path.into() }))
        })
        .as_ref()
}

/// Record one diagnostic line, if `DON_CROSSPLAY_TRACE` named a file.
///
/// Every line is flushed synchronously: the point of this log is to survive
/// whatever happens next.
pub fn trace(line: &str) {
    if let Some(sink) = trace_sink() {
        if let Ok(mut sink) = sink.lock() {
            sink.write(line);
        }
    }
}

/// Where a retail log line goes once [`LocalLogger`] has decoded it.
fn log_sink(level: LogLevel, message: &str, file: Option<&str>, line: i32) {
    let origin = file.unwrap_or("<none>");
    trace(&format!(
        "log level={level} file={origin} line={line} message={message:?}"
    ));
}

fn environment(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

// ---------------------------------------------------------------------------
// Ordinal 1 — `Crossplay::Logging::Logger()`.
// ---------------------------------------------------------------------------

static LOGGER: OnceLock<usize> = OnceLock::new();

/// `class Crossplay::Logging::ICrossplayLogger * __cdecl Crossplay::Logging::Logger(void)`
///
/// Retail dereferences the result with **no null check** — `mov ecx, eax` two
/// instructions after the call at `0x004ff326` — so this must always answer a
/// live object. **[measured]**
///
/// Exported under the decorated shipped name at ordinal 1 by `build.rs`.
#[no_mangle]
pub extern "C" fn proxy_logger() -> *mut ICrossplayLogger {
    let address = *LOGGER.get_or_init(|| {
        let mut logger = LocalLogger::new();
        logger.state_mut().set_sink(log_sink);
        let pointer = Box::leak(logger);
        trace("logger=constructed");
        pointer.as_logger() as usize
    });
    trace("export.Logger");
    address as *mut ICrossplayLogger
}

// ---------------------------------------------------------------------------
// Ordinal 2 — `Crossplay::Service()`.
// ---------------------------------------------------------------------------

static SERVICE: OnceLock<usize> = OnceLock::new();

/// `struct Crossplay::ICrossPlayService * __cdecl Crossplay::Service(void)`
///
/// The PDB name of the code behind this export is
/// `?Service@CrossplayProxy@@YAPAUICrossPlayService@1@XZ` at RVA `0x130e0`; the
/// export table publishes that address under the *game's* spelling. Both name
/// the same 58-slot layout in different namespaces. **[measured]**
///
/// Exported under the decorated shipped name at ordinal 2 by `build.rs`.
#[no_mangle]
pub extern "C" fn proxy_service() -> *mut ICrossPlayService {
    let address = *SERVICE.get_or_init(|| {
        let user_id = environment("DON_CROSSPLAY_USER_ID")
            .unwrap_or_else(|| format!("don-{}", std::process::id()));
        let user_name = environment("DON_CROSSPLAY_USER_NAME").unwrap_or_else(|| "don".to_owned());
        let service = LocalCrossPlayService::new(RawMem, &user_id, &user_name);
        let pointer = Box::leak(service);
        trace(&format!(
            "service=constructed user_id={user_id:?} user_name={user_name:?}"
        ));
        pointer.as_service() as usize
    });
    trace("export.Service");
    address as *mut ICrossPlayService
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gpu_hints_are_the_shipped_value() {
        // SAFETY: read-only access to a `u32` nothing in this crate writes.
        unsafe {
            assert_eq!(*proxy_nv_optimus_enablement.0.get(), 1);
            assert_eq!(
                *proxy_amd_power_xpress_request_high_performance.0.get(),
                1
            );
        }
    }

    #[test]
    fn the_two_factories_are_stable_singletons() {
        let first = proxy_logger();
        let second = proxy_logger();
        assert!(!first.is_null());
        assert_eq!(first, second);

        let first = proxy_service();
        let second = proxy_service();
        assert!(!first.is_null());
        assert_eq!(first, second);
    }
}
