//! Binary oracle harness: map riseofnations.exe and call retail functions directly.
//!
//! Must run as a 32-bit x86 process (i686-unknown-linux-musl). Each call happens in a
//! forked child so that probing an unknown function cannot take down the harness --
//! essential when we do not yet know which functions are reachable with fabricated
//! inputs (see the ISLAND / SELF-CALL / DATA-ONLY taxonomy in docs/oracle-architecture.md).

mod damage_env;
mod damage_test;

use don_pe::PeImage;
use std::ffi::c_void;

const PAGE: usize = 4096;

fn round_up(v: usize, to: usize) -> usize {
    (v + to - 1) & !(to - 1)
}

struct Mapped {
    base: *mut u8,
    len: usize,
}

impl Mapped {
    /// Map the image anywhere the kernel likes, relocate it to that address, then set
    /// per-section protections.
    fn load(bytes: &[u8]) -> Result<(Mapped, PeImage), String> {
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

    fn addr_of_rva(&self, rva: u32) -> *const u8 {
        unsafe { self.base.add(rva as usize) }
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
fn in_child<F: FnOnce() -> u32>(f: F) -> Result<u32, String> {
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
fn selftest() -> Result<(), String> {
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
    println!("selftest: OK (hand-written cdecl add returned 42 through fork isolation)");
    trampoline_selftest()?;
    Ok(())
}

/// Prove `oracle_call4` against machine code we wrote ourselves, in all four shapes the
/// sweep must survive: __cdecl (caller pops), __stdcall (callee pops), __thiscall (ecx),
/// and an x87 float return. The stdcall case is the one that matters most -- if esp is
/// not restored from ebp the harness corrupts its own stack on every callee-pops
/// candidate, and the sweep would report garbage for a large fraction of the image
/// without ever crashing.
fn trampoline_selftest() -> Result<(), String> {
    // Laid out one after another in a single page.
    //   cdecl_add:   mov eax,[esp+4]; add eax,[esp+8]; ret
    //   stdcall_mul: mov eax,[esp+4]; imul eax,[esp+8]; ret 8
    //   thiscall:    mov eax,[ecx+4]; ret
    //   float:       fld dword ptr [esp+4]; ret
    let blobs: [&[u8]; 4] = [
        &[0x8B, 0x44, 0x24, 0x04, 0x03, 0x44, 0x24, 0x08, 0xC3],
        &[0x8B, 0x44, 0x24, 0x04, 0x0F, 0xAF, 0x44, 0x24, 0x08, 0xC2, 0x08, 0x00],
        &[0x8B, 0x41, 0x04, 0xC3],
        &[0xD9, 0x44, 0x24, 0x04, 0xC3],
    ];
    let code = unsafe {
        libc::mmap(std::ptr::null_mut(), PAGE, libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS, -1, 0)
    };
    if code == libc::MAP_FAILED {
        return Err("trampoline selftest mmap failed".into());
    }
    let mut at = [0usize; 4];
    let mut off = 0usize;
    for (i, b) in blobs.iter().enumerate() {
        at[i] = code as usize + off;
        unsafe { std::ptr::copy_nonoverlapping(b.as_ptr(), (code as *mut u8).add(off), b.len()) };
        off += b.len() + 8;
    }
    if unsafe { libc::mprotect(code, PAGE, libc::PROT_READ | libc::PROT_EXEC) } != 0 {
        return Err("trampoline selftest mprotect failed".into());
    }
    let arena = unsafe {
        libc::mmap(std::ptr::null_mut(), ARENA, libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS, -1, 0) as *mut u8
    };
    if arena as isize == -1 {
        return Err("trampoline selftest arena mmap failed".into());
    }

    let want_f32: f32 = -1234.5;
    let cases: [(usize, [u32; 4], u32); 3] = [
        (at[0], [40, 2, 0, 0], 42),
        (at[1], [7, 6, 0, 0], 42),
        (at[3], [want_f32.to_bits(), 0, 0, 0], 0),
    ];
    for (i, (f, args, want)) in cases.iter().enumerate() {
        match probe_calls(*f as *const u8, arena, Fill::Zero, *args, 3, 300) {
            ProbeEnd::Ok(v) => {
                if i < 2 && v[0].eax != *want {
                    return Err(format!("trampoline case {i}: got {} want {want}", v[0].eax));
                }
                if i < 2 && v[0].has_float != 0 {
                    return Err(format!("trampoline case {i}: spurious float return"));
                }
                if i == 2 {
                    if v[0].has_float == 0 {
                        return Err("trampoline float case: st(0) return not detected".into());
                    }
                    if v[0].fret as f32 != want_f32 {
                        return Err(format!("trampoline float case: got {} want {want_f32}", v[0].fret));
                    }
                }
                // Three repeats through one child must agree, or the intra-process
                // determinism signal the sweep reports is meaningless.
                if v.iter().any(|o| o.eax != v[0].eax) {
                    return Err(format!("trampoline case {i}: repeats disagreed"));
                }
            }
            _ => return Err(format!("trampoline case {i}: probe did not return")),
        }
    }
    // __thiscall: the arena is zero-filled except for one word we plant at this+4.
    unsafe { std::ptr::write_bytes(arena, 0, ARENA) };
    match probe_calls(at[2] as *const u8, arena, Fill::Garbage(0x0102_0304_0506_0708, 0), [0; 4], 1, 300) {
        ProbeEnd::Ok(v) => {
            // Fill::Garbage writes u64 slot k = seed*(k+1); this = arena+ARENA_MID, so
            // the dword at this+4 is the high half of slot ARENA_MID/8.
            let k = (ARENA_MID / 8) as u64;
            let want = (0x0102_0304_0506_0708u64.wrapping_mul(k + 1) >> 32) as u32;
            if v[0].eax != want {
                return Err(format!("trampoline thiscall: got {:#x} want {want:#x}", v[0].eax));
            }
        }
        _ => return Err("trampoline thiscall: probe did not return".into()),
    }
    // The timeout guard itself must work, or the sweep can wedge again.
    let spin: [u8; 2] = [0xEB, 0xFE]; // jmp $
    let sp = unsafe {
        libc::mmap(std::ptr::null_mut(), PAGE, libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS, -1, 0)
    };
    unsafe { std::ptr::copy_nonoverlapping(spin.as_ptr(), sp as *mut u8, 2) };
    unsafe { libc::mprotect(sp, PAGE, libc::PROT_READ | libc::PROT_EXEC) };
    let t = std::time::Instant::now();
    let spun = probe_calls(sp as *const u8, arena, Fill::Zero, [0; 4], 1, 300);
    if !matches!(spun, ProbeEnd::Timeout) {
        return Err("trampoline: an infinite loop was not classified as a timeout".into());
    }
    if t.elapsed().as_secs_f64() > 2.0 {
        return Err(format!("trampoline: timeout took {:.1}s, guard too slow", t.elapsed().as_secs_f64()));
    }
    // And a wild jump must come back as a signal, not take down the harness.
    if !matches!(probe_calls(4 as *const u8, arena, Fill::Zero, [0; 4], 1, 300), ProbeEnd::Signal(_)) {
        return Err("trampoline: a call to an unmapped address was not classified as a fault".into());
    }
    unsafe {
        libc::munmap(code, PAGE);
        libc::munmap(sp, PAGE);
        libc::munmap(arena as *mut c_void, ARENA);
    }
    println!("selftest: OK (trampoline: cdecl, stdcall callee-pops, thiscall, x87 float return, timeout guard, fault guard)");
    Ok(())
}

/// Call a `__thiscall` function: `this` in ECX, result in EAX.
///
/// Rust has no stable `extern "thiscall"` on this target, and hand-rolling the call keeps
/// the ABI contract visible rather than hidden behind a feature gate.
unsafe fn call_thiscall(f: *const u8, this: *mut u8) -> u32 {
    let ret: u32;
    std::arch::asm!(
        "call {f}",
        f = in(reg) f,
        in("ecx") this,
        lateout("eax") ret,
        clobber_abi("C"),
    );
    ret
}

/// SUPERSEDED by the `regress` binary and `oracle::registry` -- delete once the sweep work
/// in this file settles.
///
/// The reason it must go, not merely be left alone: its models are **inline copies**. The
/// `0x00846450` model here is a lambda, not `don_sim::hash_into_range`, so this test can
/// only tell you that the copy in this file is right. `don-sim` could drift arbitrarily and
/// this would still print PASS. The registry points every case at the shipped function
/// instead; `regress --only hash_into_range,accessor_movsx_word_0xa,accessor_diff_0x12c_0x12a`
/// is the replacement.
///
/// Differential test: retail machine code versus a Rust model of the same computation,
/// over N pseudo-random inputs. This is Tier B evidence (see docs/CHARTER.md) -- testing,
/// not proof -- so the sample count is reported with the result and never omitted.
fn difftest(m: &Mapped, pe: &PeImage, trials: u32) -> bool {
    let mut all_ok_outer = true;
    // A tiny reproducible PRNG; no external crates, and the seed is fixed so a failing
    // case can be replayed exactly.
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };

    // Scratch object the getters read from. 1 KiB is ample for the offsets involved.
    let obj = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            PAGE,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        ) as *mut u8
    };

    struct Case {
        va: u32,
        label: &'static str,
        // writes the inputs, returns what the retail code must produce
        model: fn(*mut u8, u64) -> u32,
    }

    fn model_getter_0xa(obj: *mut u8, r: u64) -> u32 {
        let v = r as u16;
        unsafe { std::ptr::write_unaligned(obj.add(0x0A) as *mut u16, v) };
        (v as i16) as i32 as u32
    }
    fn model_diff_12c_12a(obj: *mut u8, r: u64) -> u32 {
        let a = r as u16;
        let b = (r >> 16) as u16;
        unsafe {
            std::ptr::write_unaligned(obj.add(0x12C) as *mut u16, a);
            std::ptr::write_unaligned(obj.add(0x12A) as *mut u16, b);
        }
        ((a as i16) as i32).wrapping_sub((b as i16) as i32) as u32
    }

    let cases = [
        Case { va: 0x0047_2400, label: "movsx eax,[this+0xA]", model: model_getter_0xa },
        Case { va: 0x0048_F770, label: "[this+0x12C] - [this+0x12A]", model: model_diff_12c_12a },
    ];

    // ---- stdcall, 4 integer args, no memory: ((a*a*b) % (|hi-lo|+1)) + lo ----
    {
        let f = m.addr_of_rva(0x0084_6450 - pe.image_base);
        let g: extern "stdcall" fn(i32, i32, i32, i32) -> i32 = unsafe { std::mem::transmute(f) };
        let model = |a: i32, b: i32, lo: i32, hi: i32| -> i32 {
            let divisor = hi.wrapping_sub(lo).wrapping_abs().wrapping_add(1);
            a.wrapping_mul(a).wrapping_mul(b).wrapping_rem(divisor).wrapping_add(lo)
        };
        let mut mism = 0u32;
        let mut first: Option<(i32, i32, i32, i32, i32, i32)> = None;
        let mut n = 0u32;
        // fixed edge cases first, then random
        let edges: [(i32, i32, i32, i32); 8] = [
            (0, 0, 0, 0), (1, 1, 0, 1), (-1, -1, -5, 5), (i32::MAX, 1, 0, 10),
            (i32::MIN, 1, 0, 10), (7, -3, -100, 100), (123456, 789, 0, 0), (5, 5, 10, -10),
        ];
        for (a, b, lo, hi) in edges {
            let expect = model(a, b, lo, hi);
            let got = g(a, b, lo, hi);
            n += 1;
            if got != expect { mism += 1; if first.is_none() { first = Some((a,b,lo,hi,expect,got)); } }
        }
        for _ in 0..trials {
            let r = next(); let q = next();
            let (a, b) = (r as i32, (r >> 32) as i32);
            let (lo, hi) = (q as i32 >> 16, (q >> 32) as i32 >> 16);
            let expect = model(a, b, lo, hi);
            let got = g(a, b, lo, hi);
            n += 1;
            if got != expect { mism += 1; if first.is_none() { first = Some((a,b,lo,hi,expect,got)); } }
        }
        if mism == 0 {
            println!("  PASS  0x00846450  {:<32} {} trials, 0 mismatches", "((a*a*b) % (|hi-lo|+1)) + lo", n);
        } else {
            all_ok_outer = false;
            let (a,b,lo,hi,e,go) = first.unwrap();
            println!("  FAIL  0x00846450  {}/{} mismatched; first a={a} b={b} lo={lo} hi={hi} expect={e} got={go}", mism, n);
        }
    }

    let mut all_ok = true;
    for c in &cases {
        let f = m.addr_of_rva(c.va - pe.image_base);
        let mut mismatches = 0u32;
        let mut first_bad = None;
        for _ in 0..trials {
            let r = next();
            let expect = (c.model)(obj, r);
            let got = unsafe { call_thiscall(f, obj) };
            if got != expect {
                mismatches += 1;
                if first_bad.is_none() {
                    first_bad = Some((r, expect, got));
                }
            }
        }
        if mismatches == 0 {
            println!("  PASS  {:#010x}  {:<32} {} trials, 0 mismatches", c.va, c.label, trials);
        } else {
            all_ok = false;
            println!(
                "  FAIL  {:#010x}  {:<32} {}/{} mismatched, first: input={:#x} expect={:#x} got={:#x}",
                c.va,
                c.label,
                mismatches,
                trials,
                first_bad.unwrap().0,
                first_bad.unwrap().1,
                first_bad.unwrap().2
            );
        }
    }
    unsafe { libc::munmap(obj as *mut c_void, PAGE) };
    all_ok && all_ok_outer
}


/// SUPERSEDED by the `regress` binary and `oracle::registry` -- delete once the sweep work
/// in this file settles. Same defect as `difftest` above: the flank model is an inline
/// lambda and the balance model is raw pointer arithmetic, so neither exercises
/// `don_sim::flank_level` or `don_sim::balance_index`. Replacement:
/// `regress --only flank_level,balance_accessor`.
///
/// Combat-lane differential cases (docs/derivation/combat.md).
///
/// Two retail entry points from the damage pipeline that are callable with fabricated
/// inputs, so they can carry Tier-B evidence:
///   * `0x0092CFE0` -- flank-level classifier, ISLAND, single ECX argument. The whole
///     32-bit input domain is enumerable in chunks; we sample it densely.
///   * `0x00581CA0` -- balance-table lookup, `__stdcall(attacker_type, defender_type)`,
///     reads `int16 balance[atk*493 + def]` at VA 0x00C06AFC. Exhaustive over the entire
///     defined type domain (493x493 = 243,049 pairs).
fn combat_difftest(m: &Mapped, pe: &PeImage, trials: u32) -> bool {
    let mut ok = true;

    // ---- flank level: __fastcall-ish, argument in ECX ----
    {
        let f = m.addr_of_rva(0x0092_CFE0 - pe.image_base);
        let call = |x: u32| -> u32 {
            let r: u32;
            unsafe {
                std::arch::asm!("call {f}", f = in(reg) f,
                    in("ecx") x, lateout("eax") r, clobber_abi("C"));
            }
            r
        };
        // Model transcribed from the seven instructions at 0x0092CFE0.
        let model = |x: u32| -> u32 {
            if x > 0xD555_5555 {
                0
            } else if 0x4000_0000u32 < x.wrapping_sub(0x6000_0000) {
                2
            } else {
                1
            }
        };
        let mut n = 0u32;
        let mut mism = 0u32;
        let mut first = None;
        let probe = |x: u32, n: &mut u32, mism: &mut u32, first: &mut Option<(u32, u32, u32)>| {
            let e = model(x);
            let g = call(x);
            *n += 1;
            if e != g && first.is_none() {
                *first = Some((x, e, g));
            }
            if e != g {
                *mism += 1;
            }
        };
        // every boundary the instruction sequence can distinguish, plus neighbours
        for b in [
            0u32,
            1,
            0x3FFF_FFFF,
            0x4000_0000,
            0x4000_0001,
            0x5FFF_FFFF,
            0x6000_0000,
            0x6000_0001,
            0x9FFF_FFFF,
            0xA000_0000,
            0xA000_0001,
            0xD555_5554,
            0xD555_5555,
            0xD555_5556,
            0xFFFF_FFFF,
            0x8000_0000,
            0x7FFF_FFFF,
        ] {
            probe(b, &mut n, &mut mism, &mut first);
        }
        // dense sweep of the whole 32-bit domain on a stride that is coprime with 2^32
        let stride: u32 = 8191;
        let mut x: u32 = 0;
        for _ in 0..trials.max(500_000) {
            probe(x, &mut n, &mut mism, &mut first);
            x = x.wrapping_add(stride);
        }
        if mism == 0 {
            println!("  PASS  0x0092cfe0  {:<38} {} trials, 0 mismatches", "flank_level(angle_delta)", n);
        } else {
            ok = false;
            let (x, e, g) = first.unwrap();
            println!("  FAIL  0x0092cfe0  flank_level: {mism}/{n} mismatched; first x={x:#x} expect={e} got={g}");
        }
    }

    // ---- balance table lookup: __stdcall(atk_type, def_type) -> i32 ----
    {
        let f = m.addr_of_rva(0x0058_1CA0 - pe.image_base);
        let g: extern "stdcall" fn(i32, i32) -> i32 = unsafe { std::mem::transmute(f) };
        const TABLE_VA: u32 = 0x00C0_6AFC;
        const STRIDE: i32 = 493;
        let table = m.addr_of_rva(TABLE_VA - pe.image_base);
        let model = |a: i32, b: i32| -> i32 {
            let idx = a.wrapping_mul(STRIDE).wrapping_add(b);
            let p = unsafe { (table as *const i16).offset(idx as isize) };
            (unsafe { std::ptr::read_unaligned(p) }) as i32
        };
        let mut n = 0u32;
        let mut mism = 0u32;
        let mut first = None;
        // exhaustive over the whole defined type domain
        for a in 0..STRIDE {
            for b in 0..STRIDE {
                let e = model(a, b);
                let got = g(a, b);
                n += 1;
                if e != got {
                    mism += 1;
                    if first.is_none() {
                        first = Some((a, b, e, got));
                    }
                }
            }
        }
        // plus indices past the type domain, still inside the mapped image, to exercise
        // the multiply/add without a bounds check
        for a in 500..1500 {
            for b in [0, 1, 100, 492] {
                let e = model(a, b);
                let got = g(a, b);
                n += 1;
                if e != got {
                    mism += 1;
                    if first.is_none() {
                        first = Some((a, b, e, got));
                    }
                }
            }
        }
        if mism == 0 {
            println!("  PASS  0x00581ca0  {:<38} {} trials, 0 mismatches", "balance[atk*493 + def] (int16)", n);
        } else {
            ok = false;
            let (a, b, e, got) = first.unwrap();
            println!("  FAIL  0x00581ca0  balance: {mism}/{n} mismatched; first a={a} b={b} expect={e} got={got}");
        }

        // report what the table actually contains, since the file image at this VA looks
        // like unrelated static data -- see docs/derivation/combat.md
        let mut hist = std::collections::BTreeMap::new();
        for a in 0..STRIDE {
            for b in 0..STRIDE {
                *hist.entry(g(a, b)).or_insert(0u32) += 1;
            }
        }
        let mut v: Vec<_> = hist.into_iter().collect();
        v.sort_by_key(|&(_, c)| std::cmp::Reverse(c));
        println!("  balance-table value histogram (top 12 of {} distinct): {:?}", v.len(), &v[..v.len().min(12)]);
    }
    ok
}

/// Result of one retail call, captured at the machine level.
///
/// `eax`/`edx` are the integer return pair. 32-bit MSVC returns `float`/`double` in
/// `st(0)`, not in a register, so the trampoline resets the x87 stack before the call and
/// checks the TOP field afterwards: a non-zero TOP means the callee left a value there,
/// which is the machine-level signature of a floating-point return. Knowing which ISLANDs
/// return floats matters because floats are the whole IEEE hazard surface of this project.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq)]
struct CallOut {
    eax: u32,
    edx: u32,
    fsw: u16,
    has_float: u16,
    _pad: u32,
    fret: f64,
}

// A trampoline rather than a transmuted fn pointer, for three reasons that each cost a
// debugging session if ignored:
//   * calling convention is UNKNOWN per candidate. __cdecl leaves args on the stack,
//     __stdcall/__thiscall pop them. Restoring esp from ebp makes the probe correct for
//     all three instead of corrupting the harness stack on every stdcall candidate.
//   * ecx must carry `this` for __thiscall while the same four dwords sit on the stack
//     for __cdecl, so one probe covers both shapes.
//   * a hostile callee may trash ebx/esi/edi; we save them ourselves.
core::arch::global_asm!(
    ".text",
    ".globl oracle_call4",
    ".hidden oracle_call4",
    ".type oracle_call4,@function",
    "oracle_call4:",
    "    push ebp",
    "    mov ebp, esp",
    "    push ebx",
    "    push esi",
    "    push edi",
    "    fninit",                      // x87 TOP = 0, all registers empty
    "    mov eax, [ebp+16]",           // args: *const [u32; 4]
    "    push dword ptr [eax+12]",
    "    push dword ptr [eax+8]",
    "    push dword ptr [eax+4]",
    "    push dword ptr [eax]",
    "    mov ecx, [ebp+12]",           // this
    "    call dword ptr [ebp+8]",      // f
    "    mov esi, [ebp+20]",           // out: *mut CallOut (esp may be anything here)
    "    mov [esi], eax",
    "    mov [esi+4], edx",
    "    fnstsw ax",
    "    mov [esi+8], ax",
    "    test ax, 0x3800",             // TOP != 0 -> the callee pushed an x87 value
    "    jz 2f",
    "    mov word ptr [esi+10], 1",
    "    fstp qword ptr [esi+16]",
    "    jmp 3f",
    "2:  mov word ptr [esi+10], 0",
    "3:  lea esp, [ebp-12]",
    "    pop edi",
    "    pop esi",
    "    pop ebx",
    "    pop ebp",
    "    ret",
);

extern "C" {
    /// Call `f` with `this` in ecx and four dwords on the stack, capturing eax/edx/st(0).
    fn oracle_call4(f: *const u8, this: u32, args: *const u32, out: *mut CallOut);
}

/// How a probe ended.
enum ProbeEnd {
    /// The child returned `n` call results cleanly.
    Ok(Vec<CallOut>),
    /// Killed by a signal (SIGSEGV etc.) -- the function needs state we did not fabricate.
    Signal(i32),
    /// Did not terminate inside the budget -- an unbounded loop over the garbage buffer.
    Timeout,
    /// Exited without producing output.
    NoValue,
}

/// The `this` arena: one contiguous mapping, with `this` pointing at its middle so a
/// callee reading `[ecx-N]` is still inside mapped memory.
const ARENA: usize = 1 << 20;
const ARENA_MID: usize = ARENA / 2;

/// Fill patterns for the arena. Every pattern is reproducible from a seed, so any finding
/// here can be replayed exactly with `oracle call`.
#[derive(Clone, Copy, PartialEq)]
enum Fill {
    /// Pseudorandom garbage. Pointer fields become wild addresses, so anything that
    /// dereferences its `this` faults -- which is itself the signal we want.
    Garbage(u64, u64),
    /// All zero. Exercises the null-pointer path, which for a large class of member
    /// functions is a clean early return rather than a fault.
    Zero,
}

fn fill_arena(obj: *mut u8, fill: Fill) {
    unsafe {
        match fill {
            Fill::Zero => std::ptr::write_bytes(obj, 0, ARENA),
            Fill::Garbage(sa, sb) => {
                for i in 0..ARENA / 8 {
                    std::ptr::write_unaligned(
                        (obj as *mut u64).add(i),
                        sa.wrapping_mul(i as u64 + 1) ^ sb,
                    );
                }
            }
        }
    }
}

/// Run `reps` identical calls to `f` inside one forked child and report every result.
///
/// Two independent guards, because one is not enough: an interval timer for wall-clock
/// (a function blocked in a syscall), and RLIMIT_CPU as a backstop (a function that
/// masks or outruns signals). The parent additionally polls with its own deadline and
/// SIGKILLs, so a child that manages to survive both cannot wedge the sweep the way the
/// untimed version did -- it burned 9.5 hours of CPU on one function.
fn probe_calls(
    f: *const u8,
    obj: *mut u8,
    fill: Fill,
    args: [u32; 4],
    reps: usize,
    budget_ms: u32,
) -> ProbeEnd {
    let mut fds = [0i32; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return ProbeEnd::NoValue;
    }
    let nbytes = reps * std::mem::size_of::<CallOut>();
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
        }
        return ProbeEnd::NoValue;
    }
    if pid == 0 {
        unsafe {
            libc::close(fds[0]);
            // No files, no children: a probed function cannot write to the tree or fork
            // a grandchild that would hold the pipe open past our own death.
            let z = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
            libc::setrlimit(libc::RLIMIT_FSIZE, &z);
            let one = libc::rlimit { rlim_cur: 1, rlim_max: 1 };
            libc::setrlimit(libc::RLIMIT_NPROC, &one);
            let cpu = libc::rlimit { rlim_cur: 2, rlim_max: 2 };
            libc::setrlimit(libc::RLIMIT_CPU, &cpu);
            let it = libc::itimerval {
                it_interval: libc::timeval { tv_sec: 0, tv_usec: 0 },
                it_value: libc::timeval {
                    tv_sec: (budget_ms / 1000) as libc::time_t,
                    tv_usec: ((budget_ms % 1000) * 1000) as libc::suseconds_t,
                },
            };
            libc::setitimer(libc::ITIMER_REAL, &it, std::ptr::null_mut());

            let mut outs = vec![CallOut::default(); reps];
            for o in outs.iter_mut() {
                fill_arena(obj, fill);
                oracle_call4(f, obj.add(ARENA_MID) as u32, args.as_ptr(), o);
            }
            libc::write(fds[1], outs.as_ptr() as *const c_void, nbytes);
            libc::_exit(0);
        }
    }
    unsafe { libc::close(fds[1]) };

    // Parent-side deadline, generously past the child's own, so a normal timeout is
    // reported as the child's SIGALRM rather than as a kill.
    let mut pfd = libc::pollfd { fd: fds[0], events: libc::POLLIN, revents: 0 };
    let mut buf = vec![0u8; nbytes];
    let mut got = 0usize;
    let mut hard_killed = false;
    let deadline = budget_ms as i32 * reps as i32 + 1500;
    loop {
        let rc = unsafe { libc::poll(&mut pfd, 1, deadline) };
        if rc <= 0 {
            unsafe { libc::kill(pid, libc::SIGKILL) };
            hard_killed = true;
            break;
        }
        let n = unsafe {
            libc::read(fds[0], buf.as_mut_ptr().add(got) as *mut c_void, nbytes - got)
        };
        if n <= 0 {
            break;
        }
        got += n as usize;
        if got == nbytes {
            break;
        }
    }
    unsafe { libc::close(fds[0]) };
    let mut status = 0i32;
    unsafe { libc::waitpid(pid, &mut status, 0) };

    if got == nbytes && !hard_killed {
        let outs = (0..reps)
            .map(|i| unsafe {
                std::ptr::read_unaligned(
                    buf.as_ptr().add(i * std::mem::size_of::<CallOut>()) as *const CallOut,
                )
            })
            .collect();
        return ProbeEnd::Ok(outs);
    }
    if hard_killed {
        return ProbeEnd::Timeout;
    }
    if libc::WIFSIGNALED(status) {
        let sig = libc::WTERMSIG(status);
        // SIGALRM/SIGXCPU are our own guards firing, not a property of the function.
        if sig == libc::SIGALRM || sig == libc::SIGXCPU {
            return ProbeEnd::Timeout;
        }
        return ProbeEnd::Signal(sig);
    }
    ProbeEnd::NoValue
}

