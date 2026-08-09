//! Mapping the retail image and making it survivable to execute.
//!
//! Deliberately a copy of the shape used by `crates/oracle/src/image.rs` (that crate is
//! owned by another lane and must not be edited). What is *new* here:
//!
//! * the fake TEB carries a TLS array, a stack range and a self-pointer, not just the SEH
//!   sentinel — the script module is full of `__declspec(thread)`-free but SEH-heavy code
//!   and MSVC also reads `fs:[0x2c]`;
//! * `.text` is mapped RWX because the IAT lives inside `.rdata` and the trampolines we
//!   install for unimplemented imports must be executable;
//! * a fault sandbox (`guard`) that catches SIGSEGV/SIGBUS/SIGILL/SIGFPE and reports the
//!   faulting EIP plus the dereferenced address, so a probe that cannot run produces a
//!   *diagnosis* rather than a dead child.

use don_pe::PeImage;
use std::ffi::c_void;

pub const PAGE: usize = 4096;
pub const IMAGE_BASE: u32 = 0x0040_0000;

pub fn round_up(v: usize, to: usize) -> usize {
    (v + to - 1) & !(to - 1)
}

pub struct Mapped {
    pub base: *mut u8,
    pub len: usize,
    pub pe: PeImage,
}

impl Mapped {
    pub fn load(bytes: &[u8]) -> Result<Mapped, String> {
        let pe = PeImage::parse(bytes).map_err(|e| e.to_string())?;
        if !pe.has_relocations() {
            return Err("image has no relocations".into());
        }
        let len = round_up(pe.size_of_image as usize, PAGE);
        let base = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if base == libc::MAP_FAILED {
            return Err(format!("mmap failed: {}", std::io::Error::last_os_error()));
        }
        let base = base as *mut u8;
        let image = pe.map(bytes).map_err(|e| e.to_string())?;
        unsafe { std::ptr::copy_nonoverlapping(image.as_ptr(), base, image.len()) };

        let actual = base as usize;
        if actual > u32::MAX as usize {
            return Err(format!("mapping landed above 4GiB at {actual:#x}"));
        }
        let slice = unsafe { std::slice::from_raw_parts_mut(base, len) };
        let fixups = pe.relocate(slice, actual as u32).map_err(|e| e.to_string())?;
        eprintln!("[bhs] mapped at {actual:#010x}, {fixups} relocations applied");

        // Everything RWX. The IAT sits inside .rdata and we rewrite it; the import
        // trampolines we synthesise are placed in a separate page, but retail code also
        // writes into .data constantly and a wrong protection here shows up as a fault
        // that looks like a missing global.
        let rc = unsafe {
            libc::mprotect(
                base as *mut c_void,
                len,
                libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
            )
        };
        if rc != 0 {
            return Err("mprotect RWX failed".into());
        }
        Ok(Mapped { base, len, pe })
    }

    /// Static VA (image-base 0x00400000) -> live address.
    #[inline]
    pub fn at(&self, va: u32) -> *mut u8 {
        assert!(va >= IMAGE_BASE, "VA {va:#x} below image base");
        let rva = (va - IMAGE_BASE) as usize;
        assert!(rva < self.len, "VA {va:#x} outside the mapped image");
        unsafe { self.base.add(rva) }
    }

    #[inline]
    pub fn contains(&self, va: u32) -> bool {
        va >= IMAGE_BASE && ((va - IMAGE_BASE) as usize) < self.len
    }

    /// Live address -> static VA, for reporting a fault against the PDB.
    #[inline]
    pub fn va_of(&self, p: usize) -> Option<u32> {
        let b = self.base as usize;
        if p >= b && p < b + self.len {
            Some((p - b) as u32 + IMAGE_BASE)
        } else {
            None
        }
    }

    pub fn read_u32(&self, va: u32) -> u32 {
        unsafe { std::ptr::read_unaligned(self.at(va) as *const u32) }
    }
    pub fn write_u32(&self, va: u32, v: u32) {
        unsafe { std::ptr::write_unaligned(self.at(va) as *mut u32, v) }
    }
}

// ---------------------------------------------------------------------------------
// Fake TEB
// ---------------------------------------------------------------------------------

pub static mut TEB_BASE: u32 = 0;

