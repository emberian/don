//! Mapping the retail image, calling into it, and identifying it.
//!
//! Split out of `main.rs` so that every consumer — the regression runner, the ad-hoc
//! `call` command, the RNG probe — maps the image exactly one way. A second copy of this
//! code is a second set of section protections, and a differential result is only as
//! trustworthy as the mapping it ran against.

use don_pe::PeImage;
use std::ffi::c_void;

pub const PAGE: usize = 4096;

pub fn round_up(v: usize, to: usize) -> usize {
    (v + to - 1) & !(to - 1)
}

pub struct Mapped {
    pub base: *mut u8,
    pub len: usize,
}

impl Mapped {
    /// Map the image anywhere the kernel likes, relocate it to that address, then set
    /// per-section protections.
    pub fn load(bytes: &[u8]) -> Result<(Mapped, PeImage), String> {
        let pe = PeImage::parse(bytes).map_err(|e| e.to_string())?;
        if !pe.has_relocations() {
            return Err("image has no relocations; cannot map at an arbitrary base".into());
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
        eprintln!("[oracle] mapped at {actual:#010x}, {fixups} relocations applied");

        for s in &pe.sections {
            let start = s.virtual_address as usize;
            let size = round_up(s.virtual_size.max(1) as usize, PAGE);
            if start + size > len {
                continue;
            }
            let prot = if s.is_executable() {
                libc::PROT_READ | libc::PROT_EXEC
            } else if s.is_writable() {
                libc::PROT_READ | libc::PROT_WRITE
            } else {
                libc::PROT_READ
            };
            let rc = unsafe { libc::mprotect(base.add(start) as *mut c_void, size, prot) };
            if rc != 0 {
                return Err(format!("mprotect {} failed", s.name));
            }
        }
        Ok((Mapped { base, len }, pe))
    }

    pub fn addr_of_rva(&self, rva: u32) -> *mut u8 {
        unsafe { self.base.add(rva as usize) }
    }

    /// Make one page writable. Used by the tokenizer case, which patches two IAT slots
    /// that live in a read-only section; the alternative — mapping the whole image
    /// writable — would silently change the environment every other case runs in.
    pub fn make_page_writable(&self, addr: *mut u8) -> Result<(), String> {
        let page = (addr as usize) & !(PAGE - 1);
        let rc = unsafe {
            libc::mprotect(page as *mut c_void, PAGE, libc::PROT_READ | libc::PROT_WRITE)
        };
        if rc != 0 {
            return Err(format!("mprotect rw at {page:#x} failed"));
        }
        Ok(())
    }
}

impl Drop for Mapped {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.base as *mut c_void, self.len) };
    }
}

/// Run `f` in a forked child. Returns Ok(value) if the child exited cleanly, or a
/// description of the signal that killed it. This is what makes probing unknown
/// functions safe.
pub fn in_child<F: FnOnce() -> u32>(f: F) -> Result<u32, String> {
    let mut fds = [0i32; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err("pipe failed".into());
    }
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err("fork failed".into());
    }
    if pid == 0 {
        unsafe { libc::close(fds[0]) };
        let v = f();
        let bytes = v.to_le_bytes();
        unsafe { libc::write(fds[1], bytes.as_ptr() as *const c_void, 4) };
        unsafe { libc::_exit(0) };
    }
    unsafe { libc::close(fds[1]) };
    let mut buf = [0u8; 4];
    let n = unsafe { libc::read(fds[0], buf.as_mut_ptr() as *mut c_void, 4) };
    unsafe { libc::close(fds[0]) };
    let mut status = 0i32;
    unsafe { libc::waitpid(pid, &mut status, 0) };

    if libc::WIFSIGNALED(status) {
        let sig = libc::WTERMSIG(status);
        return Err(format!("child killed by signal {sig}"));
    }
    if n != 4 {
        return Err("child produced no value".into());
    }
    Ok(u32::from_le_bytes(buf))
}