/// Characterise every ISLAND by probing it under fork isolation.
///
/// Each candidate is asked the questions that decide whether it is usable as a
/// differential-testing target at all:
///   * does it fault with fabricated inputs, or loop forever, or return cleanly
///   * is it deterministic *within* one process (three back-to-back calls) and *across*
///     processes (a fresh fork with identical inputs) -- these are different questions,
///     because a function accumulating into a global is stable across forks and unstable
///     within one
///   * does the result move when the `this` buffer changes, when the arguments change,
///     or neither
///   * does it return an address inside the mapped image (a locator) rather than a
///     computed value (a formula)
///   * does it return a float in st(0)
///
/// A function that faults, or that ignores everything we can control, cannot be
/// differentially tested from fabricated inputs, and saying so up front is cheaper than
/// discovering it one function at a time.
fn sweep(m: &Mapped, pe: &PeImage, list: &str, out_path: &str) {
    use std::io::Write;
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let obj = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            ARENA,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        ) as *mut u8
    };
    if obj as isize == -1 {
        eprintln!("[oracle] arena mmap failed");
        return;
    }

    let text = std::fs::read_to_string(list).expect("island list");
    let mut out = std::fs::File::create(out_path).expect("create out");
    let img_lo = m.base as u32;
    let img_hi = img_lo.wrapping_add(m.len as u32);
    writeln!(
        out,
        "{{\"kind\":\"header\",\"image_base\":{},\"map_lo\":{},\"map_hi\":{},\"preferred_base\":{},\"arena_lo\":{},\"arena_hi\":{},\"this\":{}}}",
        pe.image_base, img_lo, img_hi, pe.image_base, obj as u32, obj as u32 + ARENA as u32,
        obj as u32 + ARENA_MID as u32
    )
    .ok();
    out.flush().ok();

    let (mut total, mut faulted, mut timedout, mut callable) = (0u32, 0u32, 0u32, 0u32);
    let (mut det_intra, mut det_inter, mut nondet) = (0u32, 0u32, 0u32);
    let (mut v_this, mut v_args, mut v_none) = (0u32, 0u32, 0u32);
    let (mut in_img, mut floats, mut rescued) = (0u32, 0u32, 0u32);
    let t0 = std::time::Instant::now();

    for line in text.lines() {
        // minimal field pull; avoids a JSON dependency in a 32-bit musl build
        let Some(ea) = line.split("\"ea\":\"").nth(1).and_then(|s| s.split('"').next()) else {
            continue;
        };
        if !line.contains("\"class\":\"ISLAND\"") {
            continue;
        }
        let Ok(va) = u32::from_str_radix(ea, 16) else { continue };
        if va < pe.image_base || va - pe.image_base >= m.len as u32 {
            continue;
        }
        total += 1;
        let f = m.addr_of_rva(va - pe.image_base);

        let (sa, sb) = (next(), next());
        let argv_a = [next() as u32, next() as u32, next() as u32, next() as u32];
        let argv_b = [next() as u32, next() as u32, next() as u32, next() as u32];
        let fill_a = Fill::Garbage(sa, sb);
        let fill_b = Fill::Garbage(sb, sa);

        // The zero-fill probe runs for every candidate, faulting or not: it is how we
        // learn how many ISLANDs are blocked only by implausible pointers rather than by
        // needing real game state.
        let zero = probe_calls(f, obj, Fill::Zero, [0; 4], 1, 300);
        let (zero_ok, r_zero) = match &zero {
            ProbeEnd::Ok(v) => (true, v[0].eax),
            _ => (false, 0),
        };

        // Probe 1: three back-to-back calls in one process -> intra-process determinism.
        let p1 = probe_calls(f, obj, fill_a, argv_a, 3, 400);
        let outs = match p1 {
            ProbeEnd::Ok(v) => v,
            ProbeEnd::Signal(sig) => {
                faulted += 1;
                if zero_ok {
                    rescued += 1;
                }
                writeln!(out, "{{\"kind\":\"fn\",\"ea\":\"{ea}\",\"status\":\"fault\",\"signal\":{sig},\"zero_ok\":{zero_ok},\"r_zero\":{r_zero}}}").ok();
                out.flush().ok();
                continue;
            }
            ProbeEnd::Timeout => {
                timedout += 1;
                if zero_ok {
                    rescued += 1;
                }
                writeln!(out, "{{\"kind\":\"fn\",\"ea\":\"{ea}\",\"status\":\"timeout\",\"zero_ok\":{zero_ok},\"r_zero\":{r_zero}}}").ok();
                out.flush().ok();
                continue;
            }
            ProbeEnd::NoValue => {
                faulted += 1;
                writeln!(out, "{{\"kind\":\"fn\",\"ea\":\"{ea}\",\"status\":\"novalue\",\"zero_ok\":{zero_ok},\"r_zero\":{r_zero}}}").ok();
                out.flush().ok();
                continue;
            }
        };
        callable += 1;
        let r1 = outs[0];
        let intra = outs.iter().all(|o| o.eax == r1.eax && o.edx == r1.edx);
        if intra {
            det_intra += 1;
        }

        // Probe 2: a fresh process, identical inputs -> inter-process determinism.
        let inter = matches!(probe_calls(f, obj, fill_a, argv_a, 1, 300),
            ProbeEnd::Ok(ref v) if v[0].eax == r1.eax && v[0].edx == r1.edx);
        if inter {
            det_inter += 1;
        }
        if !(intra && inter) {
            nondet += 1;
        }

        // Probe 3 / 4: move the `this` buffer alone, then the arguments alone.
        let vt = matches!(probe_calls(f, obj, fill_b, argv_a, 1, 300),
            ProbeEnd::Ok(ref v) if v[0].eax != r1.eax || v[0].edx != r1.edx);
        let va_ = matches!(probe_calls(f, obj, fill_a, argv_b, 1, 300),
            ProbeEnd::Ok(ref v) if v[0].eax != r1.eax || v[0].edx != r1.edx);
        if vt {
            v_this += 1;
        }
        if va_ {
            v_args += 1;
        }
        if !vt && !va_ {
            v_none += 1;
        }

        let in_image = r1.eax >= img_lo && r1.eax < img_hi;
        if in_image {
            in_img += 1;
        }
        let is_float = r1.has_float != 0;
        if is_float {
            floats += 1;
        }
        // Rebase an in-image return to the preferred base so it is comparable with every
        // static address in the repo.
        let rebased = if in_image {
            r1.eax.wrapping_sub(img_lo).wrapping_add(pe.image_base)
        } else {
            0
        };
        let in_arena = r1.eax >= obj as u32 && r1.eax < obj as u32 + ARENA as u32;

        writeln!(out,
            "{{\"kind\":\"fn\",\"ea\":\"{ea}\",\"status\":\"ok\",\"eax\":{},\"edx\":{},\
             \"det_intra\":{intra},\"det_inter\":{inter},\
             \"varies_this\":{vt},\"varies_args\":{va_},\
             \"in_image\":{in_image},\"rebased\":{rebased},\"in_arena\":{in_arena},\
             \"is_float\":{is_float},\"fret\":{:?},\"zero_ok\":{zero_ok},\"r_zero\":{r_zero}}}",
            r1.eax, r1.edx,
            if is_float && r1.fret.is_finite() { r1.fret } else { 0.0 }
        ).ok();
        out.flush().ok();

        if total % 100 == 0 {
            eprintln!(
                "[oracle] {total} probed, {:.0}s elapsed, {callable} callable, {faulted} fault, {timedout} timeout",
                t0.elapsed().as_secs_f64()
            );
        }
    }
    unsafe { libc::munmap(obj as *mut c_void, ARENA) };
    let summary = format!(
        "{{\"kind\":\"summary\",\"total\":{total},\"callable\":{callable},\"faulted\":{faulted},\
         \"timedout\":{timedout},\"det_intra\":{det_intra},\"det_inter\":{det_inter},\
         \"nondeterministic\":{nondet},\"varies_this\":{v_this},\"varies_args\":{v_args},\
         \"varies_neither\":{v_none},\"returns_in_image\":{in_img},\"returns_float\":{floats},\
         \"fault_but_zero_ok\":{rescued},\"elapsed_s\":{:.1}}}",
        t0.elapsed().as_secs_f64()
    );
    writeln!(out, "{summary}").ok();
    out.flush().ok();
    println!("{summary}");
    println!("wrote {out_path}");
}


