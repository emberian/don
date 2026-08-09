//! Import-table surgery: give the retail image a Win32/UCRT it can actually run against.
//!
//! `riseofnations.exe` links the CRT **dynamically** (`api-ms-win-crt-*`), so every
//! `malloc`, `fopen`, `memcpy` and `printf` the script compiler uses goes through the IAT.
//! That is the whole reason this is tractable: we never have to emulate a Windows CRT
//! inside the image, only replace ~300 IAT slots with i686 Linux code.
//!
//! Two calling conventions are in play. UCRT/VCRUNTIME entries are `__cdecl`, which is
//! byte-for-byte Rust's `extern "C"` on i686, so those slots point straight at a Rust
//! function. KERNEL32 and friends are `__stdcall` (callee pops); rather than rely on
//! Rust's `extern "stdcall"` on a non-Windows target, each of those slots points at a
//! 20-byte generated adapter that calls a cdecl Rust function and then does the callee
//! cleanup itself.
//!
//! Anything we have not implemented gets a generated trap: it prints `dll!name` and the
//! retail return address, then exits. A run that dies in a trap is a *shopping list*,
//! which is the point — it converts "the compiler needs an environment" into an
//! enumerable set of missing functions.

use crate::image::Mapped;
use std::ffi::{c_char, c_int, c_void, CStr};
use std::io::Write;

#[derive(Clone)]
pub struct Import {
    pub dll: String,
    pub name: String,
    pub slot_va: u32,
    pub bound: Option<&'static str>,
}

pub fn parse_imports(m: &Mapped) -> Vec<Import> {
    let (dir_rva, _sz) = m.pe.data_directories[1];
    let mut out = Vec::new();
    let mut p = dir_rva + crate::image::IMAGE_BASE;
    loop {
        let orig_thunk = m.read_u32(p);
        let name_rva = m.read_u32(p + 12);
        let first_thunk = m.read_u32(p + 16);
        if orig_thunk == 0 && name_rva == 0 && first_thunk == 0 {
            break;
        }
        let dll = read_cstr(m, name_rva + crate::image::IMAGE_BASE);
        let int = if orig_thunk != 0 {
            orig_thunk
        } else {
            first_thunk
        };
        let mut i = 0u32;
        loop {
            let e = m.read_u32(int + crate::image::IMAGE_BASE + i * 4);
            if e == 0 {
                break;
            }
            let name = if e & 0x8000_0000 != 0 {
                format!("#{}", e & 0xffff)
            } else {
                read_cstr(m, e + crate::image::IMAGE_BASE + 2)
            };
            out.push(Import {
                dll: dll.clone(),
                name,
                slot_va: first_thunk + crate::image::IMAGE_BASE + i * 4,
                bound: None,
            });
            i += 1;
        }
        p += 20;
    }
    out
}

fn read_cstr(m: &Mapped, va: u32) -> String {
    let mut s = Vec::new();
    let mut q = va;
    loop {
        let b = unsafe { *m.at(q) };
        if b == 0 || s.len() > 512 {
            break;
        }
        s.push(b);
        q += 1;
    }
    String::from_utf8_lossy(&s).into_owned()
}

// ---------------------------------------------------------------------------------
// Generated code: stdcall adapters and traps
// ---------------------------------------------------------------------------------

struct CodeGen {
    base: *mut u8,
    len: usize,
    at: usize,
}

impl CodeGen {
    fn new(pages: usize) -> CodeGen {
        let len = pages * crate::image::PAGE;
        let p = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        assert!(p != libc::MAP_FAILED, "codegen mmap failed");
        assert!((p as usize) < u32::MAX as usize, "codegen above 4GiB");
        CodeGen {
            base: p as *mut u8,
            len,
            at: 0,
        }
    }
    fn emit(&mut self, bytes: &[u8]) -> u32 {
        let start = unsafe { self.base.add(self.at) };
        assert!(self.at + bytes.len() <= self.len, "codegen page full");
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), start, bytes.len()) };
        self.at += bytes.len();
        start as u32
    }
}

/// Every entry from retail code into ours goes through this thunk.
///
/// Two problems, one thunk:
///
/// 1. **Stack alignment.** MSVC guarantees only 4-byte alignment at a call site; the i386
///    psABI that Rust and the precompiled `std` are built against guarantees 16, and LLVM
///    emits `movaps [ebp-x]` on the strength of it. A misaligned `movaps` raises #GP,
///    which Linux delivers as SIGSEGV *with `si_addr == 0`* — indistinguishable at a
///    glance from retail dereferencing a null global, and it cost this lane a wrong
///    diagnosis before the thunk existed. `-C llvm-args=-stackrealign` fixes our own crate
///    but not the precompiled `std` it calls, so the fix has to be at the boundary.
/// 2. **Callee cleanup.** `ret imm16` lets one thunk serve both `__cdecl` (`ret_bytes` 0)
///    and `__stdcall` (`ret_bytes` = 4 x argument count) without Rust needing an
///    `extern "stdcall"` on a non-Windows target.
///
/// The 32 dwords of arguments copied across is a fixed over-approximation: no import we
/// bind takes more, and reading a little past the caller's arguments is harmless because
/// it is the caller's own live stack.
fn align_thunk(cg: &mut CodeGen, target: usize, ret_bytes: u16) -> u32 {
    let mut code: Vec<u8> = vec![
        0x55, // push ebp
        0x89, 0xe5, // mov  ebp, esp
        0x83, 0xe4, 0xf0, // and  esp, -16
        0x81, 0xec, 0x90, 0x00, 0x00, 0x00, // sub  esp, 0x90
        0x89, 0xb4, 0x24, 0x88, 0x00, 0x00, 0x00, // mov  [esp+0x88], esi
        0x89, 0xbc, 0x24, 0x8c, 0x00, 0x00, 0x00, // mov  [esp+0x8c], edi
        0x8d, 0x75, 0x08, // lea  esi, [ebp+8]
        0x89, 0xe7, // mov  edi, esp
        0xb9, 0x20, 0x00, 0x00, 0x00, // mov  ecx, 32
        0xf3, 0xa5, // rep  movsd
    ];
    let call_off = code.len();
    code.push(0xe8);
    code.extend_from_slice(&0u32.to_le_bytes()); // call rel32
    code.extend_from_slice(&[
        0x8b, 0xb4, 0x24, 0x88, 0x00, 0x00, 0x00, // mov esi, [esp+0x88]
        0x8b, 0xbc, 0x24, 0x8c, 0x00, 0x00, 0x00, // mov edi, [esp+0x8c]
        0x89, 0xec, // mov esp, ebp
        0x5d, // pop ebp
    ]);
    if ret_bytes == 0 {
        code.push(0xc3);
    } else {
        code.push(0xc2);
        code.extend_from_slice(&ret_bytes.to_le_bytes());
    }
    let entry = cg.emit(&code);
    let call_site = entry + call_off as u32;
    let rel = (target as u32).wrapping_sub(call_site + 5);
    unsafe { std::ptr::write_unaligned((call_site + 1) as *mut u32, rel) };
    entry
}

/// trap: realign, then `trap_report(idx, retail_ret)`. Never returns.
fn trap_stub(cg: &mut CodeGen, idx: u32) -> u32 {
    let mut code: Vec<u8> = vec![
        0x55, // push ebp
        0x89, 0xe5, // mov ebp, esp
        0x83, 0xe4, 0xf0, // and esp, -16
        0x83, 0xec, 0x08, // sub esp, 8
        0x8b, 0x45, 0x04, // mov eax, [ebp+4]   (retail return address)
        0x50, // push eax
        0x68, // push idx
    ];
    code.extend_from_slice(&idx.to_le_bytes());
    let call_off = code.len();
    code.push(0xe8);
    code.extend_from_slice(&0u32.to_le_bytes());
    let entry = cg.emit(&code);
    let call_site = entry + call_off as u32;
    let rel = (trap_report as usize as u32).wrapping_sub(call_site + 5);
    unsafe { std::ptr::write_unaligned((call_site + 1) as *mut u32, rel) };
    entry
}

static mut NAMES: Option<Vec<String>> = None;

extern "C" fn trap_report(idx: u32, ret: u32) {
    let n = unsafe {
        (*std::ptr::addr_of!(NAMES))
            .as_ref()
            .and_then(|v| v.get(idx as usize))
            .cloned()
            .unwrap_or_else(|| format!("#{idx}"))
    };
    let mut e = std::io::stderr();
    let _ = writeln!(e, "  [TRAP] unimplemented import {n}  (called from {ret:#010x})");
    let _ = e.flush();
    unsafe { MISSING.push(n) };
    crate::image::escape()
}

/// Every import that was actually reached and is not implemented, in call order.
pub static mut MISSING: Vec<String> = Vec::new();

// ---------------------------------------------------------------------------------
// Install
// ---------------------------------------------------------------------------------

pub struct Env {
    pub imports: Vec<Import>,
    pub implemented: usize,
    pub trapped: usize,
}

static mut CG: Option<&'static mut CodeGen> = None;

fn cg() -> &'static mut CodeGen {
    unsafe {
        let slot = &mut *std::ptr::addr_of_mut!(CG);
        if slot.is_none() {
            *slot = Some(Box::leak(Box::new(CodeGen::new(64))));
        }
        slot.as_mut().unwrap()
    }
}

/// Public entry for callers that need a retail-callable thunk of their own — the
/// `text_message` vtable hook, for instance.
pub fn make_thunk(target: usize, ret_bytes: u16) -> u32 {
    align_thunk(cg(), target, ret_bytes)
}