/// Validate the mmap/mprotect/call mechanism using machine code we wrote ourselves, so a
/// failure here is unambiguously the harness and not the retail image.
///
/// The regression runner calls this **first** and refuses to report any case as passing
/// if it fails: a broken harness that reports zero mismatches is precisely the vacuous
/// green this suite exists to prevent.
pub fn selftest() -> Result<(), String> {
    // mov eax,[esp+4]; add eax,[esp+8]; ret   -- cdecl add
    const CODE: [u8; 9] = [0x8B, 0x44, 0x24, 0x04, 0x03, 0x44, 0x24, 0x08, 0xC3];
    let p = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            PAGE,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    if p == libc::MAP_FAILED {
        return Err("selftest mmap failed".into());
    }
    unsafe { std::ptr::copy_nonoverlapping(CODE.as_ptr(), p as *mut u8, CODE.len()) };
    if unsafe { libc::mprotect(p, PAGE, libc::PROT_READ | libc::PROT_EXEC) } != 0 {
        return Err("selftest mprotect failed".into());
    }
    let f: extern "C" fn(u32, u32) -> u32 = unsafe { std::mem::transmute(p) };
    let got = in_child(|| f(40, 2))?;
    unsafe { libc::munmap(p, PAGE) };
    if got != 42 {
        return Err(format!("selftest returned {got}, expected 42"));
    }
    Ok(())
}

/// MSVC x86 functions with an SEH frame execute `mov eax, fs:[0]`. On i386 Linux TLS is
/// in `%gs` and `%fs` is a null selector, so any `fs:` access faults in the prologue.
/// Installing a page as the `%fs` base makes `fs:[0]` ordinary writable memory.
///
/// Only cases that need it call this, and they call it **inside their forked child**, so
/// the segment change cannot leak into any other case's environment.
pub fn install_fake_teb() -> Result<(), String> {
    #[repr(C)]
    struct UserDesc {
        entry_number: u32,
        base_addr: u32,
        limit: u32,
        flags: u32,
    }
    const SYS_SET_THREAD_AREA: libc::c_long = 243;
    let page = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            PAGE,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    if page == libc::MAP_FAILED {
        return Err("TEB mmap failed".into());
    }
    // fs:[0] = end-of-SEH-chain sentinel, exactly as Windows initialises it.
    unsafe { std::ptr::write_volatile(page as *mut u32, 0xffff_ffff) };
    let mut d = UserDesc {
        entry_number: u32::MAX, // let the kernel pick a free GDT slot
        base_addr: page as u32,
        limit: 0x000f_ffff,
        // seg_32bit(0) | limit_in_pages(4) | useable(6); data, read/write, present
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
    Ok(())
}

// ---------------------------------------------------------------------------------
// SHA-256, FIPS 180-4. Present so the JSON records the hash of the bytes this process
// actually mapped, rather than a hash somebody typed in alongside it.
// ---------------------------------------------------------------------------------

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
    0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
    0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
    0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
    0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
    0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
    0xc67178f2,
];

pub fn sha256_hex(data: &[u8]) -> String {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let bitlen = (data.len() as u64).wrapping_mul(8);

    let mut block = [0u8; 64];
    let compress = |h: &mut [u32; 8], b: &[u8; 64]| {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([b[i * 4], b[i * 4 + 1], b[i * 4 + 2], b[i * 4 + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b_, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b_) ^ (a & c) ^ (b_ & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b_;
            b_ = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b_);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    };

    let full = data.len() / 64;
    for i in 0..full {
        block.copy_from_slice(&data[i * 64..i * 64 + 64]);
        compress(&mut h, &block);
    }
    let rem = &data[full * 64..];
    let mut tail = [0u8; 128];
    tail[..rem.len()].copy_from_slice(rem);
    tail[rem.len()] = 0x80;
    let tail_len = if rem.len() + 9 <= 64 { 64 } else { 128 };
    tail[tail_len - 8..tail_len].copy_from_slice(&bitlen.to_be_bytes());
    for i in 0..tail_len / 64 {
        block.copy_from_slice(&tail[i * 64..i * 64 + 64]);
        compress(&mut h, &block);
    }

    let mut out = String::with_capacity(64);
    for v in h {
        out.push_str(&format!("{v:08x}"));
    }
    out
}