/// Re-test cross-process determinism with *matched* probe parameters.
///
/// The sweep compares a 3-repetition child against a 1-repetition child. Those two
/// children do not have identical stacks -- the harness allocates a different result
/// buffer in each -- so a callee that reads uninitialised stack below its own frame, or
/// returns a stack address, looks non-deterministic when it is merely uninitialised.
/// This re-runs the flagged candidates as four *identical* children and reports every
/// return value, which separates "genuinely non-deterministic" from "reads stack
/// garbage" and from "returns a stack pointer".
fn recheck(m: &Mapped, pe: &PeImage, inp: &str, out_path: &str) {
    use std::io::Write;
    let text = std::fs::read_to_string(inp).expect("sweep file");
    let mut out = std::fs::File::create(out_path).expect("create out");
    let obj = unsafe {
        libc::mmap(std::ptr::null_mut(), ARENA, libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS, -1, 0) as *mut u8
    };
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        state ^= state << 13; state ^= state >> 7; state ^= state << 17; state
    };
    let (mut n, mut agree, mut stackish) = (0u32, 0u32, 0u32);
    for line in text.lines() {
        if !line.contains("\"status\":\"ok\"") { continue; }
        let Some(ea) = line.split("\"ea\":\"").nth(1).and_then(|s| s.split('"').next()) else { continue };
        let Ok(va) = u32::from_str_radix(ea, 16) else { continue };
        if va < pe.image_base || va - pe.image_base >= m.len as u32 { continue; }
        let f = m.addr_of_rva(va - pe.image_base);
        // One fixed environment, reused for every child: nothing varies but the fork.
        let (sa, sb) = (next(), next());
        let argv = [next() as u32, next() as u32, next() as u32, next() as u32];
        let mut vals = Vec::new();
        for _ in 0..4 {
            match probe_calls(f, obj, Fill::Garbage(sa, sb), argv, 1, 300) {
                ProbeEnd::Ok(v) => vals.push(v[0].eax),
                _ => { vals.clear(); break; }
            }
        }
        if vals.len() != 4 { continue; }
        n += 1;
        let same = vals.iter().all(|v| *v == vals[0]);
        if same { agree += 1; }
        // A return that differs only in its low bits, and is near the child stack, is a
        // stack address rather than a computed value.
        let spread = vals.iter().max().unwrap() - vals.iter().min().unwrap();
        let high = vals.iter().all(|v| *v > 0xb000_0000);
        if !same && high && spread < 0x10_0000 { stackish += 1; }
        writeln!(out, "{{\"ea\":\"{ea}\",\"matched_agree\":{same},\"vals\":[{},{},{},{}]}}",
            vals[0], vals[1], vals[2], vals[3]).ok();
    }
    unsafe { libc::munmap(obj as *mut c_void, ARENA) };
    let s = format!("{{\"kind\":\"summary\",\"rechecked\":{n},\"agree_with_matched_probes\":{agree},\
        \"disagree\":{},\"of_which_look_like_stack_addresses\":{stackish}}}", n - agree);
    writeln!(out, "{s}").ok();
    println!("{s}");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: oracle selftest | oracle call <rva-hex> [arg0 arg1 ...] | oracle info");
        std::process::exit(2);
    }

    if args[1] == "selftest" {
        match selftest() {
            Ok(()) => return,
            Err(e) => {
                eprintln!("selftest FAILED: {e}");
                std::process::exit(1);
            }
        }
    }

    let bytes = std::fs::read("data/riseofnations.exe").expect("read image");
    let (m, pe) = Mapped::load(&bytes).expect("load");

    match args[1].as_str() {
        "info" => {
            println!("image_base   {:#010x}", pe.image_base);
            println!("entry rva    {:#010x}", pe.entry_point_rva);
            println!("size_of_image {:#x}", pe.size_of_image);
            for s in &pe.sections {
                println!(
                    "  {:<9} rva={:#010x} vsize={:>10} x={} w={}",
                    s.name,
                    s.virtual_address,
                    s.virtual_size,
                    s.is_executable(),
                    s.is_writable()
                );
            }
        }
        "call" => {
            let va = u32::from_str_radix(args[2].trim_start_matches("0x"), 16).expect("hex rva/va");
            // accept either an RVA or a preferred-base VA
            let rva = if va >= pe.image_base { va - pe.image_base } else { va };
            let a: Vec<u32> = args[3..]
                .iter()
                .map(|s| {
                    let s = s.trim_start_matches("0x");
                    u32::from_str_radix(s, 16).unwrap_or_else(|_| s.parse().unwrap_or(0))
                })
                .collect();
            let p = m.addr_of_rva(rva);
            println!("calling rva {rva:#010x} (va {:#010x}) at {p:p}", rva + pe.image_base);
            let f: extern "C" fn(u32, u32, u32, u32) -> u32 = unsafe { std::mem::transmute(p) };
            let (a0, a1, a2, a3) = (
                a.first().copied().unwrap_or(0),
                a.get(1).copied().unwrap_or(0),
                a.get(2).copied().unwrap_or(0),
                a.get(3).copied().unwrap_or(0),
            );
            match in_child(|| f(a0, a1, a2, a3)) {
                Ok(v) => println!("returned {v} ({v:#010x})"),
                Err(e) => println!("FAULTED: {e}"),
            }
        }
        "vectors" => {
            // Print retail outputs for fixed inputs, so test expectations are captured
            // from the binary rather than hand-computed.
            let f = m.addr_of_rva(0x0084_6450 - pe.image_base);
            let g: extern "stdcall" fn(i32, i32, i32, i32) -> i32 = unsafe { std::mem::transmute(f) };
            let cases: [(i32, i32, i32, i32); 8] = [
                (0, 0, 0, 0), (1, 1, 0, 1), (-1, -1, -5, 5), (7, -3, -100, 100),
                (123456, 789, 0, 0), (5, 5, 10, -10), (100, 3, 1, 6), (-9, 4, 0, 100),
            ];
            for (a, b, lo, hi) in cases {
                println!("assert_eq!(hash_into_range({a}, {b}, {lo}, {hi}), {});", g(a, b, lo, hi));
            }
        }
        "sweep" => {
            let list = args.get(2).map(|s| s.as_str()).unwrap_or("islands.jsonl");
            let outp = args.get(3).map(|s| s.as_str()).unwrap_or("sweep.jsonl");
            sweep(&m, &pe, list, outp);
        }
        "recheck" => {
            let inp = args.get(2).map(|s| s.as_str()).unwrap_or("sweep.jsonl");
            let outp = args.get(3).map(|s| s.as_str()).unwrap_or("recheck.jsonl");
            recheck(&m, &pe, inp, outp);
        }
        "combat" => {
            let n: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(500_000);
            println!("combat differential test: retail machine code vs Rust model");
            let ok = combat_difftest(&m, &pe, n);
            if !ok {
                std::process::exit(1);
            }
        }
        "damage" | "damage-vectors" => {
            // FUN_00644130 is not an ISLAND: it walks two objects, four vtables, the
            // RULES singleton, the game object, the player array, the map, two object
            // tables and a city table. damage_env builds all of that; see its header for
            // what the construction does and does not buy us.
            let reloc = |va: u32| m.addr_of_rva(va - pe.image_base) as u32;
            let wr32 = |va: u32, v: u32| unsafe {
                std::ptr::write_unaligned(m.addr_of_rva(va - pe.image_base) as *mut u32, v)
            };
            let wr16 = |va: u32, v: u16| unsafe {
                std::ptr::write_unaligned(m.addr_of_rva(va - pe.image_base) as *mut u16, v)
            };
            let mut arena = damage_env::Arena::new().expect("arena");
            damage_env::build(&mut arena, &reloc);
            damage_test::install_globals(&arena, &wr32);
            let f = reloc(damage_env::VA_DAMAGE);

            if args[1] == "damage-vectors" {
                damage_test::print_vectors(&arena, f, &wr32, &wr16);
            } else {
                let n: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(200_000);
                let seed: u64 = args
                    .get(3)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0x2545_F491_4F6C_DD1D);
                println!("damage differential test: retail FUN_00644130 vs don_sim::damage");
                println!("  fabricated world at {:#010x}, seed {seed:#x}", arena.addr(0));
                let rep = damage_test::run(&arena, f, n, seed, &wr32, &wr16);
                damage_test::print_report(&rep);
                if rep.mismatches != 0 || rep.unexpected_panics != 0 {
                    std::process::exit(1);
                }
            }
        }
        "difftest" => {
            let n: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(100_000);
            println!("differential test: retail machine code vs Rust model");
            let ok = difftest(&m, &pe, n);
            if !ok { std::process::exit(1); }
        }
        other => eprintln!("unknown command {other}"),
    }
}