pub fn install(m: &Mapped) -> Env {
    let mut imports = parse_imports(m);
    let cg: &mut CodeGen = cg();
    let shims = shim_table();
    let mut names = Vec::with_capacity(imports.len());
    for im in &imports {
        names.push(format!("{}!{}", im.dll, im.name));
    }
    unsafe { NAMES = Some(names) };

    let (mut ok, mut bad) = (0usize, 0usize);
    for (i, im) in imports.iter_mut().enumerate() {
        let key = im.name.as_str();
        let target = match shims.iter().find(|(n, _, _)| *n == key) {
            Some((_, addr, Conv::Cdecl)) => {
                im.bound = Some("cdecl");
                align_thunk(cg, *addr, 0)
            }
            Some((_, addr, Conv::Stdcall(nbytes))) => {
                im.bound = Some("stdcall");
                align_thunk(cg, *addr, *nbytes as u16)
            }
            None => {
                bad += 1;
                trap_stub(cg, i as u32)
            }
        };
        if im.bound.is_some() {
            ok += 1;
        }
        m.write_u32(im.slot_va, target);
    }
    // The lazily-resolved wide Win32 layer. `riseofnations.exe` does not import the `*W`
    // APIs directly: 43 slots in `.data` start out pointing at per-API stubs that call
    // `ResolveThunk` (0x0040189d) -> `LoadLibraryA` -> `GetProcAddressInternal`. Filling
    // the slots ourselves bypasses that machinery completely. This is not optional even
    // for the smallest probe: `String::init_const` calls `String::char_to_wchar`, which
    // calls `MultiByteToWideChar` through slot 0x00c8dc44, so *constructing a String from
    // a literal* goes through here. [all VAs measured from the binary]
    for (slot, name, nargs, imp) in godot_table() {
        let idx = names_len_push(&format!("godot!{name}"));
        let target = match imp {
            Some(f) => align_thunk(cg, f, (nargs * 4) as u16),
            None => trap_stub(cg, idx),
        };
        if imp.is_some() {
            ok += 1;
        } else {
            bad += 1;
        }
        m.write_u32(slot, target);
    }

    Env {
        imports,
        implemented: ok,
        trapped: bad,
    }
}

fn names_len_push(s: &str) -> u32 {
    unsafe {
        let v = (*std::ptr::addr_of_mut!(NAMES)).as_mut().unwrap();
        v.push(s.to_string());
        (v.len() - 1) as u32
    }
}

/// (slot VA, API name, argument count, implementation) — see the comment in `install`.
fn godot_table() -> Vec<(u32, &'static str, u32, Option<usize>)> {
    vec![
        (0x00c8_dc44, "MultiByteToWideChar", 6, Some(mb2wc as usize)),
        (0x00c8_dc5c, "WideCharToMultiByte", 8, Some(wc2mb as usize)),
        (0x00c8_dc4c, "OutputDebugStringW", 1, Some(ods_w as usize)),
        (0x00c8_dc1c, "GetFileAttributesW", 1, Some(get_file_attributes_w as usize)),
        (0x00c8_dc14, "GetCurrentDirectoryW", 2, Some(get_cwd_w as usize)),
        (0x00c8_dc54, "SetCurrentDirectoryW", 1, Some(set_cwd_w as usize)),
        (0x00c8_dc20, "GetModuleFileNameW", 3, Some(get_module_file_name_w as usize)),
        (0x00c8_dc24, "GetModuleHandleW", 1, Some(k_fake_module as usize)),
        (0x00c8_dc3c, "LoadLibraryW", 1, Some(ret0_1 as usize)),
        (0x00c8_dc2c, "GetProcAddress", 2, Some(ret0_2 as usize)),
        (0x00c8_dbe4, "IsTextUnicode", 3, Some(ret0_3 as usize)),
        (0x00c8_dbf0, "CreateDirectoryW", 2, Some(create_directory_w as usize)),
        (0x00c8_dc08, "DeleteFileW", 1, Some(delete_file_w as usize)),
        (0x00c8_dc38, "GetTempPathW", 2, Some(get_temp_path_w as usize)),
        (0x00c8_dc58, "SetFileAttributesW", 2, Some(k_one as usize)),
        (0x00c8_dc80, "PostMessageW", 4, Some(k_one as usize)),
        (0x00c8_dc7c, "MessageBoxW", 4, Some(msgbox_w as usize)),
        (0x00c8_dbfc, "CreateFileW", 7, Some(create_file_w as usize)),
        (0x00c8_dc0c, "FindFirstFileW", 2, Some(find_first_file_w as usize)),
        (0x00c8_dc10, "FindNextFileW", 2, Some(find_next_file_w as usize)),
        (0x00c8_dbe0, "GetUserNameW", 2, Some(k_one as usize)),
        (0x00c8_dbe8, "RegOpenKeyExW", 5, Some(k_one as usize)),
        (0x00c8_dbec, "CopyFileW", 3, Some(k_one as usize)),
        (0x00c8_dbf4, "CreateEventW", 4, Some(k_fake_handle as usize)),
        (0x00c8_dbf8, "CreateFileMappingW", 6, Some(ret0_0 as usize)),
        (0x00c8_dc00, "CreateMutexW", 3, Some(k_fake_handle as usize)),
        (0x00c8_dc04, "CreateProcessW", 10, Some(ret0_0 as usize)),
        (0x00c8_dc18, "GetDiskFreeSpaceExW", 4, Some(k_one as usize)),
        (0x00c8_dc28, "GetPrivateProfileStringW", 6, Some(get_private_profile_string_w as usize)),
        (0x00c8_dc30, "GetStartupInfoW", 1, Some(ret0_1 as usize)),
        (0x00c8_dc34, "GetTempFileNameW", 4, Some(k_one as usize)),
        (0x00c8_dc40, "MoveFileW", 2, Some(k_one as usize)),
        (0x00c8_dc48, "OpenMutexW", 3, Some(k_fake_handle as usize)),
        (0x00c8_dc50, "RemoveDirectoryW", 1, Some(k_one as usize)),
        (0x00c8_dc60, "WritePrivateProfileStringW", 4, Some(k_one as usize)),
        (0x00c8_dc64, "ShellExecuteW", 6, Some(k_one as usize)),
        (0x00c8_dc68, "EnableWindow", 2, Some(k_one as usize)),
        (0x00c8_dc6c, "GetClipboardData", 1, Some(ret0_1 as usize)),
        (0x00c8_dc70, "GetKeyNameTextW", 3, Some(ret0_3 as usize)),
        (0x00c8_dc74, "IsClipboardFormatAvailable", 1, Some(ret0_1 as usize)),
        (0x00c8_dc78, "MapVirtualKeyW", 2, Some(ret0_2 as usize)),
        (0x00c8_dc84, "SystemParametersInfoW", 4, Some(k_one as usize)),
        (0x00c8_dc88, "VkKeyScanW", 1, Some(ret0_1 as usize)),
    ]
}

// ---- the wide Win32 layer -------------------------------------------------------

extern "C" fn mb2wc(
    _cp: u32,
    _flags: u32,
    src: *const u8,
    cb: i32,
    dst: *mut u16,
    cch: i32,
) -> i32 {
    if src.is_null() {
        return 0;
    }
    let bytes: &[u8] = unsafe {
        if cb < 0 {
            let mut n = 0usize;
            while *src.add(n) != 0 {
                n += 1;
            }
            std::slice::from_raw_parts(src, n + 1) // Windows counts the terminator
        } else {
            std::slice::from_raw_parts(src, cb as usize)
        }
    };
    let s = String::from_utf8_lossy(bytes);
    let u: Vec<u16> = s.encode_utf16().collect();
    if cch == 0 {
        return u.len() as i32;
    }
    if u.len() as i32 > cch || dst.is_null() {
        return 0;
    }
    unsafe {
        for (i, c) in u.iter().enumerate() {
            *dst.add(i) = *c;
        }
    }
    u.len() as i32
}

extern "C" fn wc2mb(
    _cp: u32,
    _flags: u32,
    src: *const u16,
    cch: i32,
    dst: *mut u8,
    cb: i32,
    _defchar: u32,
    used: *mut i32,
) -> i32 {
    if src.is_null() {
        return 0;
    }
    let u: Vec<u16> = unsafe {
        if cch < 0 {
            let mut n = 0usize;
            while *src.add(n) != 0 {
                n += 1;
            }
            (0..=n).map(|i| *src.add(i)).collect()
        } else {
            (0..cch as usize).map(|i| *src.add(i)).collect()
        }
    };
    let s = String::from_utf16_lossy(&u);
    let b = s.as_bytes();
    if !used.is_null() {
        unsafe { *used = 0 };
    }
    if cb == 0 {
        return b.len() as i32;
    }
    if b.len() as i32 > cb || dst.is_null() {
        return 0;
    }
    unsafe {
        for (i, c) in b.iter().enumerate() {
            *dst.add(i) = *c;
        }
    }
    b.len() as i32
}

extern "C" fn ods_w(s: *const u16) {
    eprint!("[ODS] {}", unsafe { wstr(s) });
}
extern "C" fn msgbox_w(_h: u32, text: *const u16, cap: *const u16, _f: u32) -> u32 {
    eprintln!(
        "[MessageBoxW] {} / {}",
        unsafe { wstr(cap) },
        unsafe { wstr(text) }
    );
    1
}
extern "C" fn get_file_attributes_w(p: *const u16) -> u32 {
    let path = resolve(&unsafe { wstr(p) });
    match std::fs::metadata(&path) {
        Ok(md) if md.is_dir() => 0x10,
        Ok(_) => 0x80,
        Err(_) => u32::MAX,
    }
}
fn put_wide(buf: *mut u16, cch: u32, s: &str) -> u32 {
    let u: Vec<u16> = s.encode_utf16().collect();
    if buf.is_null() || (u.len() + 1) as u32 > cch {
        return (u.len() + 1) as u32;
    }
    unsafe {
        for (i, c) in u.iter().enumerate() {
            *buf.add(i) = *c;
        }
        *buf.add(u.len()) = 0;
    }
    u.len() as u32
}
extern "C" fn get_cwd_w(cch: u32, buf: *mut u16) -> u32 {
    let d = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    put_wide(buf, cch, &d)
}
extern "C" fn set_cwd_w(p: *const u16) -> u32 {
    std::env::set_current_dir(unsafe { wstr(p) }).is_ok() as u32
}
extern "C" fn get_module_file_name_w(_h: u32, buf: *mut u16, cch: u32) -> u32 {
    put_wide(buf, cch, "riseofnations.exe")
}
extern "C" fn get_temp_path_w(cch: u32, buf: *mut u16) -> u32 {
    put_wide(buf, cch, "/tmp/")
}
extern "C" fn create_directory_w(p: *const u16, _sa: u32) -> u32 {
    std::fs::create_dir_all(unsafe { wstr(p) }.replace('\\', "/")).is_ok() as u32
}
/// No `.ini` files exist under the harness, so every query returns its default — which
/// is exactly what Windows does for a missing file.
extern "C" fn get_private_profile_string_w(
    _app: *const u16,
    _key: *const u16,
    default: *const u16,
    out: *mut u16,
    size: u32,
    _file: *const u16,
) -> u32 {
    let d = if default.is_null() {
        String::new()
    } else {
        unsafe { wstr(default) }
    };
    let n = put_wide(out, size, &d);
    n.min(size.saturating_sub(1))
}

