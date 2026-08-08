//! Binary oracle harness: map riseofnations.exe and call retail functions directly.
//!
//! Must run as a 32-bit x86 process (i686-unknown-linux-musl). Each call happens in a
//! forked child so that probing an unknown function cannot take down the harness --
//! essential when we do not yet know which functions are reachable with fabricated
//! inputs (see the ISLAND / SELF-CALL / DATA-ONLY taxonomy in docs/oracle-architecture.md).

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


/// Characterise a function by probing it under fork isolation.
///
/// For each candidate we ask three questions that decide whether it is usable as a
/// differential-testing target at all:
///   * does it fault with fabricated inputs (needs live globals -> not usable)
///   * is it deterministic (same inputs twice -> same output)
///   * does the output actually depend on the arguments, or on the `this` buffer
///
/// A function that faults, or that ignores everything we can control, cannot be
/// differentially tested from fabricated inputs, and saying so up front is cheaper than
/// discovering it one function at a time.
fn sweep(m: &Mapped, pe: &PeImage, list: &str, out_path: &str) {
    use std::io::Write;
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        state ^= state << 13; state ^= state >> 7; state ^= state << 17; state
    };
    let obj = unsafe {
        libc::mmap(std::ptr::null_mut(), PAGE * 4,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS, -1, 0) as *mut u8
    };

    let text = std::fs::read_to_string(list).expect("island list");
    let mut out = std::fs::File::create(out_path).expect("create out");
    let (mut faulted, mut det, mut arg_dep, mut this_dep, mut total) = (0, 0, 0, 0, 0);

    for line in text.lines() {
        // minimal field pull; avoids a JSON dependency in a 32-bit musl build
        let Some(ea) = line.split("\"ea\":\"").nth(1).and_then(|s| s.split('"').next()) else { continue };
        if !line.contains("\"class\":\"ISLAND\"") { continue; }
        let Ok(va) = u32::from_str_radix(ea, 16) else { continue };
        if va < pe.image_base { continue; }
        total += 1;

        let f = m.addr_of_rva(va - pe.image_base);
        let seed_a = next();
        let seed_b = next();

        // probe in a child: same inputs twice, then varied args, then varied this
        let probe = |sa: u64, sb: u64| -> Result<u32, String> {
            in_child(move || {
                unsafe {
                    for i in 0..(PAGE * 4) / 8 {
                        std::ptr::write_unaligned(
                            (obj as *mut u64).add(i),
                            sa.wrapping_mul(i as u64 + 1) ^ sb,
                        );
                    }
                }
                let g: extern "C" fn(u32, u32, u32, u32) -> u32 = unsafe { std::mem::transmute(f) };
                let r: u32;
                unsafe {
                    std::arch::asm!("call {f}", f = in(reg) f,
                        in("ecx") obj, lateout("eax") r, clobber_abi("C"));
                }
                let _ = g;
                r
            })
        };

        let r1 = probe(seed_a, seed_b);
        if let Err(e) = &r1 {
            writeln!(out, "{{\"ea\":\"{ea}\",\"status\":\"fault\",\"detail\":\"{e}\"}}").ok();
            faulted += 1;
            continue;
        }
        let r1 = r1.unwrap();
        let r1b = probe(seed_a, seed_b);
        let deterministic = matches!(r1b, Ok(v) if v == r1);
        if deterministic { det += 1; }
        let r2 = probe(seed_b, seed_a);
        let varies = matches!(r2, Ok(v) if v != r1);
        if varies { this_dep += 1; }
        let _ = &mut arg_dep;

        writeln!(out,
            "{{\"ea\":\"{ea}\",\"status\":\"ok\",\"det\":{deterministic},\"varies_with_state\":{varies},\"r\":{r1}}}"
        ).ok();
    }
    unsafe { libc::munmap(obj as *mut c_void, PAGE * 4) };
    println!("sweep: {total} ISLANDs probed | {faulted} faulted | {det} deterministic | {this_dep} vary with input state");
    println!("wrote {out_path}");
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
        "difftest" => {
            let n: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(100_000);
            println!("differential test: retail machine code vs Rust model");
            let ok = difftest(&m, &pe, n);
            if !ok { std::process::exit(1); }
        }
        other => eprintln!("unknown command {other}"),
    }
}
