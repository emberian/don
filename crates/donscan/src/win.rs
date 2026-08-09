//! Raw kernel32 FFI. No `windows`/`windows-sys` dependency on purpose: the whole
//! crate must cross-link from arm64 macOS with nothing but lld-link and the MSVC
//! import libraries that cargo-xwin fetches.

#![allow(non_snake_case, non_camel_case_types, dead_code)]

use std::ffi::c_void;

pub type HANDLE = isize;
pub const INVALID_HANDLE_VALUE: HANDLE = -1;

pub const PROCESS_QUERY_INFORMATION: u32 = 0x0400;
pub const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
pub const PROCESS_VM_READ: u32 = 0x0010;

pub const MEM_COMMIT: u32 = 0x0000_1000;
pub const MEM_PRIVATE: u32 = 0x0002_0000;
pub const MEM_MAPPED: u32 = 0x0004_0000;
pub const MEM_IMAGE: u32 = 0x0100_0000;

pub const PAGE_NOACCESS: u32 = 0x01;
pub const PAGE_READONLY: u32 = 0x02;
pub const PAGE_READWRITE: u32 = 0x04;
pub const PAGE_WRITECOPY: u32 = 0x08;
pub const PAGE_EXECUTE: u32 = 0x10;
pub const PAGE_EXECUTE_READ: u32 = 0x20;
pub const PAGE_EXECUTE_READWRITE: u32 = 0x40;
pub const PAGE_EXECUTE_WRITECOPY: u32 = 0x80;
pub const PAGE_GUARD: u32 = 0x100;

pub const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;

/// x64/arm64 layout of MEMORY_BASIC_INFORMATION (48 bytes). We are always a
/// 64-bit process; the *target* may be 32-bit, which VirtualQueryEx handles.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct MemoryBasicInformation {
    pub base_address: u64,
    pub allocation_base: u64,
    pub allocation_protect: u32,
    pub _align1: u32,
    pub region_size: u64,
    pub state: u32,
    pub protect: u32,
    pub typ: u32,
    pub _align2: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ProcessEntry32W {
    pub dwSize: u32,
    pub cntUsage: u32,
    pub th32ProcessID: u32,
    pub th32DefaultHeapID: usize,
    pub th32ModuleID: u32,
    pub cntThreads: u32,
    pub th32ParentProcessID: u32,
    pub pcPriClassBase: i32,
    pub dwFlags: u32,
    pub szExeFile: [u16; 260],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FileTime {
    pub low: u32,
    pub high: u32,
}

impl Default for ProcessEntry32W {
    fn default() -> Self {
        // SAFETY: all fields are plain integers / integer arrays.
        unsafe { std::mem::zeroed() }
    }
}

#[link(name = "kernel32")]
extern "system" {
    pub fn OpenProcess(dwDesiredAccess: u32, bInheritHandle: i32, dwProcessId: u32) -> HANDLE;
    pub fn CloseHandle(h: HANDLE) -> i32;
    pub fn GetLastError() -> u32;
    pub fn ReadProcessMemory(
        hProcess: HANDLE,
        lpBaseAddress: *const c_void,
        lpBuffer: *mut c_void,
        nSize: usize,
        lpNumberOfBytesRead: *mut usize,
    ) -> i32;
    pub fn VirtualQueryEx(
        hProcess: HANDLE,
        lpAddress: *const c_void,
        lpBuffer: *mut MemoryBasicInformation,
        dwLength: usize,
    ) -> usize;
    pub fn CreateToolhelp32Snapshot(dwFlags: u32, th32ProcessID: u32) -> HANDLE;
    pub fn Process32FirstW(hSnapshot: HANDLE, lppe: *mut ProcessEntry32W) -> i32;
    pub fn Process32NextW(hSnapshot: HANDLE, lppe: *mut ProcessEntry32W) -> i32;
    pub fn IsWow64Process(hProcess: HANDLE, Wow64Process: *mut i32) -> i32;
    pub fn GetProcessTimes(
        hProcess: HANDLE,
        lpCreationTime: *mut FileTime,
        lpExitTime: *mut FileTime,
        lpKernelTime: *mut FileTime,
        lpUserTime: *mut FileTime,
    ) -> i32;
    pub fn QueryFullProcessImageNameW(
        hProcess: HANDLE,
        dwFlags: u32,
        lpExeName: *mut u16,
        lpdwSize: *mut u32,
    ) -> i32;
}

pub fn protect_is_readable(protect: u32) -> bool {
    if protect & PAGE_GUARD != 0 {
        return false;
    }
    matches!(
        protect & 0xff,
        PAGE_READONLY
            | PAGE_READWRITE
            | PAGE_WRITECOPY
            | PAGE_EXECUTE_READ
            | PAGE_EXECUTE_READWRITE
            | PAGE_EXECUTE_WRITECOPY
    )
}

pub fn region_type_name(typ: u32) -> &'static str {
    match typ {
        MEM_PRIVATE => "private",
        MEM_MAPPED => "mapped",
        MEM_IMAGE => "image",
        _ => "other",
    }
}

fn wide_to_string(w: &[u16]) -> String {
    let n = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..n])
}