extern "C" fn delete_file_w(p: *const u16) -> u32 {
    std::fs::remove_file(resolve(&unsafe { wstr(p) })).is_ok() as u32
}

#[derive(Clone, Copy)]
pub enum Conv {
    Cdecl,
    /// bytes of arguments the callee must pop
    Stdcall(u32),
}

macro_rules! c {
    ($t:expr, $f:expr) => {
        ($t, $f as usize, Conv::Cdecl)
    };
}
macro_rules! s {
    ($t:expr, $f:expr, $n:expr) => {
        ($t, $f as usize, Conv::Stdcall($n * 4))
    };
}

fn shim_table() -> Vec<(&'static str, usize, Conv)> {
    vec![
        // ---- heap ----
        c!("malloc", libc::malloc),
        c!("free", libc::free),
        c!("calloc", libc::calloc),
        c!("realloc", libc::realloc),
        c!("_set_new_mode", ret0_1),
        c!("_set_new_handler", ret0_1),
        // ---- vcruntime ----
        c!("memset", libc::memset),
        c!("memcpy", libc::memcpy),
        c!("memmove", libc::memmove),
        c!("strchr", libc::strchr),
        c!("strrchr", libc::strrchr),
        c!("wcschr", w_wcschr),
        c!("wcsrchr", w_wcsrchr),
        c!("wcsstr", w_wcsstr),
        c!("_purecall", purecall),
        c!("__std_type_info_compare", type_info_compare),
        // ---- runtime ----
        c!("_errno", my_errno),
        c!("_invalid_parameter_noinfo", ret0_0),
        c!("_invalid_parameter_noinfo_noreturn", ret0_0),
        c!("_initterm", my_initterm),
        c!("_initterm_e", my_initterm_e),
        c!("_crt_atexit", ret0_1),
        c!("_register_onexit_function", ret0_2),
        c!("_initialize_onexit_table", ret0_1),
        c!("_register_thread_local_exe_atexit_callback", ret0_1),
        c!("_controlfp_s", ret0_3),
        c!("_configthreadlocale", ret0_1),
        c!("_initialize_narrow_environment", ret0_0),
        c!("_configure_narrow_argv", ret0_1),
        c!("terminate", my_abort),
        c!("abort", my_abort),
        c!("exit", my_exit),
        c!("_exit", my_exit),
        c!("_c_exit", ret0_0),
        c!("_cexit", ret0_0),
        // ---- stdio ----
        c!("fopen", my_fopen),
        c!("fopen_s", my_fopen_s),
        c!("_wfsopen", my_wfsopen),
        c!("_wfopen_s", my_wfopen_s),
        c!("fclose", libc::fclose),
        c!("fread", libc::fread),
        c!("fwrite", libc::fwrite),
        c!("fseek", libc::fseek),
        c!("ftell", libc::ftell),
        c!("_fseeki64", my_fseeki64),
        c!("_ftelli64", my_ftelli64),
        c!("feof", libc::feof),
        c!("ferror", libc::ferror),
        c!("fflush", libc::fflush),
        c!("fgetc", libc::fgetc),
        c!("fputc", libc::fputc),
        c!("ungetc", libc::ungetc),
        c!("rewind", libc::rewind),
        c!("setvbuf", libc::setvbuf),
        c!("_fileno", libc::fileno),
        c!("_filelength", my_filelength),
        c!("__acrt_iob_func", my_iob),
        c!("__p__commode", my_p_commode),
        c!("_set_fmode", ret0_1),
        c!("__stdio_common_vfprintf", common_vfprintf),
        c!("__stdio_common_vsprintf", common_vsprintf),
        c!("__stdio_common_vsprintf_s", common_vsprintf),
        c!("__stdio_common_vsnprintf_s", common_vsnprintf_s),
        c!("fgetwc", my_fgetwc),
        c!("fputwc", my_fputwc),
        c!("ungetwc", my_ungetwc),
        c!("fgetws", my_fgetws),
        c!("fputws", my_fputws),
        c!("fgetpos", my_fgetpos),
        c!("fsetpos", my_fsetpos),
        c!("_get_stream_buffer_pointers", ret_neg1_5),
        c!("_lock_file", ret0_1),
        c!("_unlock_file", ret0_1),
        // ---- narrow string ----
        c!("strncmp", libc::strncmp),
        c!("strnlen", my_strnlen),
        c!("_stricmp", libc::strcasecmp),
        c!("isdigit", libc::isdigit),
        c!("isxdigit", libc::isxdigit),
        c!("tolower", libc::tolower),
        c!("toupper", libc::toupper),
        c!("strcpy_s", my_strcpy_s),
        c!("strncpy_s", my_strncpy_s),
        c!("strncat_s", my_strncat_s),
        c!("strtok_s", my_strtok_s),
        // ---- wide string (Windows wchar_t is 16-bit; musl's is 32-bit, so all hand-written)
        c!("wcsncmp", w_wcsncmp),
        c!("_wcsicmp", w_wcsicmp),
        c!("_wcsnicmp", w_wcsnicmp),
        c!("_wcsnicoll", w_wcsnicmp),
        c!("wcscspn", w_wcscspn),
        c!("wcscpy_s", w_wcscpy_s),
        c!("wcscat_s", w_wcscat_s),
        c!("wcsncpy_s", w_wcsncpy_s),
        c!("towupper", w_towupper),
        c!("towlower", w_towlower),
        c!("iswspace", w_iswspace),
        // ---- convert ----
        c!("atoi", libc::atoi),
        c!("strtod", libc::strtod),
        c!("_wtoi", w_wtoi),
        c!("_wtoi64", w_wtoi64),
        c!("wcstod", w_wcstod),
        c!("wcstol", w_wcstol),
        c!("wcstoul", w_wcstoul),
        c!("_wcstoui64", w_wcstoui64),
        c!("wcstoull", w_wcstoui64),
        c!("_itow_s", w_itow_s),
        c!("_ultow_s", w_ultow_s),
        c!("_ui64tow_s", w_ui64tow_s),
        c!("_i64toa_s", my_i64toa_s),
        c!("mbstowcs", w_mbstowcs),
        c!("wcstombs_s", w_wcstombs_s),
        c!("mbtowc", w_mbtowc),
        c!("wctomb_s", w_wctomb_s),
        c!("_gcvt_s", my_gcvt_s),
        // ---- utility / math / time ----
        c!("qsort", libc::qsort),
        c!("_set_SSE2_enable", ret0_1),
        c!("__setusermatherr", ret0_1),
        c!("_time64", my_time64),
        c!("_set_app_type", ret0_1),
        c!("_libm_sse2_acos_precise", m_acos),
        c!("_libm_sse2_asin_precise", m_asin),
        c!("_libm_sse2_atan_precise", m_atan),
        c!("_libm_sse2_cos_precise", m_cos),
        c!("_libm_sse2_sin_precise", m_sin),
        c!("_libm_sse2_tan_precise", m_tan),
        c!("_libm_sse2_pow_precise", m_pow),
        c!("_libm_sse2_sqrt_precise", m_sqrt),
        c!("_except1", ret0_4),
        c!("_localtime64_s", my_localtime64_s),
        c!("_gmtime64_s", my_localtime64_s),
        c!("_localtime64", my_localtime64),
        c!("asctime_s", my_asctime_s),
        c!("wcsftime", my_wcsftime),
        // ---- wide printf ----
        c!("__stdio_common_vswprintf_s", common_vswprintf),
        c!("__stdio_common_vsnwprintf_s", common_vsnwprintf_s),
        c!("__stdio_common_vfwprintf", common_vfwprintf),
        // ---- filesystem ----
        c!("_wgetcwd", my_wgetcwd),
        c!("_wchdir", my_wchdir),
        c!("_wmkdir", my_wmkdir),
        c!("_wremove", my_wremove),
        c!("_wrename", my_wrename),
        c!("_wfindfirst64i32", my_wfindfirst),
        c!("_wfindnext64i32", my_wfindnext),
        c!("_findclose", my_findclose),
        c!("_seh_filter_exe", ret0_2),
        c!("_get_narrow_winmain_command_line", ret0_0),
        c!("__RTDynamicCast", ret0_5),
        c!("__std_exception_copy", ret0_2),
        c!("__std_exception_destroy", ret0_1),
        // ---- kernel32 (stdcall) ----
        s!("GetLastError", k_getlasterror, 0),
        s!("SetLastError", ret0_1, 1),
        s!("GetCurrentThreadId", k_tid, 0),
        s!("GetCurrentProcessId", k_tid, 0),
        s!("GetCurrentProcess", k_minus1, 0),
        s!("GetCurrentThread", k_minus1, 0),
        s!("IsDebuggerPresent", ret0_0, 0),
        s!("InitializeSListHead", ret0_1, 1),
        s!("IsProcessorFeaturePresent", k_one, 1),
        s!("GetVersion", k_version, 0),
        s!("OutputDebugStringA", k_outputdebug, 1),
        s!("EnterCriticalSection", ret0_1, 1),
        s!("LeaveCriticalSection", ret0_1, 1),
        s!("DeleteCriticalSection", ret0_1, 1),
        s!("Sleep", ret0_1, 1),
        s!("timeGetTime", k_ticks, 0),
        s!("GetTickCount64", k_ticks64, 0),
        s!("QueryPerformanceFrequency", k_qpf, 1),
        s!("QueryPerformanceCounter", k_qpc, 1),
        s!("GetSystemTimeAsFileTime", k_systime, 1),
        s!("__vcrt_InitializeCriticalSectionEx", k_one, 3),
        s!("GetModuleHandleA", ret0_1, 1),
        s!("GetModuleFileNameA", ret0_3, 3),
        s!("SetUnhandledExceptionFilter", ret0_1, 1),
        s!("UnhandledExceptionFilter", ret0_1, 1),
        s!("VirtualQuery", ret0_3, 3),
        s!("VirtualProtect", k_one, 4),
        s!("FlushInstructionCache", k_one, 3),
        s!("CreateDirectoryA", k_one, 2),
        s!("GetLocalTime", ret0_1, 1),
        s!("MessageBoxA", ret0_4, 4),
        s!("GetDoubleClickTime", k_dblclick, 0),
        s!("GetSystemMetrics", ret0_1, 1),
        s!("SHGetFolderPathW", sh_get_folder_path_w, 5),
        s!("GetWindowsDirectoryA", ret0_2, 2),
        s!("GetSystemDirectoryA", ret0_2, 2),
        s!("GetSystemInfo", ret0_1, 1),
        s!("GetLocaleInfoEx", ret0_4, 4),
        s!("GetUserDefaultLocaleName", ret0_2, 2),
        s!("CompareStringA", k_one, 6),
        s!("VerSetConditionMask", k_vsc, 4),
        s!("VerifyVersionInfoW", k_one, 3),
        s!("DisableProcessWindowsGhosting", ret0_0, 0),
        s!("CoInitialize", ret0_1, 1),
        s!("CoInitializeEx", ret0_2, 2),
        s!("CoUninitialize", ret0_0, 0),
        s!("CoCreateGuid", ret0_1, 1),
        s!("RegOpenKeyExA", k_one, 5),
        s!("RegCloseKey", ret0_1, 1),
        s!("RegQueryValueExA", k_one, 6),
        s!("timeBeginPeriod", ret0_1, 1),
        s!("SetThreadPriority", k_one, 2),
        s!("GetThreadPriority", ret0_1, 1),
        s!("LocalFree", ret0_1, 1),
        s!("InterlockedExchange", k_interlocked_exchange, 2),
        s!("GlobalAlloc", ret0_2, 2),
        s!("GlobalLock", ret0_1, 1),
        s!("GlobalUnlock", ret0_1, 1),
        s!("SetEvent", k_one, 1),
        s!("ResetEvent", k_one, 1),
        s!("CloseHandle", k_one, 1),
        s!("FreeLibrary", k_one, 1),
        s!("LoadLibraryA", ret0_1, 1),
        s!("LoadLibraryExA", ret0_3, 3),
        s!("SymSetOptions", ret0_1, 1),
        s!("SymInitialize", ret0_3, 3),
        s!("CreateFileA", create_file_a, 7),
        s!("ReadFile", read_file, 5),
        s!("WriteFile", write_file, 5),
        s!("SetFilePointer", set_file_pointer, 4),
        s!("SetFilePointerEx", set_file_pointer_ex, 5),
        s!("GetFileSize", get_file_size, 2),
        s!("GetFileSizeEx", get_file_size_ex, 2),
        s!("SetEndOfFile", set_end_of_file, 1),
        s!("FlushFileBuffers", flush_file_buffers, 1),
        s!("FindClose", find_close, 1),
        s!("CloseHandle", close_handle, 1),
        s!("WaitForSingleObject", ret0_2, 2),
        s!("WaitForSingleObjectEx", ret0_3, 3),
        s!("ReleaseMutex", k_one, 1),
        s!("TerminateProcess", k_one, 2),
        s!("GetCommandLineW", k_cmdline_w, 0),
        s!("UnmapViewOfFile", k_one, 1),
        s!("MapViewOfFile", ret0_5, 5),
        s!("SystemTimeToTzSpecificLocalTime", k_one, 3),
        s!("FileTimeToSystemTime", k_one, 2),
        s!("RaiseException", ret0_4, 4),
        s!("timeSetEvent", ret0_5, 5),
        s!("timeKillEvent", ret0_1, 1),
        s!("GetDesktopWindow", ret0_0, 0),
        s!("GetWindowRect", ret0_2, 2),
        s!("GetClientRect", ret0_2, 2),
        s!("MoveWindow", ret0_5, 5),
        s!("ShowWindow", ret0_2, 2),
        s!("EndDialog", ret0_2, 2),
        s!("OpenClipboard", ret0_1, 1),
        s!("CloseClipboard", ret0_0, 0),
        s!("EmptyClipboard", ret0_0, 0),
        s!("SetClipboardData", ret0_2, 2),
        s!("KillTimer", ret0_2, 2),
        s!("SetTimer", ret0_4, 4),
        s!("GetAsyncKeyState", ret0_1, 1),
        s!("GetKeyboardState", ret0_1, 1),
        s!("GetKeyState", ret0_1, 1),
        s!("MessageBeep", ret0_1, 1),
        s!("GetDlgItem", ret0_2, 2),
        s!("SetDlgItemTextA", ret0_3, 3),
        s!("SendDlgItemMessageA", ret0_5, 5),
        s!("CoCreateInstance", k_one, 5),
        s!("MiniDumpWriteDump", ret0_0, 7),
    ]
}