/// Install a `%fs` segment whose base is a fabricated TEB.
///
/// `fs:[0x00]` SEH chain head, `fs:[0x04]` stack base, `fs:[0x08]` stack limit,
/// `fs:[0x18]` self, `fs:[0x2c]` thread-local storage array, `fs:[0x30]` PEB.
/// The TLS array's slot 0 points at a private copy of the image's `.tls` template, and
/// the image's TLS index cell is forced to 0.
pub fn install_fake_teb(m: Option<&Mapped>) -> Result<(), String> {
    #[repr(C)]
    struct UserDesc {
        entry_number: u32,
        base_addr: u32,
        limit: u32,
        flags: u32,
    }
    const SYS_SET_THREAD_AREA: libc::c_long = 243;

    let teb = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            PAGE * 4,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    if teb == libc::MAP_FAILED {
        return Err("TEB mmap failed".into());
    }
    let teb = teb as *mut u8;
    let w = |off: usize, v: u32| unsafe { std::ptr::write_unaligned(teb.add(off) as *mut u32, v) };

    // TLS: 64 slots, slot 0 = a copy of the image .tls template.
    let tls_array = unsafe { teb.add(PAGE) };
    let tls_block = unsafe { teb.add(PAGE * 2) };
    if let Some(m) = m {
        // .tls raw data 0x00ee3000..0x00ee3fb0 [measured from the PE TLS directory]
        let src = m.at(0x00ee_3000);
        unsafe { std::ptr::copy_nonoverlapping(src, tls_block, 0xfb0) };
        // TLS index cell 0x00caa2e0 [measured] -> slot 0
        m.write_u32(0x00ca_a2e0, 0);
    }
    unsafe { std::ptr::write_unaligned(tls_array as *mut u32, tls_block as u32) };

    w(0x00, 0xffff_ffff); // SEH chain terminator
    w(0x04, 0xc000_0000); // StackBase  (nonsense but non-null; nothing dereferences it)
    w(0x08, 0x0001_0000); // StackLimit
    w(0x18, teb as u32); // Self
    w(0x2c, tls_array as u32); // ThreadLocalStoragePointer
    w(0x30, unsafe { teb.add(PAGE * 3) } as u32); // PEB
    w(0x34, 0); // LastErrorValue

    let mut d = UserDesc {
        entry_number: u32::MAX,
        base_addr: teb as u32,
        limit: 0x000f_ffff,
        flags: 0x51,
    };
    let rc = unsafe { libc::syscall(SYS_SET_THREAD_AREA, &mut d as *mut UserDesc) };
    if rc != 0 {
        return Err(format!(
            "set_thread_area failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    let sel: u16 = ((d.entry_number << 3) | 3) as u16;
    unsafe { std::arch::asm!("mov fs, {0:x}", in(reg) sel, options(nomem, nostack)) };
    unsafe { TEB_BASE = teb as u32 };
    Ok(())
}

pub fn seh_head() -> u32 {
    unsafe { std::ptr::read_volatile(TEB_BASE as *const u32) }
}
pub fn set_seh_head(v: u32) {
    unsafe { std::ptr::write_volatile(TEB_BASE as *mut u32, v) }
}

// ---------------------------------------------------------------------------------
// Fault sandbox
// ---------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct Fault {
    pub signo: i32,
    pub eip: u32,
    pub addr: u32,
    pub eax: u32,
    pub ecx: u32,
    pub edx: u32,
    pub ebx: u32,
    pub esi: u32,
    pub edi: u32,
    pub esp: u32,
    pub ebp: u32,
}

extern "C" {
    fn sigsetjmp(env: *mut c_void, savemask: i32) -> i32;
    fn siglongjmp(env: *mut c_void, val: i32) -> !;
}

const JB: usize = 256;
static mut JMPBUF: [u8; JB] = [0u8; JB];
static mut ARMED: bool = false;
static mut LAST: Fault = Fault {
    signo: 0,
    eip: 0,
    addr: 0,
    eax: 0,
    ecx: 0,
    edx: 0,
    ebx: 0,
    esi: 0,
    edi: 0,
    esp: 0,
    ebp: 0,
};

// i386 sigcontext offsets inside ucontext_t [uc_flags 0, uc_link 4, uc_stack 8..20,
// uc_mcontext 20..]. Within sigcontext: 8 x u16 segment regs (16 bytes) then
// edi, esi, ebp, esp, ebx, edx, ecx, eax, trapno, err, eip.
const MC: usize = 20;
const O_EDI: usize = MC + 16;
const O_ESI: usize = MC + 20;
const O_EBP: usize = MC + 24;
const O_ESP: usize = MC + 28;
const O_EBX: usize = MC + 32;
const O_EDX: usize = MC + 36;
const O_ECX: usize = MC + 40;
const O_EAX: usize = MC + 44;
const O_EIP: usize = MC + 56;

// ---------------------------------------------------------------------------------
// Single-step tracer
// ---------------------------------------------------------------------------------
//
// There is no debugger on the far side of a fault into retail code, and a stack scan only
// finds *pushed* return addresses — useless when control transferred through a garbage
// indirect target and left no frame. Setting TF and recording %eip on every SIGTRAP gives
// the exact instruction sequence that led into the fault, which is the difference between
// "jumped to 0x993aed03" and "jumped to 0x993aed03 from <named retail function>".

const O_EFL: usize = MC + 64;
pub const TRACE_RING: usize = 512;
pub static mut TRACE: [u32; TRACE_RING] = [0; TRACE_RING];
pub static mut TRACE_N: u64 = 0;
pub static mut TRACE_LIMIT: u64 = 0;

pub fn trace_on(limit: u64) {
    unsafe {
        TRACE_N = 0;
        TRACE_LIMIT = limit;
        std::arch::asm!("pushfd", "or dword ptr [esp], 0x100", "popfd");
    }
}
pub fn trace_off() {
    unsafe {
        TRACE_LIMIT = 0;
        std::arch::asm!("pushfd", "and dword ptr [esp], 0xfffffeff", "popfd");
    }
}
/// The last `n` instruction pointers, oldest first.
pub fn trace_tail(n: usize) -> Vec<u32> {
    unsafe {
        let total = TRACE_N as usize;
        let k = n.min(total).min(TRACE_RING);
        (0..k)
            .map(|i| TRACE[(total - k + i) % TRACE_RING])
            .collect()
    }
}

/// Argument watchpoints. Patching a `jmp` over a retail prologue would work but needs a
/// hand-built trampoline for whatever instructions it displaced; with the stepper already
/// running, comparing `%eip` costs nothing and cannot corrupt the image.
pub static mut WATCH: [u32; 4] = [0; 4];
pub const WATCH_MAX: usize = 64;
/// Per hit: `[which, eax, ecx, edx, ebx, esi, edi, esp, [esp+4], [esp+8], [esp+12]]`.
pub static mut WATCH_LOG: [[u32; 11]; WATCH_MAX] = [[0; 11]; WATCH_MAX];
pub static mut WATCH_HITS: usize = 0;

unsafe extern "C" fn step_handler(_s: i32, _i: *mut libc::siginfo_t, uc: *mut c_void) {
    let eip = std::ptr::read_unaligned((uc as *const u8).add(O_EIP) as *const u32);
    TRACE[(TRACE_N as usize) % TRACE_RING] = eip;
    TRACE_N += 1;
    for w in 0..4 {
        if WATCH[w] != 0 && WATCH[w] == eip && WATCH_HITS < WATCH_MAX {
            let g = |o: usize| std::ptr::read_unaligned((uc as *const u8).add(o) as *const u32);
            let esp = g(O_ESP);
            let rd = |off: u32| std::ptr::read_unaligned((esp + off) as *const u32);
            WATCH_LOG[WATCH_HITS] = [
                w as u32, g(O_EAX), g(O_ECX), g(O_EDX), g(O_EBX), g(O_ESI), g(O_EDI), esp,
                rd(4), rd(8), rd(12),
            ];
            WATCH_HITS += 1;
        }
    }
    if TRACE_N >= TRACE_LIMIT {
        let p = (uc as *mut u8).add(O_EFL) as *mut u32;
        std::ptr::write_unaligned(p, std::ptr::read_unaligned(p) & !0x100);
    }
}

unsafe extern "C" fn handler(signo: i32, info: *mut libc::siginfo_t, uc: *mut c_void) {
    // Stop stepping before we do anything that would flood the ring.
    {
        let p = (uc as *mut u8).add(O_EFL) as *mut u32;
        std::ptr::write_unaligned(p, std::ptr::read_unaligned(p) & !0x100);
        TRACE_LIMIT = 0;
    }
    let rd = |off: usize| std::ptr::read_unaligned((uc as *const u8).add(off) as *const u32);
    let addr = if info.is_null() {
        0
    } else {
        std::ptr::read_unaligned((info as *const u8).add(12) as *const u32)
    };
    LAST = Fault {
        signo,
        eip: rd(O_EIP),
        addr,
        eax: rd(O_EAX),
        ecx: rd(O_ECX),
        edx: rd(O_EDX),
        ebx: rd(O_EBX),
        esi: rd(O_ESI),
        edi: rd(O_EDI),
        esp: rd(O_ESP),
        ebp: rd(O_EBP),
    };
    if ARMED {
        ARMED = false;
        siglongjmp(std::ptr::addr_of_mut!(JMPBUF) as *mut c_void, 1);
    }
    libc::_exit(90);
}

pub fn install_fault_handler() {
    unsafe {
        // Alternate stack, so a stack-overflow fault is still reportable.
        let ss_sp = libc::mmap(
            std::ptr::null_mut(),
            PAGE * 16,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        );
        let ss = libc::stack_t {
            ss_sp,
            ss_flags: 0,
            ss_size: PAGE * 16,
        };
        libc::sigaltstack(&ss, std::ptr::null_mut());

        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = handler as usize;
        sa.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK | libc::SA_NODEFER;
        libc::sigemptyset(&mut sa.sa_mask);
        for s in [
            libc::SIGSEGV,
            libc::SIGBUS,
            libc::SIGILL,
            libc::SIGFPE,
            libc::SIGALRM,
        ] {
            libc::sigaction(s, &sa, std::ptr::null_mut());
        }

        let mut st: libc::sigaction = std::mem::zeroed();
        st.sa_sigaction = step_handler as usize;
        st.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK | libc::SA_NODEFER;
        libc::sigemptyset(&mut st.sa_mask);
        libc::sigaction(libc::SIGTRAP, &st, std::ptr::null_mut());
    }
}

fn set_timer(secs: u32) {
    let it = libc::itimerval {
        it_interval: libc::timeval {
            tv_sec: 0,
            tv_usec: 0,
        },
        it_value: libc::timeval {
            tv_sec: secs as libc::time_t,
            tv_usec: 0,
        },
    };
    unsafe { libc::setitimer(libc::ITIMER_REAL, &it, std::ptr::null_mut()) };
}

/// `guard` plus a wall-clock deadline. A retail function that spins forever under a
/// half-built environment is indistinguishable from a hang of the harness itself, which
/// is exactly the failure mode `README-LLM.md` warns about, so every call has a deadline.
pub fn guard_for<F: FnOnce()>(secs: u32, f: F) -> Result<(), Fault> {
    set_timer(secs);
    let r = guard(f);
    set_timer(0);
    r
}

/// Run `f`; if it faults, return the fault instead of dying.
///
/// The SEH chain head is saved and restored around the call: a retail function that
/// faults never runs its epilogue, so `fs:[0]` would otherwise be left pointing into a
/// dead frame and the *next* probe would fault for a reason that is not its own.
pub fn guard<F: FnOnce()>(f: F) -> Result<(), Fault> {
    unsafe {
        let saved_seh = if TEB_BASE != 0 { seh_head() } else { 0 };
        let rc = sigsetjmp(std::ptr::addr_of_mut!(JMPBUF) as *mut c_void, 1);
        if rc == 0 {
            ARMED = true;
            f();
            ARMED = false;
            Ok(())
        } else {
            if TEB_BASE != 0 {
                set_seh_head(saved_seh);
            }
            Err(LAST)
        }
    }
}

pub fn signame(s: i32) -> &'static str {
    match s {
        libc::SIGSEGV => "SIGSEGV",
        libc::SIGBUS => "SIGBUS",
        libc::SIGILL => "SIGILL",
        libc::SIGFPE => "SIGFPE",
        libc::SIGALRM => "TIMEOUT",
        -1 => "ESCAPE",
        _ => "SIG?",
    }
}

/// Abandon the current guarded call from *inside* retail code — used by an import that we
/// have not implemented, and by `abort`/`exit`/`_purecall`.
///
/// A trap that called `_exit` would take the whole sweep down on the first unimplemented
/// import, which turns a shopping list into a single line. Escaping instead lets one run
/// enumerate every initialiser that needs something we do not have.
pub fn escape() -> ! {
    unsafe {
        LAST = Fault {
            signo: -1,
            eip: 0,
            addr: 0,
            eax: 0,
            ecx: 0,
            edx: 0,
            ebx: 0,
            esi: 0,
            edi: 0,
            esp: 0,
            ebp: 0,
        };
        if ARMED {
            ARMED = false;
            siglongjmp(std::ptr::addr_of_mut!(JMPBUF) as *mut c_void, 1);
        }
        libc::_exit(77)
    }
}