/// (pid, exe name) for every process the caller can see.
pub fn list_processes() -> Vec<(u32, String)> {
    let mut out = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut pe = ProcessEntry32W::default();
        pe.dwSize = std::mem::size_of::<ProcessEntry32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut pe);
        while ok != 0 {
            out.push((pe.th32ProcessID, wide_to_string(&pe.szExeFile)));
            pe.dwSize = std::mem::size_of::<ProcessEntry32W>() as u32;
            ok = Process32NextW(snap, &mut pe);
        }
        CloseHandle(snap);
    }
    out
}

pub struct Proc {
    pub handle: HANDLE,
    pub pid: u32,
}

impl Proc {
    pub fn open(pid: u32) -> Result<Proc, u32> {
        let h = unsafe {
            OpenProcess(
                PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
                0,
                pid,
            )
        };
        let h = if h == 0 {
            // Fall back to the limited right, which suffices for
            // VirtualQueryEx + ReadProcessMemory on modern Windows.
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid) }
        } else {
            h
        };
        if h == 0 {
            Err(unsafe { GetLastError() })
        } else {
            Ok(Proc { handle: h, pid })
        }
    }

    pub fn query(&self, addr: u64) -> Option<MemoryBasicInformation> {
        let mut mbi = MemoryBasicInformation::default();
        let n = unsafe {
            VirtualQueryEx(
                self.handle,
                addr as *const c_void,
                &mut mbi,
                std::mem::size_of::<MemoryBasicInformation>(),
            )
        };
        if n == 0 {
            None
        } else {
            Some(mbi)
        }
    }

    /// Reads into `buf[..len]`. Returns bytes actually read (0 on failure).
    pub fn read(&self, addr: u64, buf: &mut [u8]) -> usize {
        let mut got: usize = 0;
        let ok = unsafe {
            ReadProcessMemory(
                self.handle,
                addr as *const c_void,
                buf.as_mut_ptr() as *mut c_void,
                buf.len(),
                &mut got,
            )
        };
        if ok == 0 {
            // A partial read still fills the prefix; ReadProcessMemory reports
            // the count even when it returns FALSE on the trailing page.
            got
        } else {
            got
        }
    }

    /// Windows creation time in 100 ns ticks since 1601. Together with PID this
    /// distinguishes process reuse across game restarts.
    pub fn creation_time_100ns(&self) -> Option<u64> {
        let mut creation = FileTime::default();
        let mut exit = FileTime::default();
        let mut kernel = FileTime::default();
        let mut user = FileTime::default();
        let ok = unsafe {
            GetProcessTimes(
                self.handle,
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )
        };
        (ok != 0).then_some(((creation.high as u64) << 32) | creation.low as u64)
    }

    pub fn image_path(&self) -> Option<String> {
        let mut path = vec![0u16; 32_768];
        let mut len = path.len() as u32;
        let ok = unsafe { QueryFullProcessImageNameW(self.handle, 0, path.as_mut_ptr(), &mut len) };
        if ok == 0 || len == 0 || len as usize > path.len() {
            None
        } else {
            Some(String::from_utf16_lossy(&path[..len as usize]))
        }
    }
}

impl crate::live::Mem for Proc {
    fn read(&self, addr: u64, buf: &mut [u8]) -> usize {
        Proc::read(self, addr, buf)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ModuleInfo {
    pub base: u64,
    pub machine: u16,
    pub entry_rva: u32,
    pub size_of_image: u32,
}

fn read_u16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn read_u32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn module_header(p: &Proc, base: u64) -> Option<ModuleInfo> {
    let mut hdr = vec![0u8; 0x1000];
    let n = p.read(base, &mut hdr);
    if n < 0x200 || hdr.get(0..2) != Some(b"MZ") {
        return None;
    }
    let pe = read_u32(&hdr, 0x3c) as usize;
    let opt = pe.checked_add(0x18)?;
    if opt.checked_add(0x3c)? > n || hdr.get(pe..pe + 4) != Some(b"PE\0\0") {
        return None;
    }
    Some(ModuleInfo {
        base,
        machine: read_u16(&hdr, pe + 4),
        entry_rva: read_u32(&hdr, opt + 0x10),
        size_of_image: read_u32(&hdr, opt + 0x38),
    })
}

/// Find the supported retail image by PE fields the loader does not rewrite.
pub fn find_game_image(p: &Proc) -> Option<ModuleInfo> {
    let mut addr = 0u64;
    let mut seen_allocation = None;
    while let Some(mbi) = p.query(addr) {
        if mbi.region_size == 0 {
            break;
        }
        if mbi.typ == MEM_IMAGE
            && mbi.state == MEM_COMMIT
            && Some(mbi.allocation_base) != seen_allocation
        {
            seen_allocation = Some(mbi.allocation_base);
            if let Some(module) = module_header(p, mbi.allocation_base) {
                if module.machine == 0x14c
                    && module.entry_rva == crate::live::SOURCE_ENTRY_RVA
                    && module.size_of_image == crate::live::SOURCE_IMAGE_SIZE
                {
                    return Some(module);
                }
            }
        }
        let next = mbi.base_address.saturating_add(mbi.region_size);
        if next <= addr || next >= 0x7fff_fffe_0000 {
            break;
        }
        addr = next;
    }
    None
}

impl Drop for Proc {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
        }
    }
}