extern "C" fn m_acos(x: f64) -> f64 { x.acos() }
extern "C" fn m_asin(x: f64) -> f64 { x.asin() }
extern "C" fn m_atan(x: f64) -> f64 { x.atan() }
extern "C" fn m_cos(x: f64) -> f64 { x.cos() }
extern "C" fn m_sin(x: f64) -> f64 { x.sin() }
extern "C" fn m_tan(x: f64) -> f64 { x.tan() }
extern "C" fn m_pow(x: f64, y: f64) -> f64 { x.powf(y) }
extern "C" fn m_sqrt(x: f64) -> f64 { x.sqrt() }

extern "C" fn ret0_5(_: u32, _: u32, _: u32, _: u32, _: u32) -> u32 {
    0
}
extern "C" fn k_fake_handle(_: u32, _: u32, _: u32) -> u32 {
    0x1234
}
static EMPTY_CMDLINE: [u16; 1] = [0];
extern "C" fn k_cmdline_w() -> *const u16 {
    EMPTY_CMDLINE.as_ptr()
}

/// Non-null, because `__scrt_initialize_thread_safe_statics` fastfails on a null
/// `GetModuleHandleW(L"kernel32.dll")`. Paired with `GetProcAddress` returning 0, the CRT
/// then takes its documented fallback path and implements magic statics with an event
/// instead of a condition variable — which is fine here, the harness is single-threaded.
/// Without this the CRT never initialises and the *first* function-local `static` in the
/// compiler jumps through a garbage decoded pointer.
extern "C" fn k_fake_module(_name: u32) -> u32 {
    0x1000_0000
}

extern "C" fn k_dblclick() -> u32 {
    500
}
extern "C" fn k_vsc(lo: u32, hi: u32, _t: u32, _c: u32) -> u64 {
    ((hi as u64) << 32) | lo as u64
}
extern "C" fn k_interlocked_exchange(target: *mut u32, value: u32) -> u32 {
    unsafe {
        let old = *target;
        *target = value;
        old
    }
}
extern "C" fn sh_get_folder_path_w(
    _hwnd: u32,
    _csidl: u32,
    _tok: u32,
    _flags: u32,
    path: *mut u16,
) -> u32 {
    put_wide(path, 260, "/tmp");
    0
}

// ---- time -----------------------------------------------------------------------

#[repr(C)]
struct Tm {
    sec: c_int,
    min: c_int,
    hour: c_int,
    mday: c_int,
    mon: c_int,
    year: c_int,
    wday: c_int,
    yday: c_int,
    isdst: c_int,
}
extern "C" fn my_localtime64_s(out: *mut Tm, _t: *const i64) -> c_int {
    if out.is_null() {
        return 22;
    }
    unsafe {
        *out = Tm {
            sec: 0,
            min: 0,
            hour: 0,
            mday: 1,
            mon: 0,
            year: 124,
            wday: 1,
            yday: 0,
            isdst: 0,
        }
    };
    0
}
static mut TM_STATIC: Tm = Tm {
    sec: 0,
    min: 0,
    hour: 0,
    mday: 1,
    mon: 0,
    year: 124,
    wday: 1,
    yday: 0,
    isdst: 0,
};
extern "C" fn my_localtime64(_t: *const i64) -> *mut Tm {
    unsafe { std::ptr::addr_of_mut!(TM_STATIC) }
}
extern "C" fn my_asctime_s(buf: *mut c_char, n: usize, _tm: *const Tm) -> c_int {
    let s = b"Mon Jan  1 00:00:00 2024\n\0";
    unsafe {
        for (i, c) in s.iter().enumerate() {
            if i >= n {
                break;
            }
            *buf.add(i) = *c as c_char;
        }
    }
    0
}
extern "C" fn my_wcsftime(buf: *mut u16, n: usize, _fmt: *const u16, _tm: *const Tm) -> usize {
    put_wide(buf, n as u32, "2024-01-01") as usize
}

// ---- filesystem -----------------------------------------------------------------

extern "C" fn my_wgetcwd(buf: *mut u16, n: c_int) -> *mut u16 {
    let d = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    if buf.is_null() {
        let u: Vec<u16> = d.encode_utf16().chain(std::iter::once(0)).collect();
        let p = unsafe { libc::malloc(u.len() * 2) } as *mut u16;
        unsafe { std::ptr::copy_nonoverlapping(u.as_ptr(), p, u.len()) };
        return p;
    }
    put_wide(buf, n as u32, &d);
    buf
}
extern "C" fn my_wchdir(p: *const u16) -> c_int {
    if std::env::set_current_dir(unsafe { wstr(p) }.replace('\\', "/")).is_ok() {
        0
    } else {
        -1
    }
}
extern "C" fn my_wmkdir(p: *const u16) -> c_int {
    if std::fs::create_dir_all(unsafe { wstr(p) }.replace('\\', "/")).is_ok() {
        0
    } else {
        -1
    }
}
extern "C" fn my_wremove(p: *const u16) -> c_int {
    if std::fs::remove_file(resolve(&unsafe { wstr(p) })).is_ok() {
        0
    } else {
        -1
    }
}
extern "C" fn my_wrename(a: *const u16, b: *const u16) -> c_int {
    let (a, b) = (resolve(&unsafe { wstr(a) }), unsafe { wstr(b) }.replace('\\', "/"));
    if std::fs::rename(a, b).is_ok() {
        0
    } else {
        -1
    }
}
extern "C" fn my_wfindfirst(_p: *const u16, _fd: *mut u8) -> isize {
    -1
}
extern "C" fn my_wfindnext(_h: isize, _fd: *mut u8) -> c_int {
    -1
}
extern "C" fn my_findclose(_h: isize) -> c_int {
    0
}

// ---- the Win32 HANDLE file layer ------------------------------------------------
//
// A HANDLE is `0x4000_0000 | fd`. The tag keeps `CloseHandle` from closing a file
// descriptor when the game hands it a mutex or event handle from one of the stubs.

const HTAG: u32 = 0x4000_0000;
const INVALID_HANDLE: u32 = 0xffff_ffff;

fn h_fd(h: u32) -> Option<c_int> {
    if h & 0xf000_0000 == HTAG {
        Some((h & 0x0fff_ffff) as c_int)
    } else {
        None
    }
}

fn win_open(path: &str, access: u32, disposition: u32) -> u32 {
    let want_write = access & 0x4000_0000 != 0;
    let mut flags = if want_write {
        libc::O_RDWR
    } else {
        libc::O_RDONLY
    };
    match disposition {
        1 => flags |= libc::O_CREAT | libc::O_EXCL,       // CREATE_NEW
        2 => flags |= libc::O_CREAT | libc::O_TRUNC,      // CREATE_ALWAYS
        4 => flags |= libc::O_CREAT,                      // OPEN_ALWAYS
        5 => flags |= libc::O_TRUNC,                      // TRUNCATE_EXISTING
        _ => {}                                           // OPEN_EXISTING
    }
    let target = if disposition == 1 || disposition == 2 || disposition == 4 {
        path.replace('\\', "/")
    } else {
        resolve(path)
    };
    let c = match std::ffi::CString::new(target.clone()) {
        Ok(c) => c,
        Err(_) => return INVALID_HANDLE,
    };
    let fd = unsafe { libc::open(c.as_ptr(), flags, 0o644) };
    if unsafe { TRACE_IO } {
        eprintln!("[io] CreateFile({path:?} acc={access:#x} disp={disposition}) -> {target:?} fd={fd}");
    }
    if fd < 0 {
        INVALID_HANDLE
    } else {
        HTAG | fd as u32
    }
}

extern "C" fn create_file_w(
    name: *const u16,
    access: u32,
    _share: u32,
    _sa: u32,
    disposition: u32,
    _flags: u32,
    _tmpl: u32,
) -> u32 {
    win_open(&unsafe { wstr(name) }, access, disposition)
}
extern "C" fn create_file_a(
    name: *const c_char,
    access: u32,
    _share: u32,
    _sa: u32,
    disposition: u32,
    _flags: u32,
    _tmpl: u32,
) -> u32 {
    win_open(&unsafe { cstr(name) }, access, disposition)
}
extern "C" fn read_file(h: u32, buf: *mut u8, n: u32, got: *mut u32, _ovl: u32) -> u32 {
    let fd = match h_fd(h) {
        Some(f) => f,
        None => return 0,
    };
    let r = unsafe { libc::read(fd, buf as *mut c_void, n as usize) };
    if r < 0 {
        return 0;
    }
    if !got.is_null() {
        unsafe { *got = r as u32 };
    }
    1
}
extern "C" fn write_file(h: u32, buf: *const u8, n: u32, put: *mut u32, _ovl: u32) -> u32 {
    let fd = match h_fd(h) {
        Some(f) => f,
        None => return 0,
    };
    let r = unsafe { libc::write(fd, buf as *const c_void, n as usize) };
    if r < 0 {
        return 0;
    }
    if !put.is_null() {
        unsafe { *put = r as u32 };
    }
    1
}
extern "C" fn set_file_pointer(h: u32, lo: i32, hi: *mut i32, method: u32) -> u32 {
    let fd = match h_fd(h) {
        Some(f) => f,
        None => return INVALID_HANDLE,
    };
    let high = if hi.is_null() { 0i64 } else { (unsafe { *hi }) as i64 };
    let off = (high << 32) | (lo as u32 as i64);
    let whence = match method {
        1 => libc::SEEK_CUR,
        2 => libc::SEEK_END,
        _ => libc::SEEK_SET,
    };
    let r = unsafe { libc::lseek64(fd, off, whence) };
    if r < 0 {
        return INVALID_HANDLE;
    }
    if !hi.is_null() {
        unsafe { *hi = (r >> 32) as i32 };
    }
    r as u32
}
extern "C" fn set_file_pointer_ex(
    h: u32,
    lo: u32,
    hi: u32,
    newpos: *mut i64,
    method: u32,
) -> u32 {
    let fd = match h_fd(h) {
        Some(f) => f,
        None => return 0,
    };
    let off = (((hi as u64) << 32) | lo as u64) as i64;
    let whence = match method {
        1 => libc::SEEK_CUR,
        2 => libc::SEEK_END,
        _ => libc::SEEK_SET,
    };
    let r = unsafe { libc::lseek64(fd, off, whence) };
    if r < 0 {
        return 0;
    }
    if !newpos.is_null() {
        unsafe { *newpos = r };
    }
    1
}
fn fsize(h: u32) -> Option<i64> {
    let fd = h_fd(h)?;
    let mut st: libc::stat64 = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat64(fd, &mut st) } != 0 {
        return None;
    }
    Some(st.st_size as i64)
}
extern "C" fn get_file_size(h: u32, hi: *mut u32) -> u32 {
    match fsize(h) {
        Some(n) => {
            if !hi.is_null() {
                unsafe { *hi = (n >> 32) as u32 };
            }
            n as u32
        }
        None => INVALID_HANDLE,
    }
}
extern "C" fn get_file_size_ex(h: u32, out: *mut i64) -> u32 {
    match fsize(h) {
        Some(n) => {
            unsafe { *out = n };
            1
        }
        None => 0,
    }
}
extern "C" fn set_end_of_file(h: u32) -> u32 {
    let fd = match h_fd(h) {
        Some(f) => f,
        None => return 0,
    };
    let pos = unsafe { libc::lseek64(fd, 0, libc::SEEK_CUR) };
    (unsafe { libc::ftruncate64(fd, pos) } == 0) as u32
}
extern "C" fn flush_file_buffers(h: u32) -> u32 {
    match h_fd(h) {
        Some(fd) => (unsafe { libc::fsync(fd) } == 0) as u32,
        None => 1,
    }
}
extern "C" fn close_handle(h: u32) -> u32 {
    match h_fd(h) {
        Some(fd) => (unsafe { libc::close(fd) } == 0) as u32,
        None => 1,
    }
}

// ---- FindFirstFileW / FindNextFileW ---------------------------------------------

struct FindState {
    entries: Vec<String>,
    at: usize,
}
static mut FINDS: Vec<Option<FindState>> = Vec::new();

fn fill_find_data(fd: *mut u8, name: &str) {
    unsafe {
        std::ptr::write_bytes(fd, 0, 592);
        std::ptr::write_unaligned(fd as *mut u32, 0x80);
        let u: Vec<u16> = name.encode_utf16().take(259).collect();
        let dst = fd.add(44) as *mut u16;
        for (i, c) in u.iter().enumerate() {
            *dst.add(i) = *c;
        }
        *dst.add(u.len()) = 0;
    }
}

extern "C" fn find_first_file_w(pattern: *const u16, out: *mut u8) -> u32 {
    let pat = unsafe { wstr(pattern) }.replace('\\', "/");
    let (dir, glob) = match pat.rfind('/') {
        Some(i) => (pat[..i].to_string(), pat[i + 1..].to_string()),
        None => (".".to_string(), pat.clone()),
    };
    let suffix = glob.rsplit('*').next().unwrap_or("").to_string();
    let mut entries: Vec<String> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(resolve(&dir)) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if suffix.is_empty() || n.to_lowercase().ends_with(&suffix.to_lowercase()) {
                entries.push(n);
            }
        }
    }
    if entries.is_empty() {
        return INVALID_HANDLE;
    }
    fill_find_data(out, &entries[0]);
    unsafe {
        let v = &mut *std::ptr::addr_of_mut!(FINDS);
        v.push(Some(FindState { entries, at: 1 }));
        (v.len() - 1) as u32
    }
}
extern "C" fn find_next_file_w(h: u32, out: *mut u8) -> u32 {
    unsafe {
        let v = &mut *std::ptr::addr_of_mut!(FINDS);
        match v.get_mut(h as usize).and_then(|s| s.as_mut()) {
            Some(st) if st.at < st.entries.len() => {
                fill_find_data(out, &st.entries[st.at]);
                st.at += 1;
                1
            }
            _ => 0,
        }
    }
}
extern "C" fn find_close(h: u32) -> u32 {
    unsafe {
        let v = &mut *std::ptr::addr_of_mut!(FINDS);
        if let Some(s) = v.get_mut(h as usize) {
            *s = None;
        }
    }
    1
}

// ---- wide printf ----------------------------------------------------------------

/// A UTF-16 `printf`. MSVC's wide `%s` is `wchar_t*` and `%S`/`%hs` is `char*`, which no
/// host `vswprintf` will agree with (and musl's `wchar_t` is 32-bit besides), so this is
/// written out rather than delegated.
fn wide_format(fmt: *const u16, ap: *mut c_void) -> Vec<u16> {
    let f = unsafe { wstr(fmt) };
    let mut out = String::new();
    let mut args = ap as *const u32;
    let mut next = || unsafe {
        let v = std::ptr::read_unaligned(args);
        args = args.add(1);
        v
    };
    let mut it = f.chars().peekable();
    while let Some(c) = it.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut spec = String::new();
        // flags, width, precision, length modifier
        while let Some(&p) = it.peek() {
            if "-+ #0".contains(p) {
                spec.push(p);
                it.next();
            } else {
                break;
            }
        }
        let mut width = String::new();
        while let Some(&p) = it.peek() {
            if p.is_ascii_digit() {
                width.push(p);
                it.next();
            } else {
                break;
            }
        }
        let mut prec = String::new();
        if it.peek() == Some(&'.') {
            it.next();
            while let Some(&p) = it.peek() {
                if p.is_ascii_digit() {
                    prec.push(p);
                    it.next();
                } else {
                    break;
                }
            }
        }
        let mut wide_arg = true;
        while let Some(&p) = it.peek() {
            if p == 'l' || p == 'h' || p == 'I' || p == '6' || p == '4' || p == 'w' {
                if p == 'h' {
                    wide_arg = false;
                }
                spec.push(p);
                it.next();
            } else {
                break;
            }
        }
        let conv = match it.next() {
            Some(c) => c,
            None => break,
        };
        let w: usize = width.parse().unwrap_or(0);
        let p: Option<usize> = prec.parse().ok();
        let left = spec.contains('-');
        let zero = spec.contains('0');
        let body = match conv {
            '%' => "%".to_string(),
            'd' | 'i' => format!("{}", next() as i32),
            'u' => format!("{}", next()),
            'x' => format!("{:x}", next()),
            'X' => format!("{:X}", next()),
            'o' => format!("{:o}", next()),
            'p' => format!("{:08X}", next()),
            'c' => {
                let v = next();
                char::from_u32(v & 0xffff).unwrap_or('?').to_string()
            }
            'f' | 'F' | 'e' | 'E' | 'g' | 'G' => {
                let lo = next() as u64;
                let hi = next() as u64;
                let d = f64::from_bits((hi << 32) | lo);
                match p {
                    Some(n) => format!("{:.*}", n, d),
                    None => format!("{:.6}", d),
                }
            }
            's' | 'S' => {
                let ptr = next();
                if ptr == 0 {
                    "(null)".into()
                } else if (conv == 's') == wide_arg {
                    unsafe { wstr(ptr as *const u16) }
                } else {
                    unsafe { CStr::from_ptr(ptr as *const c_char) }
                        .to_string_lossy()
                        .into_owned()
                }
            }
            other => {
                let _ = next();
                format!("%{other}")
            }
        };
        let pad = w.saturating_sub(body.chars().count());
        if left {
            out.push_str(&body);
            for _ in 0..pad {
                out.push(' ');
            }
        } else {
            for _ in 0..pad {
                out.push(if zero { '0' } else { ' ' });
            }
            out.push_str(&body);
        }
    }
    out.encode_utf16().collect()
}

fn emit_wide(buf: *mut u16, count: usize, v: &[u16]) -> c_int {
    if buf.is_null() || count == 0 {
        return v.len() as c_int;
    }
    let k = v.len().min(count - 1);
    unsafe {
        for i in 0..k {
            *buf.add(i) = v[i];
        }
        *buf.add(k) = 0;
    }
    k as c_int
}

extern "C" fn common_vswprintf(
    _o_lo: u32,
    _o_hi: u32,
    buf: *mut u16,
    count: usize,
    fmt: *const u16,
    _loc: u32,
    ap: *mut c_void,
) -> c_int {
    let v = wide_format(fmt, ap);
    emit_wide(buf, count, &v)
}
extern "C" fn common_vsnwprintf_s(
    _o_lo: u32,
    _o_hi: u32,
    buf: *mut u16,
    count: usize,
    maxcount: usize,
    fmt: *const u16,
    _loc: u32,
    ap: *mut c_void,
) -> c_int {
    let v = wide_format(fmt, ap);
    emit_wide(buf, count.min(maxcount.saturating_add(1)), &v)
}
extern "C" fn common_vfwprintf(
    _o_lo: u32,
    _o_hi: u32,
    f: *mut libc::FILE,
    fmt: *const u16,
    _loc: u32,
    ap: *mut c_void,
) -> c_int {
    let v = wide_format(fmt, ap);
    let s = String::from_utf16_lossy(&v);
    let c = std::ffi::CString::new(s.replace('\0', "")).unwrap();
    unsafe { libc::fputs(c.as_ptr(), f) }
}

// ---------------------------------------------------------------------------------
// Trivial shims
// ---------------------------------------------------------------------------------

extern "C" fn ret0_0() -> u32 {
    0
}
extern "C" fn ret0_1(_: u32) -> u32 {
    0
}
extern "C" fn ret0_2(_: u32, _: u32) -> u32 {
    0
}
extern "C" fn ret0_3(_: u32, _: u32, _: u32) -> u32 {
    0
}
extern "C" fn ret0_4(_: u32, _: u32, _: u32, _: u32) -> u32 {
    0
}
extern "C" fn ret_neg1_5(_: u32, _: u32, _: u32, _: u32, _: u32) -> u32 {
    u32::MAX
}
extern "C" fn k_one(_: u32) -> u32 {
    1
}
extern "C" fn k_minus1() -> u32 {
    u32::MAX
}
extern "C" fn k_tid() -> u32 {
    1
}
extern "C" fn k_version() -> u32 {
    0x0000_0A00
}
extern "C" fn k_getlasterror() -> u32 {
    0
}
extern "C" fn k_ticks() -> u32 {
    0
}
extern "C" fn k_ticks64() -> u64 {
    0
}
extern "C" fn k_qpf(p: *mut u64) -> u32 {
    unsafe { *p = 1_000_000 };
    1
}
extern "C" fn k_qpc(p: *mut u64) -> u32 {
    unsafe { *p = 0 };
    1
}
extern "C" fn k_systime(p: *mut u64) -> u32 {
    unsafe { *p = 0 };
    1
}
extern "C" fn k_outputdebug(s: *const c_char) {
    if !s.is_null() {
        let m = unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned();
        eprint!("[ODS] {m}");
    }
}
extern "C" fn purecall() {
    eprintln!("  [TRAP] _purecall");
    crate::image::escape()
}
extern "C" fn type_info_compare(_a: u32, _b: u32) -> u32 {
    1
}
extern "C" fn my_abort() {
    eprintln!("  [TRAP] abort()/terminate()");
    crate::image::escape()
}
extern "C" fn my_exit(code: u32) {
    eprintln!("  [TRAP] exit({code})");
    crate::image::escape()
}
extern "C" fn my_errno() -> *mut c_int {
    unsafe { libc::__errno_location() }
}
static mut COMMODE: c_int = 0;
extern "C" fn my_p_commode() -> *mut c_int {
    unsafe { std::ptr::addr_of_mut!(COMMODE) }
}
extern "C" fn my_time64(p: *mut i64) -> i64 {
    let t = unsafe { libc::time(std::ptr::null_mut()) } as i64;
    if !p.is_null() {
        unsafe { *p = t };
    }
    t
}

extern "C" fn my_initterm(first: *const usize, last: *const usize) {
    let mut p = first;
    while p < last {
        let f = unsafe { *p };
        if f != 0 {
            let f: extern "C" fn() = unsafe { std::mem::transmute(f) };
            f();
        }
        p = unsafe { p.add(1) };
    }
}
extern "C" fn my_initterm_e(first: *const usize, last: *const usize) -> u32 {
    let mut p = first;
    while p < last {
        let f = unsafe { *p };
        if f != 0 {
            let f: extern "C" fn() -> u32 = unsafe { std::mem::transmute(f) };
            let r = f();
            if r != 0 {
                return r;
            }
        }
        p = unsafe { p.add(1) };
    }
    0
}

// ---------------------------------------------------------------------------------
// Files. Retail paths are Windows-shaped (`data\scripts\x.bhs`); the harness runs on a
// case-sensitive filesystem, so translate separators and fall back to a case-insensitive
// walk before giving up.
// ---------------------------------------------------------------------------------

pub static mut TRACE_IO: bool = false;

fn resolve(path: &str) -> String {
    let unixish = path.replace('\\', "/");
    if std::path::Path::new(&unixish).exists() {
        return unixish;
    }
    // case-insensitive component walk
    let abs = unixish.starts_with('/');
    let mut cur = if abs {
        String::from("/")
    } else {
        String::from(".")
    };
    for comp in unixish.split('/').filter(|c| !c.is_empty() && *c != ".") {
        let mut hit: Option<String> = None;
        if let Ok(rd) = std::fs::read_dir(&cur) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.eq_ignore_ascii_case(comp) {
                    hit = Some(n);
                    break;
                }
            }
        }
        let next = hit.unwrap_or_else(|| comp.to_string());
        if cur.ends_with('/') {
            cur.push_str(&next);
        } else {
            cur.push('/');
            cur.push_str(&next);
        }
    }
    if let Some(stripped) = cur.strip_prefix("./") {
        stripped.to_string()
    } else {
        cur
    }
}

fn open(path: &str, mode: &str) -> *mut libc::FILE {
    let real = resolve(path);
    let cm = std::ffi::CString::new(mode.replace('t', "")).unwrap();
    let cp = std::ffi::CString::new(real.clone()).unwrap();
    let f = unsafe { libc::fopen(cp.as_ptr(), cm.as_ptr()) };
    if unsafe { TRACE_IO } {
        eprintln!(
            "[io] fopen({path:?},{mode:?}) -> {real:?} = {}",
            if f.is_null() { "NULL" } else { "ok" }
        );
    }
    f
}

unsafe fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        CStr::from_ptr(p).to_string_lossy().into_owned()
    }
}
pub unsafe fn wstr(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut v = Vec::new();
    let mut i = 0;
    loop {
        let c = *p.add(i);
        if c == 0 || i > 32768 {
            break;
        }
        v.push(c);
        i += 1;
    }
    String::from_utf16_lossy(&v)
}

extern "C" fn my_fopen(path: *const c_char, mode: *const c_char) -> *mut libc::FILE {
    unsafe { open(&cstr(path), &cstr(mode)) }
}
extern "C" fn my_fopen_s(
    out: *mut *mut libc::FILE,
    path: *const c_char,
    mode: *const c_char,
) -> c_int {
    let f = unsafe { open(&cstr(path), &cstr(mode)) };
    unsafe { *out = f };
    if f.is_null() {
        2
    } else {
        0
    }
}
extern "C" fn my_wfsopen(path: *const u16, mode: *const u16, _sh: c_int) -> *mut libc::FILE {
    unsafe { open(&wstr(path), &wstr(mode)) }
}
extern "C" fn my_wfopen_s(
    out: *mut *mut libc::FILE,
    path: *const u16,
    mode: *const u16,
) -> c_int {
    let f = unsafe { open(&wstr(path), &wstr(mode)) };
    unsafe { *out = f };
    if f.is_null() {
        2
    } else {
        0
    }
}
extern "C" fn my_fseeki64(f: *mut libc::FILE, off: i64, whence: c_int) -> c_int {
    unsafe { libc::fseek(f, off as libc::c_long, whence) }
}
extern "C" fn my_ftelli64(f: *mut libc::FILE) -> i64 {
    unsafe { libc::ftell(f) as i64 }
}
extern "C" fn my_filelength(fd: c_int) -> c_int {
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(fd, &mut st) } != 0 {
        return -1;
    }
    st.st_size as c_int
}
static mut IOB: [*mut libc::FILE; 3] = [std::ptr::null_mut(); 3];
extern "C" fn my_iob(which: u32) -> *mut libc::FILE {
    let i = (which as usize).min(2);
    unsafe {
        let slot = std::ptr::addr_of_mut!(IOB) as *mut *mut libc::FILE;
        if (*slot.add(i)).is_null() {
            let (fd, md): (c_int, &[u8]) = match i {
                0 => (0, b"r\0"),
                1 => (1, b"w\0"),
                _ => (2, b"w\0"),
            };
            *slot.add(i) = libc::fdopen(fd, md.as_ptr() as *const c_char);
        }
        *slot.add(i)
    }
}
extern "C" fn my_fgetpos(f: *mut libc::FILE, pos: *mut i64) -> c_int {
    unsafe { *pos = libc::ftell(f) as i64 };
    0
}
extern "C" fn my_fsetpos(f: *mut libc::FILE, pos: *const i64) -> c_int {
    unsafe { libc::fseek(f, *pos as libc::c_long, libc::SEEK_SET) }
}
extern "C" fn my_fgetwc(f: *mut libc::FILE) -> u32 {
    // The engine writes/reads UTF-16LE text files, so a wide char is two raw bytes.
    let lo = unsafe { libc::fgetc(f) };
    if lo < 0 {
        return 0xffff;
    }
    let hi = unsafe { libc::fgetc(f) };
    if hi < 0 {
        return 0xffff;
    }
    ((hi as u32) << 8) | (lo as u32 & 0xff)
}
extern "C" fn my_ungetwc(c: u32, f: *mut libc::FILE) -> u32 {
    unsafe {
        libc::ungetc(((c >> 8) & 0xff) as c_int, f);
        libc::ungetc((c & 0xff) as c_int, f);
    }
    c
}
extern "C" fn my_fputwc(c: u32, f: *mut libc::FILE) -> u32 {
    unsafe {
        libc::fputc((c & 0xff) as c_int, f);
        libc::fputc(((c >> 8) & 0xff) as c_int, f);
    }
    c
}
extern "C" fn my_fgetws(buf: *mut u16, n: c_int, f: *mut libc::FILE) -> *mut u16 {
    let mut i = 0isize;
    while i < (n as isize) - 1 {
        let c = my_fgetwc(f);
        if c == 0xffff {
            break;
        }
        unsafe { *buf.offset(i) = c as u16 };
        i += 1;
        if c == 10 {
            break;
        }
    }
    if i == 0 {
        return std::ptr::null_mut();
    }
    unsafe { *buf.offset(i) = 0 };
    buf
}
extern "C" fn my_fputws(s: *const u16, f: *mut libc::FILE) -> c_int {
    let mut i = 0;
    unsafe {
        while *s.add(i) != 0 {
            my_fputwc(*s.add(i) as u32, f);
            i += 1;
        }
    }
    0
}

// ---------------------------------------------------------------------------------
// printf family
// ---------------------------------------------------------------------------------

extern "C" {
    fn vsnprintf(s: *mut c_char, n: usize, fmt: *const c_char, ap: *mut c_void) -> c_int;
    fn vfprintf(f: *mut libc::FILE, fmt: *const c_char, ap: *mut c_void) -> c_int;
}

extern "C" fn common_vfprintf(
    _o_lo: u32,
    _o_hi: u32,
    f: *mut libc::FILE,
    fmt: *const c_char,
    _loc: u32,
    ap: *mut c_void,
) -> c_int {
    unsafe { vfprintf(f, fmt, ap) }
}
extern "C" fn common_vsprintf(
    _o_lo: u32,
    _o_hi: u32,
    buf: *mut c_char,
    count: usize,
    fmt: *const c_char,
    _loc: u32,
    ap: *mut c_void,
) -> c_int {
    let n = if count == usize::MAX { 1 << 20 } else { count };
    unsafe { vsnprintf(buf, n, fmt, ap) }
}
extern "C" fn common_vsnprintf_s(
    _o_lo: u32,
    _o_hi: u32,
    buf: *mut c_char,
    count: usize,
    maxcount: usize,
    fmt: *const c_char,
    _loc: u32,
    ap: *mut c_void,
) -> c_int {
    let n = count.min(maxcount.saturating_add(1));
    unsafe { vsnprintf(buf, n, fmt, ap) }
}

// ---------------------------------------------------------------------------------
// Narrow string helpers with Microsoft's `_s` signatures
// ---------------------------------------------------------------------------------

extern "C" fn my_strnlen(s: *const c_char, n: usize) -> usize {
    let mut i = 0;
    unsafe {
        while i < n && *s.add(i) != 0 {
            i += 1;
        }
    }
    i
}
extern "C" fn my_strcpy_s(dst: *mut c_char, n: usize, src: *const c_char) -> c_int {
    let mut i = 0;
    unsafe {
        while i + 1 < n && *src.add(i) != 0 {
            *dst.add(i) = *src.add(i);
            i += 1;
        }
        if n > 0 {
            *dst.add(i) = 0;
        }
    }
    0
}
extern "C" fn my_strncpy_s(
    dst: *mut c_char,
    n: usize,
    src: *const c_char,
    count: usize,
) -> c_int {
    let mut i = 0;
    unsafe {
        while i + 1 < n && i < count && *src.add(i) != 0 {
            *dst.add(i) = *src.add(i);
            i += 1;
        }
        if n > 0 {
            *dst.add(i) = 0;
        }
    }
    0
}
extern "C" fn my_strncat_s(
    dst: *mut c_char,
    n: usize,
    src: *const c_char,
    count: usize,
) -> c_int {
    unsafe {
        let mut d = 0;
        while d < n && *dst.add(d) != 0 {
            d += 1;
        }
        let mut i = 0;
        while d + 1 < n && i < count && *src.add(i) != 0 {
            *dst.add(d) = *src.add(i);
            d += 1;
            i += 1;
        }
        if d < n {
            *dst.add(d) = 0;
        }
    }
    0
}
extern "C" fn my_strtok_s(
    s: *mut c_char,
    delim: *const c_char,
    ctx: *mut *mut c_char,
) -> *mut c_char {
    unsafe {
        let mut p = if s.is_null() { *ctx } else { s };
        if p.is_null() {
            return std::ptr::null_mut();
        }
        while *p != 0 && !libc::strchr(delim, *p as c_int).is_null() {
            p = p.add(1);
        }
        if *p == 0 {
            *ctx = p;
            return std::ptr::null_mut();
        }
        let start = p;
        while *p != 0 && libc::strchr(delim, *p as c_int).is_null() {
            p = p.add(1);
        }
        if *p != 0 {
            *p = 0;
            p = p.add(1);
        }
        *ctx = p;
        start
    }
}
extern "C" fn my_i64toa_s(v: i64, buf: *mut c_char, n: usize, radix: c_int) -> c_int {
    let s = match radix {
        16 => format!("{:x}", v),
        8 => format!("{:o}", v),
        2 => format!("{:b}", v),
        _ => format!("{}", v),
    };
    let b = s.as_bytes();
    unsafe {
        for (i, ch) in b.iter().enumerate() {
            if i + 1 >= n {
                break;
            }
            *buf.add(i) = *ch as c_char;
        }
        *buf.add(b.len().min(n.saturating_sub(1))) = 0;
    }
    0
}
extern "C" fn my_gcvt_s(buf: *mut c_char, n: usize, v: f64, digits: c_int) -> c_int {
    let s = format!("{:.*}", digits.clamp(0, 17) as usize, v);
    let b = s.as_bytes();
    unsafe {
        for (i, ch) in b.iter().enumerate() {
            if i + 1 >= n {
                break;
            }
            *buf.add(i) = *ch as c_char;
        }
        *buf.add(b.len().min(n.saturating_sub(1))) = 0;
    }
    0
}

// ---------------------------------------------------------------------------------
// UTF-16 string helpers
// ---------------------------------------------------------------------------------

unsafe fn wlen(p: *const u16) -> usize {
    let mut i = 0;
    while *p.add(i) != 0 {
        i += 1;
    }
    i
}
fn lc(c: u16) -> u16 {
    if (65..=90).contains(&c) {
        c + 32
    } else {
        c
    }
}

extern "C" fn w_wcschr(s: *const u16, c: u16) -> *const u16 {
    unsafe {
        let mut i = 0;
        loop {
            let v = *s.add(i);
            if v == c {
                return s.add(i);
            }
            if v == 0 {
                return std::ptr::null();
            }
            i += 1;
        }
    }
}
extern "C" fn w_wcsrchr(s: *const u16, c: u16) -> *const u16 {
    unsafe {
        let n = wlen(s);
        let mut i = n as isize;
        while i >= 0 {
            if *s.offset(i) == c {
                return s.offset(i);
            }
            i -= 1;
        }
        std::ptr::null()
    }
}
extern "C" fn w_wcsstr(h: *const u16, n: *const u16) -> *const u16 {
    unsafe {
        let ln = wlen(n);
        if ln == 0 {
            return h;
        }
        let lh = wlen(h);
        if ln > lh {
            return std::ptr::null();
        }
        for i in 0..=(lh - ln) {
            let mut k = 0;
            while k < ln && *h.add(i + k) == *n.add(k) {
                k += 1;
            }
            if k == ln {
                return h.add(i);
            }
        }
        std::ptr::null()
    }
}
extern "C" fn w_wcsncmp(a: *const u16, b: *const u16, n: usize) -> c_int {
    unsafe {
        for i in 0..n {
            let (x, y) = (*a.add(i), *b.add(i));
            if x != y {
                return x as c_int - y as c_int;
            }
            if x == 0 {
                return 0;
            }
        }
        0
    }
}
extern "C" fn w_wcsicmp(a: *const u16, b: *const u16) -> c_int {
    unsafe {
        let mut i = 0;
        loop {
            let (x, y) = (lc(*a.add(i)), lc(*b.add(i)));
            if x != y {
                return x as c_int - y as c_int;
            }
            if x == 0 {
                return 0;
            }
            i += 1;
        }
    }
}
extern "C" fn w_wcsnicmp(a: *const u16, b: *const u16, n: usize) -> c_int {
    unsafe {
        for i in 0..n {
            let (x, y) = (lc(*a.add(i)), lc(*b.add(i)));
            if x != y {
                return x as c_int - y as c_int;
            }
            if x == 0 {
                return 0;
            }
        }
        0
    }
}
extern "C" fn w_wcscspn(s: *const u16, set: *const u16) -> usize {
    unsafe {
        let mut i = 0;
        loop {
            let c = *s.add(i);
            if c == 0 {
                return i;
            }
            if !w_wcschr(set, c).is_null() {
                return i;
            }
            i += 1;
        }
    }
}
extern "C" fn w_wcscpy_s(dst: *mut u16, n: usize, src: *const u16) -> c_int {
    unsafe {
        let mut i = 0;
        while i + 1 < n && *src.add(i) != 0 {
            *dst.add(i) = *src.add(i);
            i += 1;
        }
        if n > 0 {
            *dst.add(i) = 0;
        }
    }
    0
}
extern "C" fn w_wcscat_s(dst: *mut u16, n: usize, src: *const u16) -> c_int {
    unsafe {
        let mut d = 0;
        while d < n && *dst.add(d) != 0 {
            d += 1;
        }
        let mut i = 0;
        while d + 1 < n && *src.add(i) != 0 {
            *dst.add(d) = *src.add(i);
            d += 1;
            i += 1;
        }
        if d < n {
            *dst.add(d) = 0;
        }
    }
    0
}
extern "C" fn w_wcsncpy_s(dst: *mut u16, n: usize, src: *const u16, count: usize) -> c_int {
    unsafe {
        let mut i = 0;
        while i + 1 < n && i < count && *src.add(i) != 0 {
            *dst.add(i) = *src.add(i);
            i += 1;
        }
        if n > 0 {
            *dst.add(i) = 0;
        }
    }
    0
}
extern "C" fn w_towupper(c: u32) -> u32 {
    if (97..=122).contains(&c) {
        c - 32
    } else {
        c
    }
}
extern "C" fn w_towlower(c: u32) -> u32 {
    if (65..=90).contains(&c) {
        c + 32
    } else {
        c
    }
}
extern "C" fn w_iswspace(c: u32) -> c_int {
    matches!(c, 0x20 | 0x09 | 0x0a | 0x0b | 0x0c | 0x0d) as c_int
}

unsafe fn w_to_string(p: *const u16) -> String {
    wstr(p)
}
extern "C" fn w_wtoi(s: *const u16) -> c_int {
    unsafe { w_to_string(s).trim().parse::<i32>().unwrap_or(0) }
}
extern "C" fn w_wtoi64(s: *const u16) -> i64 {
    unsafe { w_to_string(s).trim().parse::<i64>().unwrap_or(0) }
}
extern "C" fn w_wcstod(s: *const u16, end: *mut *const u16) -> f64 {
    let txt = unsafe { w_to_string(s) };
    let t = txt.trim_start();
    let mut n = 0;
    let b = t.as_bytes();
    if n < b.len() && (b[n] == b'+' || b[n] == b'-') {
        n += 1;
    }
    while n < b.len() && (b[n].is_ascii_digit() || b[n] == b'.') {
        n += 1;
    }
    let lead = txt.len() - t.len();
    if !end.is_null() {
        unsafe { *end = s.add(lead + n) };
    }
    t[..n].parse::<f64>().unwrap_or(0.0)
}
fn wparse_int(s: *const u16, end: *mut *const u16, base: u32) -> i64 {
    let txt = unsafe { w_to_string(s) };
    let t = txt.trim_start();
    let b = t.as_bytes();
    let mut n = 0;
    if n < b.len() && (b[n] == b'+' || b[n] == b'-') {
        n += 1;
    }
    let base = if base == 0 { 10 } else { base };
    while n < b.len() && (b[n] as char).is_digit(base) {
        n += 1;
    }
    let lead = txt.len() - t.len();
    if !end.is_null() {
        unsafe { *end = s.add(lead + n) };
    }
    i64::from_str_radix(&t[..n], base).unwrap_or(0)
}
extern "C" fn w_wcstol(s: *const u16, end: *mut *const u16, base: u32) -> i32 {
    wparse_int(s, end, base) as i32
}
extern "C" fn w_wcstoul(s: *const u16, end: *mut *const u16, base: u32) -> u32 {
    wparse_int(s, end, base) as u32
}
extern "C" fn w_wcstoui64(s: *const u16, end: *mut *const u16, base: u32) -> u64 {
    wparse_int(s, end, base) as u64
}
fn wwrite(buf: *mut u16, n: usize, s: &str) -> c_int {
    let u: Vec<u16> = s.encode_utf16().collect();
    unsafe {
        for (i, c) in u.iter().enumerate() {
            if i + 1 >= n {
                break;
            }
            *buf.add(i) = *c;
        }
        *buf.add(u.len().min(n.saturating_sub(1))) = 0;
    }
    0
}
extern "C" fn w_itow_s(v: c_int, buf: *mut u16, n: usize, radix: c_int) -> c_int {
    let s = match radix {
        16 => format!("{:x}", v),
        8 => format!("{:o}", v),
        2 => format!("{:b}", v),
        _ => format!("{}", v),
    };
    wwrite(buf, n, &s)
}
extern "C" fn w_ultow_s(v: u32, buf: *mut u16, n: usize, radix: c_int) -> c_int {
    let s = match radix {
        16 => format!("{:x}", v),
        8 => format!("{:o}", v),
        2 => format!("{:b}", v),
        _ => format!("{}", v),
    };
    wwrite(buf, n, &s)
}
extern "C" fn w_ui64tow_s(v: u64, buf: *mut u16, n: usize, radix: c_int) -> c_int {
    let s = match radix {
        16 => format!("{:x}", v),
        8 => format!("{:o}", v),
        2 => format!("{:b}", v),
        _ => format!("{}", v),
    };
    wwrite(buf, n, &s)
}
extern "C" fn w_mbstowcs(dst: *mut u16, src: *const c_char, n: usize) -> usize {
    unsafe {
        let s = cstr(src);
        let u: Vec<u16> = s.encode_utf16().collect();
        if dst.is_null() {
            return u.len();
        }
        let k = u.len().min(n);
        for i in 0..k {
            *dst.add(i) = u[i];
        }
        if k < n {
            *dst.add(k) = 0;
        }
        k
    }
}
extern "C" fn w_wcstombs_s(
    ret: *mut usize,
    dst: *mut c_char,
    n: usize,
    src: *const u16,
    count: usize,
) -> c_int {
    let s = unsafe { wstr(src) };
    let b = s.as_bytes();
    let k = b.len().min(count).min(n.saturating_sub(1));
    unsafe {
        if !dst.is_null() {
            for i in 0..k {
                *dst.add(i) = b[i] as c_char;
            }
            *dst.add(k) = 0;
        }
        if !ret.is_null() {
            *ret = k + 1;
        }
    }
    0
}
extern "C" fn w_mbtowc(dst: *mut u16, src: *const c_char, _n: usize) -> c_int {
    unsafe {
        if src.is_null() {
            return 0;
        }
        let c = *src as u8;
        if !dst.is_null() {
            *dst = c as u16;
        }
        if c == 0 {
            0
        } else {
            1
        }
    }
}
extern "C" fn w_wctomb_s(ret: *mut c_int, dst: *mut c_char, n: usize, c: u16) -> c_int {
    unsafe {
        if !dst.is_null() && n > 0 {
            *dst = c as c_char;
        }
        if !ret.is_null() {
            *ret = 1;
        }
    }
    0
}
